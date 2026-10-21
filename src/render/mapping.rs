//! Source → target mapping sheet as CSV (`;` and UTF-8 BOM, as Excel expects in European
//! locales). One row per table or file column written and per column read giving it,
//! within one package: the package, its task or Data Flow, the transformations and a
//! status.

use std::collections::BTreeSet;

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use super::columns::describe;
use super::html::project_of;
use crate::inventory::Inventory;
use crate::lineage::columns::{ColumnLineage, ColumnNode, How};

const HEADER: [&str; 9] = [
    "Project",
    "Package",
    "Task or Data Flow",
    "Target column",
    "Target server",
    "Source column",
    "Source server",
    "Transformations",
    "Status",
];

/// Statuses of a row, from the safest to the least safe; the last cell of each row.
pub(super) const STATUSES: [&str; 4] = [
    "direct",
    "transformed",
    "by name (SELECT *)",
    "unknown origin",
];

/// A mapping row, cells in the order of `HEADER`.
pub(super) type Row = [String; 9];

/// A column read (none if the step has no known input) and the ways it gives the
/// written column, from the read to the write.
type Origin = (Option<NodeIndex>, Vec<How>);

pub fn mapping_csv(inventory: &Inventory, lineage: &ColumnLineage) -> String {
    to_csv(&mapping_rows(inventory, lineage))
}

/// The mapping rows, sorted: same input, same file.
pub(super) fn mapping_rows(inventory: &Inventory, lineage: &ColumnLineage) -> BTreeSet<Row> {
    let g = &lineage.graph;
    let mut rows: BTreeSet<Row> = BTreeSet::new();
    for target in g.node_indices() {
        if !matches!(g[target], ColumnNode::Dataset { .. }) {
            continue;
        }
        for write in g.edges_directed(target, Direction::Incoming) {
            let step = write.source();
            let ColumnNode::Step { package, flow, .. } = &g[step] else {
                continue;
            };
            let mut found = Vec::new();
            origins(
                lineage,
                step,
                &mut vec![*write.weight()],
                &mut vec![step],
                &mut found,
            );
            let file = inventory
                .packages
                .get(*package)
                .map_or("", |p| p.file.as_str());
            let name = lineage.packages.get(*package).map_or("?", String::as_str);
            for (source, mut hows) in found {
                hows.reverse();
                let labels: Vec<&str> = hows.iter().map(|h| h.label()).collect();
                rows.insert([
                    project_of(file).to_owned(),
                    name.to_owned(),
                    flow.clone(),
                    describe(lineage, target),
                    server(lineage, Some(target)),
                    source.map_or_else(String::new, |s| describe(lineage, s)),
                    server(lineage, source),
                    labels.join(" → "),
                    status(source, &hows).to_owned(),
                ]);
            }
        }
    }
    rows
}

/// CSV for Excel: BOM, `;`, `\r\n` line endings.
pub(super) fn to_csv(rows: &BTreeSet<Row>) -> String {
    let header = HEADER.map(str::to_owned);
    let mut out = String::from("\u{feff}");
    for row in std::iter::once(&header).chain(rows) {
        let cells: Vec<String> = row.iter().map(|c| cell(c)).collect();
        out.push_str(&cells.join(";"));
        out.push_str("\r\n");
    }
    out
}

/// Upstream, from step to step of the package, up to the table or file columns read;
/// `hows` goes from the written column upstream.
fn origins(
    lineage: &ColumnLineage,
    node: NodeIndex,
    hows: &mut Vec<How>,
    path: &mut Vec<NodeIndex>,
    found: &mut Vec<Origin>,
) {
    let g = &lineage.graph;
    let mut incoming = g.edges_directed(node, Direction::Incoming).peekable();
    if incoming.peek().is_none() {
        found.push((None, hows.clone()));
        return;
    }
    for edge in incoming {
        let from = edge.source();
        hows.push(*edge.weight());
        match g[from] {
            ColumnNode::Dataset { .. } => found.push((Some(from), hows.clone())),
            // A loop between steps stops at the second visit.
            ColumnNode::Step { .. } if !path.contains(&from) => {
                path.push(from);
                origins(lineage, from, hows, path, found);
                path.pop();
            }
            ColumnNode::Step { .. } => {}
        }
        hows.pop();
    }
}

fn server(lineage: &ColumnLineage, node: Option<NodeIndex>) -> String {
    match node.map(|i| &lineage.graph[i]) {
        Some(ColumnNode::Dataset {
            server: Some(server),
            ..
        }) => server.clone(),
        _ => String::new(),
    }
}

/// What the row is worth: a `SELECT *` matches columns by name, not by position as
/// SQL Server does.
fn status(source: Option<NodeIndex>, hows: &[How]) -> &'static str {
    let [direct, transformed, by_name, unknown] = STATUSES;
    if source.is_none() {
        unknown
    } else if hows.contains(&How::SelectStar) {
        by_name
    } else if hows.iter().all(|h| matches!(h, How::Read | How::Write)) {
        direct
    } else {
        transformed
    }
}

/// A cell must neither open another row or column, nor run as an Excel formula
/// (leading `=`, `+`, `-`, `@`: an apostrophe makes it text).
fn cell(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if text.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lineage::columns::column_lineage;

    fn mapping(dir: &str) -> String {
        let (inventory, _) = crate::inventory(std::path::Path::new(dir));
        mapping_csv(&inventory, &column_lineage(&inventory))
    }

    #[test]
    fn one_row_per_written_column_and_column_it_comes_from() {
        let csv = mapping("tests/fixtures/columns_sql");

        let mut lines = csv.strip_prefix('\u{feff}').expect("BOM for Excel").lines();
        assert_eq!(
            lines.next(),
            Some(
                "Project;Package;Task or Data Flow;Target column;Target server;\
                 Source column;Source server;Transformations;Status"
            )
        );
        let rows: Vec<&str> = lines.collect();
        for row in [
            ";Chargement;DFT Staging;Entrepot : stg.Clients.email;srv-demo;\
             Entrepot : ext.Clients.email;srv-demo;read → write;direct",
            ";Chargement;SQL Finaliser;Entrepot : dbo.Clients.email;srv-demo;\
             Entrepot : stg.Clients.email;srv-demo;SELECT * → write;by name (SELECT *)",
            ";Chargement;SQL Resumer;Entrepot : dbo.Resume.courriel;srv-demo;\
             Entrepot : stg.Clients.email;srv-demo;SQL expression → write;transformed",
        ] {
            assert!(rows.contains(&row), "{row}\n---\n{csv}");
        }
        // Sorted: same input, same file, from one analysis to the next.
        let cells: Vec<Vec<&str>> = rows.iter().map(|r| r.split(';').collect()).collect();
        let mut sorted = cells.clone();
        sorted.sort();
        assert_eq!(cells, sorted);
    }

    #[test]
    fn columns_without_known_origin_get_their_own_row() {
        let csv = mapping("tests/fixtures/columns");

        assert!(
            csv.lines()
                .any(|l| l.ends_with(";unknown origin") && l.contains(";;;")),
            "{csv}"
        );
    }

    #[test]
    fn never_writes_connection_strings_or_passwords() {
        let csv = mapping("tests/fixtures/lineage");

        assert!(csv.lines().count() > 1, "{csv}");
        let lower = csv.to_lowercase();
        assert!(
            !lower.contains("password") && !lower.contains("pwd="),
            "{csv}"
        );
    }

    #[test]
    fn cells_cannot_break_the_row_or_run_as_a_formula() {
        assert_eq!(cell("a;b"), "\"a;b\"");
        assert_eq!(cell("said \"x\""), "\"said \"\"x\"\"\"");
        assert_eq!(cell("line\nnext"), "\"line\nnext\"");
        assert_eq!(cell("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(cell("@x"), "'@x");
        assert_eq!(cell("dbo.Clients"), "dbo.Clients");
    }
}
