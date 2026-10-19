//! Ordre d'exécution déduit des données, en Markdown : les vagues, ce qui doit passer
//! avant chaque package, les cycles, les tables écartées et les sources à charger.

use std::collections::{BTreeMap, BTreeSet};

use super::html::project_of;
use super::{clean, plural, qualified};
use crate::inventory::Inventory;
use crate::lineage::order::{ExecutionOrder, HUB_WRITERS};

/// Tables citées pour une dépendance : les premières, le reste compté.
const SHOWN_TABLES: usize = 3;

pub fn order_markdown(inventory: &Inventory, order: &ExecutionOrder) -> String {
    let name = |p: usize| {
        inventory.packages.get(p).map_or_else(
            || "?".to_owned(),
            |pkg| qualified(project_of(&pkg.file), &pkg.name),
        )
    };
    let mut out = String::from(
        "# Ordre déduit des données\n\nUn package passe après ceux qui écrivent les tables qu'il \
         lit. Les MERGE, UPDATE et procédures sans définition ne sont pas suivis : c'est un ordre \
         minimal, à confronter aux jobs.\n\n",
    );
    if order.waves.is_empty() {
        out.push_str("Aucune dépendance de données entre les packages.\n\n");
    }
    for (i, wave) in order.waves.iter().enumerate() {
        let count = wave.iter().map(Vec::len).sum();
        out.push_str(&format!(
            "## Vague {} ({})\n\n",
            i + 1,
            plural(count, "package")
        ));
        for group in wave {
            // Ce qui relie le groupe à lui-même (cycle), et ce qui doit passer avant lui.
            let mut inside: BTreeSet<&str> = BTreeSet::new();
            let mut before: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
            for (writer, tables) in group.iter().filter_map(|p| order.after.get(p)).flatten() {
                let tables = tables.iter().map(String::as_str);
                if group.contains(writer) {
                    inside.extend(tables);
                } else {
                    before.entry(*writer).or_default().extend(tables);
                }
            }
            let names: Vec<String> = group.iter().map(|&p| name(p)).collect();
            let mut line = match names.as_slice() {
                [one] => one.clone(),
                _ => format!(
                    "Cycle : {} — dépendent les uns des autres ({})",
                    names.join(", "),
                    some(&inside)
                ),
            };
            if !before.is_empty() {
                let list: Vec<String> = before
                    .iter()
                    .map(|(writer, tables)| format!("{} ({})", name(*writer), some(tables)))
                    .collect();
                line.push_str(&format!(" — après {}", list.join(", ")));
            }
            out.push_str(&format!("- {}\n", clean(&line)));
        }
        out.push('\n');
    }
    let independent: Vec<String> = order.independent.iter().map(|&p| name(p)).collect();
    section(&mut out, "Sans dépendance de données", "", &independent);
    let hubs: Vec<String> = order
        .hubs
        .iter()
        .map(|(table, n)| format!("{table} — écrite par {}", plural(*n, "package")))
        .collect();
    let why = format!(
        "Écrites par au moins {HUB_WRITERS} packages (journal, paramètres…) : elles relieraient \
         tout à tout, elles ne comptent pas dans l'ordre.\n\n"
    );
    section(&mut out, "Tables partagées écartées", &why, &hubs);
    section(
        &mut out,
        "Lues, écrites par aucun package",
        "À charger avant la bascule : données externes, ou écrites par du SQL non suivi.\n\n",
        &order.sources,
    );
    out
}

fn section(out: &mut String, title: &str, intro: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("## {title} ({})\n\n{intro}", items.len()));
    for item in items {
        out.push_str(&format!("- {}\n", clean(item)));
    }
    out.push('\n');
}

/// `a, b, c +2` : les premières tables, le reste compté.
fn some(tables: &BTreeSet<&str>) -> String {
    let shown: Vec<&str> = tables.iter().take(SHOWN_TABLES).copied().collect();
    match tables.len().saturating_sub(SHOWN_TABLES) {
        0 => shown.join(", "),
        more => format!("{} +{more}", shown.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lineage::lineage;
    use crate::lineage::order::execution_order;

    #[test]
    fn explains_each_wave_cycle_shared_table_and_source() {
        let (inventory, _) = crate::inventory(std::path::Path::new("tests/fixtures/ordre"));

        let text = order_markdown(&inventory, &execution_order(&lineage(&inventory)));

        for part in [
            "## Vague 1 (3 packages)\n\n- Cycle : Aller, Retour — dépendent les uns des autres \
             (Entrepot : dbo.A, Entrepot : dbo.B)\n- Extraire\n",
            "## Vague 2 (1 package)\n\n- Charger — après Extraire (Entrepot : stg.Clients)\n",
            "## Vague 3 (1 package)\n\n- Exporter — après Charger (Entrepot : dbo.Clients)\n",
            "## Sans dépendance de données (2)\n\n- Isole\n- Surveiller\n",
            "- Entrepot : dbo.Journal (srv-demo) — écrite par 5 packages\n",
            "## Lues, écrites par aucun package (1)",
            "- Entrepot : ext.Clients (srv-demo)\n",
        ] {
            assert!(text.contains(part), "{part}\n---\n{text}");
        }
    }
}
