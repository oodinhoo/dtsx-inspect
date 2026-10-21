//! Lineage of a column as text: where it comes from (←) then where it goes (→), one
//! step per line, with how each column gives the next one.

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use super::clean;
use crate::lineage::columns::{ColumnLineage, ColumnNode};

/// Table or file columns whose full name (`Database : schema.table.column`) contains
/// `needle` (case-insensitive); `None` if none.
pub fn column_trace(lineage: &ColumnLineage, needle: &str) -> Option<String> {
    let g = &lineage.graph;
    let needle = needle.to_lowercase();
    let starts: Vec<NodeIndex> = g
        .node_indices()
        .filter(|&i| matches!(g[i], ColumnNode::Dataset { .. }))
        .filter(|&i| describe(lineage, i).to_lowercase().contains(&needle))
        .collect();
    if starts.is_empty() {
        return None;
    }
    let mut out = String::new();
    for start in starts {
        let server = match &g[start] {
            ColumnNode::Dataset {
                server: Some(server),
                ..
            } => format!(" ({server})"),
            _ => String::new(),
        };
        out.push_str(&clean(&format!("{}{server}", describe(lineage, start))));
        out.push('\n');
        for direction in [Direction::Incoming, Direction::Outgoing] {
            walk(lineage, start, direction, &mut vec![start], &mut out);
        }
    }
    Some(out)
}

/// One line per neighbour column, then its own neighbours, depth first.
fn walk(
    lineage: &ColumnLineage,
    node: NodeIndex,
    direction: Direction,
    path: &mut Vec<NodeIndex>,
    out: &mut String,
) {
    let g = &lineage.graph;
    let upstream = direction == Direction::Incoming;
    let arrow = if upstream { "←" } else { "→" };
    // Branches in alphabetical order: an output stable from one analysis to the next.
    let mut edges: Vec<_> = g
        .edges_directed(node, direction)
        .map(|e| (if upstream { e.source() } else { e.target() }, *e.weight()))
        .collect();
    edges.sort_by_cached_key(|(next, _)| describe(lineage, *next));
    for (next, how) in edges {
        // Column produced without a known upstream column (script component…).
        let unknown = upstream
            && matches!(g[next], ColumnNode::Step { .. })
            && g.edges_directed(next, Direction::Incoming).next().is_none();
        let suffix = if unknown { ", unknown origin" } else { "" };
        let line = format!(
            "{}{arrow} {}: {}{suffix}",
            "  ".repeat(path.len()),
            how.label(),
            describe(lineage, next)
        );
        out.push_str(&clean(&line));
        if path.contains(&next) {
            out.push_str(" (loop)\n");
            continue;
        }
        out.push('\n');
        path.push(next);
        walk(lineage, next, direction, path, out);
        path.pop();
    }
}

/// `Database : dbo.Table.column`, or `Component.column (Package › Data Flow)`.
pub(super) fn describe(lineage: &ColumnLineage, i: NodeIndex) -> String {
    match &lineage.graph[i] {
        ColumnNode::Dataset {
            dataset, column, ..
        } => format!("{dataset}.{column}"),
        ColumnNode::Step {
            package,
            flow,
            component,
            column,
        } => {
            let package = lineage.packages.get(*package).map_or("?", String::as_str);
            format!("{component}.{column} ({package} › {flow})")
        }
    }
}
