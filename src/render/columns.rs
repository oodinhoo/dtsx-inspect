//! Lignage d'une colonne en texte : d'où elle vient (←) puis où elle va (→),
//! une étape par ligne, avec la façon dont chaque colonne donne la suivante.

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use super::clean;
use crate::lineage::columns::{ColumnLineage, ColumnNode};

/// Colonnes de table ou de fichier dont le nom complet (`Base : schéma.table.colonne`)
/// contient `needle` (sans casse) ; `None` si aucune.
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

/// Une ligne par colonne voisine, puis ses propres voisines, en profondeur.
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
    // Branches dans l'ordre alphabétique : une sortie stable d'une analyse à l'autre.
    let mut edges: Vec<_> = g
        .edges_directed(node, direction)
        .map(|e| (if upstream { e.source() } else { e.target() }, *e.weight()))
        .collect();
    edges.sort_by_cached_key(|(next, _)| describe(lineage, *next));
    for (next, how) in edges {
        // Colonne produite sans colonne amont connue (composant script…).
        let unknown = upstream
            && matches!(g[next], ColumnNode::Step { .. })
            && g.edges_directed(next, Direction::Incoming).next().is_none();
        let suffix = if unknown { ", origine inconnue" } else { "" };
        let line = format!(
            "{}{arrow} {} : {}{suffix}",
            "  ".repeat(path.len()),
            how.label(),
            describe(lineage, next)
        );
        out.push_str(&clean(&line));
        if path.contains(&next) {
            out.push_str(" (boucle)\n");
            continue;
        }
        out.push('\n');
        path.push(next);
        walk(lineage, next, direction, path, out);
        path.pop();
    }
}

/// `Base : dbo.Table.colonne`, ou `Composant.colonne (Package › Data Flow)`.
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
