//! Comparaison du lignage de deux analyses, avant et après la bascule : les liens colonne
//! à colonne du mapping, serveurs mis à part, puis les serveurs passés de l'un à l'autre.

use std::collections::{BTreeMap, BTreeSet};

use super::html::project_of;
use super::mapping::{mapping_rows, Row, STATUSES};
use super::{clean, plural, qualified};
use crate::inventory::Inventory;
use crate::lineage::columns::column_lineage;

/// Ce qui désigne un lien d'une analyse à l'autre : projet, package, tâche, colonne cible
/// et colonne source, sans casse (SQL Server ne la distingue pas). Pas les serveurs : ce
/// sont eux qui changent à la bascule.
type Key = [String; 5];
const KEY: [usize; 5] = [0, 1, 2, 3, 5];
// Autres cellules d'une ligne du mapping (ordre de `HEADER` dans mapping.rs).
const TARGET_SERVER: usize = 4;
const SOURCE_SERVER: usize = 6;
const HOW: usize = 7;
const STATUS: usize = 8;

struct Analysis {
    /// (projet, package) sans casse → libellé.
    packages: BTreeMap<(String, String), String>,
    /// Un lien peut passer par plusieurs chemins : une ligne par chemin.
    links: BTreeMap<Key, Vec<Row>>,
    rows: usize,
}

fn analysis(inventory: &Inventory) -> Analysis {
    let rows = mapping_rows(inventory, &column_lineage(inventory));
    let count = rows.len();
    let mut links: BTreeMap<Key, Vec<Row>> = BTreeMap::new();
    for row in rows {
        links
            .entry(KEY.map(|i| row[i].to_lowercase()))
            .or_default()
            .push(row);
    }
    let packages = inventory
        .packages
        .iter()
        .map(|p| {
            let project = project_of(&p.file);
            let key = (project.to_lowercase(), p.name.to_lowercase());
            (key, qualified(project, &p.name))
        })
        .collect();
    Analysis {
        packages,
        links,
        rows: count,
    }
}

/// Le compte rendu en Markdown, et s'il y a régression (lien ou package disparu, statut
/// dégradé).
pub fn mapping_diff(before: &Inventory, after: &Inventory) -> (String, bool) {
    let (before, after) = (analysis(before), analysis(after));
    let only = |a: &Analysis, b: &Analysis| -> Vec<String> {
        a.packages
            .iter()
            .filter(|(key, _)| !b.packages.contains_key(*key))
            .map(|(_, label)| label.clone())
            .collect()
    };
    let (gone, new) = (only(&before, &after), only(&after, &before));
    // Les liens d'un package apparu ou disparu sont comptés avec le package, pas un à un.
    let shared = |key: &Key| {
        let package = (key[0].clone(), key[1].clone());
        before.packages.contains_key(&package) && after.packages.contains_key(&package)
    };
    let (mut removed, mut added, mut changed) = (Vec::new(), Vec::new(), Vec::new());
    let mut worse = false;
    let mut servers: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (key, old) in before.links.iter().filter(|(key, _)| shared(key)) {
        let Some(now) = after.links.get(key) else {
            removed.push(format!("{} ({})", link(&old[0]), values(old)));
            continue;
        };
        if values(old) != values(now) {
            changed.push(format!(
                "{} : {} ⇒ {}",
                link(&old[0]),
                values(old),
                values(now)
            ));
            worse |= rank(now) > rank(old);
        }
        for column in [TARGET_SERVER, SOURCE_SERVER] {
            let pair = (old[0][column].clone(), now[0][column].clone());
            if !pair.0.is_empty() || !pair.1.is_empty() {
                *servers.entry(pair).or_default() += 1;
            }
        }
    }
    for (_, now) in after.links.iter().filter(|(key, _)| shared(key)) {
        if !before
            .links
            .contains_key(&KEY.map(|i| now[0][i].to_lowercase()))
        {
            added.push(format!("{} ({})", link(&now[0]), values(now)));
        }
    }
    let mut out = format!(
        "# Comparaison du lignage\n\n{} avant, {} après (serveurs mis à part).\n\n",
        plural(before.rows, "lien"),
        plural(after.rows, "lien")
    );
    for (title, items) in [
        ("Packages disparus", &gone),
        ("Packages nouveaux", &new),
        ("Liens disparus", &removed),
        ("Liens modifiés", &changed),
        ("Liens nouveaux", &added),
    ] {
        section(&mut out, title, items);
    }
    if [&gone, &new, &removed, &changed, &added]
        .iter()
        .all(|items| items.is_empty())
    {
        out.push_str("Aucune différence de lignage.\n\n");
    }
    // Un package resté sur l'ancien serveur se voit ici : `ancien → ancien`.
    let mut moves: Vec<_> = servers.into_iter().collect();
    moves.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let moves: Vec<String> = moves
        .iter()
        .map(|((old, now), n)| {
            format!(
                "{} → {} : {}",
                or_dash(old),
                or_dash(now),
                plural(*n, "lien")
            )
        })
        .collect();
    section(&mut out, "Serveurs, avant → après", &moves);
    (out, worse || !gone.is_empty() || !removed.is_empty())
}

fn section(out: &mut String, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("## {title} ({})\n\n", items.len()));
    for item in items {
        out.push_str(&format!("- {}\n", clean(item)));
    }
    out.push('\n');
}

/// `Package › Tâche : cible ← source`, la source vide si l'origine est inconnue.
fn link(row: &Row) -> String {
    format!(
        "{} › {} : {} ← {}",
        qualified(&row[0], &row[1]),
        row[2],
        row[3],
        or_dash(&row[5])
    )
}

/// Transformations et statut de chaque chemin : `lecture → écriture, directe`.
fn values(rows: &[Row]) -> String {
    let all: BTreeSet<String> = rows
        .iter()
        .map(|r| format!("{}, {}", r[HOW], r[STATUS]))
        .collect();
    all.into_iter().collect::<Vec<_>>().join(" ; ")
}

/// Le moins sûr des statuts d'un lien (0 : directe … 3 : origine inconnue).
fn rank(rows: &[Row]) -> usize {
    rows.iter()
        .filter_map(|r| STATUSES.iter().position(|s| *s == r[STATUS]))
        .max()
        .unwrap_or_default()
}

fn or_dash(text: &str) -> &str {
    if text.is_empty() {
        "—"
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysed(dir: &str) -> Inventory {
        crate::inventory(std::path::Path::new(dir)).0
    }

    #[test]
    fn the_same_folder_twice_has_no_difference() {
        let same = analysed("tests/fixtures/colonnes_sql");

        let (report, regression) = mapping_diff(&same, &same);

        assert!(!regression);
        assert!(report.contains("Aucune différence de lignage."), "{report}");
    }

    #[test]
    fn lists_what_disappeared_changed_and_which_servers_moved() {
        let (report, regression) = mapping_diff(
            &analysed("tests/fixtures/diff/avant"),
            &analysed("tests/fixtures/diff/apres"),
        );

        assert!(regression, "un lien et un package ont disparu");
        assert!(
            report.contains("## Packages disparus (1)\n\n- Demo_Minimal\n"),
            "{report}"
        );
        assert!(
            report.contains(
                "## Liens disparus (1)\n\n- Chargement › SQL Resumer : Entrepot : dbo.Resume.numero \
                 ← Entrepot : stg.Clients.id (lecture → écriture, directe)\n"
            ),
            "{report}"
        );
        // `SELECT *` remplacé par une liste de colonnes : même lien, mieux suivi.
        assert!(report.contains("## Liens modifiés (2)"), "{report}");
        assert!(
            report.contains(
                "- Chargement › SQL Archiver : Entrepot : dbo.Archive.email ← Entrepot : \
                 dbo.Clients.email : SELECT * → écriture, par nom (SELECT *) ⇒ lecture → \
                 écriture, directe\n"
            ),
            "{report}"
        );
        // Le serveur seul ne fait pas un lien différent ; il est compté à part.
        assert!(!report.contains("## Liens nouveaux"), "{report}");
        assert!(report.contains("- srv-demo → srv-neuf : "), "{report}");
    }

    #[test]
    fn a_link_whose_status_gets_worse_is_a_regression() {
        // Sens inverse : Archiver repasse à `SELECT *`, le lien de `numero` réapparaît.
        let (report, regression) = mapping_diff(
            &analysed("tests/fixtures/diff/apres"),
            &analysed("tests/fixtures/diff/avant"),
        );

        assert!(regression, "statut dégradé sans rien de disparu\n{report}");
        assert!(report.contains("## Liens nouveaux (1)"), "{report}");
        assert!(report.contains("## Packages nouveaux (1)"), "{report}");
        assert!(!report.contains("disparus"), "{report}");
    }

    #[test]
    fn never_prints_connection_strings_or_passwords() {
        let (report, _) = mapping_diff(
            &analysed("tests/fixtures/lineage"),
            &analysed("tests/fixtures/diff/avant"),
        );

        let lower = report.to_lowercase();
        assert!(
            !lower.contains("password") && !lower.contains("pwd="),
            "{report}"
        );
    }
}
