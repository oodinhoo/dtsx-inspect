//! Lineage comparison of two analyses, before and after a migration: the column-to-column
//! links of the mapping, servers aside, then the servers moved from one to the other.

use std::collections::{BTreeMap, BTreeSet};

use super::html::project_of;
use super::mapping::{mapping_rows, Row, STATUSES};
use super::{clean, plural, qualified};
use crate::inventory::Inventory;
use crate::lineage::columns::column_lineage;

/// What designates a link from one analysis to the other: project, package, task, target
/// column and source column, case-insensitive (SQL Server does not tell case apart). Not
/// the servers: they are what changes in a migration.
type Key = [String; 5];
const KEY: [usize; 5] = [0, 1, 2, 3, 5];
// Other cells of a mapping row (order of `HEADER` in mapping.rs).
const TARGET_SERVER: usize = 4;
const SOURCE_SERVER: usize = 6;
const HOW: usize = 7;
const STATUS: usize = 8;

struct Analysis {
    /// (project, package) case-insensitive → label.
    packages: BTreeMap<(String, String), String>,
    /// A link can go through several paths: one row per path.
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

/// The Markdown report, and whether there is a regression (link or package removed,
/// status degraded).
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
    // The links of an added or removed package are counted with the package, not one by one.
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
                "{}: {} ⇒ {}",
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
        "# Lineage comparison\n\n{} before, {} after (servers aside).\n\n",
        plural(before.rows, "link"),
        plural(after.rows, "link")
    );
    for (title, items) in [
        ("Removed packages", &gone),
        ("New packages", &new),
        ("Removed links", &removed),
        ("Changed links", &changed),
        ("New links", &added),
    ] {
        section(&mut out, title, items);
    }
    if [&gone, &new, &removed, &changed, &added]
        .iter()
        .all(|items| items.is_empty())
    {
        out.push_str("No lineage difference.\n\n");
    }
    // A package left on the old server shows here: `old → old`.
    let mut moves: Vec<_> = servers.into_iter().collect();
    moves.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let moves: Vec<String> = moves
        .iter()
        .map(|((old, now), n)| {
            format!(
                "{} → {}: {}",
                or_dash(old),
                or_dash(now),
                plural(*n, "link")
            )
        })
        .collect();
    section(&mut out, "Servers, before → after", &moves);
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

/// `Package › Task: target ← source`, the source empty when the origin is unknown.
fn link(row: &Row) -> String {
    format!(
        "{} › {}: {} ← {}",
        qualified(&row[0], &row[1]),
        row[2],
        row[3],
        or_dash(&row[5])
    )
}

/// Transformations and status of each path: `read → write, direct`.
fn values(rows: &[Row]) -> String {
    let all: BTreeSet<String> = rows
        .iter()
        .map(|r| format!("{}, {}", r[HOW], r[STATUS]))
        .collect();
    all.into_iter().collect::<Vec<_>>().join("; ")
}

/// The least safe status of a link (0: direct … 3: unknown origin).
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
        let same = analysed("tests/fixtures/columns_sql");

        let (report, regression) = mapping_diff(&same, &same);

        assert!(!regression);
        assert!(report.contains("No lineage difference."), "{report}");
    }

    #[test]
    fn lists_what_disappeared_changed_and_which_servers_moved() {
        let (report, regression) = mapping_diff(
            &analysed("tests/fixtures/diff/before"),
            &analysed("tests/fixtures/diff/after"),
        );

        assert!(regression, "a link and a package were removed");
        assert!(
            report.contains("## Removed packages (1)\n\n- Demo_Minimal\n"),
            "{report}"
        );
        assert!(
            report.contains(
                "## Removed links (1)\n\n- Chargement › SQL Resumer: Entrepot : dbo.Resume.numero \
                 ← Entrepot : stg.Clients.id (read → write, direct)\n"
            ),
            "{report}"
        );
        // `SELECT *` replaced by a column list: same link, better traced.
        assert!(report.contains("## Changed links (2)"), "{report}");
        assert!(
            report.contains(
                "- Chargement › SQL Archiver: Entrepot : dbo.Archive.email ← Entrepot : \
                 dbo.Clients.email: SELECT * → write, by name (SELECT *) ⇒ read → \
                 write, direct\n"
            ),
            "{report}"
        );
        // The server alone does not make a different link; it is counted apart.
        assert!(!report.contains("## New links"), "{report}");
        assert!(report.contains("- srv-demo → srv-neuf: "), "{report}");
    }

    #[test]
    fn a_link_whose_status_gets_worse_is_a_regression() {
        // Reverse direction: Archiver goes back to `SELECT *`, the `numero` link reappears.
        let (report, regression) = mapping_diff(
            &analysed("tests/fixtures/diff/after"),
            &analysed("tests/fixtures/diff/before"),
        );

        assert!(regression, "status degraded with nothing removed\n{report}");
        assert!(report.contains("## New links (1)"), "{report}");
        assert!(report.contains("## New packages (1)"), "{report}");
        assert!(!report.contains("Removed"), "{report}");
    }

    #[test]
    fn never_prints_connection_strings_or_passwords() {
        let (report, _) = mapping_diff(
            &analysed("tests/fixtures/lineage"),
            &analysed("tests/fixtures/diff/before"),
        );

        let lower = report.to_lowercase();
        assert!(
            !lower.contains("password") && !lower.contains("pwd="),
            "{report}"
        );
    }
}
