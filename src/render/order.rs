//! Execution order inferred from the data, as Markdown: the waves, what must run before
//! each package, the cycles, the tables set aside and the sources to load.

use std::collections::{BTreeMap, BTreeSet};

use super::html::project_of;
use super::{clean, plural, qualified};
use crate::inventory::Inventory;
use crate::lineage::order::{ExecutionOrder, HUB_WRITERS};

/// Tables named for a dependency: the first ones, the rest counted.
const SHOWN_TABLES: usize = 3;

pub fn order_markdown(inventory: &Inventory, order: &ExecutionOrder) -> String {
    let name = |p: usize| {
        inventory.packages.get(p).map_or_else(
            || "?".to_owned(),
            |pkg| qualified(project_of(&pkg.file), &pkg.name),
        )
    };
    let mut out = String::from(
        "# Order inferred from the data\n\nA package runs after those writing the tables it \
         reads. MERGE, UPDATE and procedures without definition are not traced: this is a \
         minimal order, to check against the jobs.\n\n",
    );
    if order.waves.is_empty() {
        out.push_str("No data dependency between the packages.\n\n");
    }
    for (i, wave) in order.waves.iter().enumerate() {
        let count = wave.iter().map(Vec::len).sum();
        out.push_str(&format!(
            "## Wave {} ({})\n\n",
            i + 1,
            plural(count, "package")
        ));
        for group in wave {
            // What ties the group to itself (cycle), and what must run before it.
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
                    "Cycle: {} — depend on each other ({})",
                    names.join(", "),
                    some(&inside)
                ),
            };
            if !before.is_empty() {
                let list: Vec<String> = before
                    .iter()
                    .map(|(writer, tables)| format!("{} ({})", name(*writer), some(tables)))
                    .collect();
                line.push_str(&format!(" — after {}", list.join(", ")));
            }
            out.push_str(&format!("- {}\n", clean(&line)));
        }
        out.push('\n');
    }
    let independent: Vec<String> = order.independent.iter().map(|&p| name(p)).collect();
    section(&mut out, "No data dependency", "", &independent);
    let hubs: Vec<String> = order
        .hubs
        .iter()
        .map(|(table, n)| format!("{table} — written by {}", plural(*n, "package")))
        .collect();
    let why = format!(
        "Written by at least {HUB_WRITERS} packages (log, settings…): they would tie \
         everything to everything, so they do not count in the order.\n\n"
    );
    section(&mut out, "Shared tables set aside", &why, &hubs);
    section(
        &mut out,
        "Read, written by no package",
        "To load first: external data, or data written by SQL that is not traced.\n\n",
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

/// `a, b, c +2`: the first tables, the rest counted.
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
        let (inventory, _) = crate::inventory(std::path::Path::new("tests/fixtures/order"));

        let text = order_markdown(&inventory, &execution_order(&lineage(&inventory)));

        for part in [
            "## Wave 1 (3 packages)\n\n- Cycle: Aller, Retour — depend on each other \
             (Entrepot : dbo.A, Entrepot : dbo.B)\n- Extraire\n",
            "## Wave 2 (1 package)\n\n- Charger — after Extraire (Entrepot : stg.Clients)\n",
            "## Wave 3 (1 package)\n\n- Exporter — after Charger (Entrepot : dbo.Clients)\n",
            "## No data dependency (2)\n\n- Isole\n- Surveiller\n",
            "- Entrepot : dbo.Journal (srv-demo) — written by 5 packages\n",
            "## Read, written by no package (1)",
            "- Entrepot : ext.Clients (srv-demo)\n",
        ] {
            assert!(text.contains(part), "{part}\n---\n{text}");
        }
    }
}
