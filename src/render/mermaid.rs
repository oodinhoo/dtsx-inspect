use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;

use super::clean;
use crate::calls::{Call, Callee};
use crate::inventory::Inventory;
use crate::lineage::{DatasetKind, Lineage, Node};

/// Appels de packages : un nœud par package concerné, la tâche sur la flèche, les
/// packages absents du dossier en pointillés.
pub fn calls_mermaid(inventory: &Inventory, calls: &[Call]) -> String {
    let package = |i: usize| {
        let name = inventory.packages.get(i).map_or("?", |p| p.name.as_str());
        (format!("p{i}"), format!("[\"{}\"]", escape(name)))
    };
    let mut nodes: Vec<String> = Vec::new();
    let mut missing: Vec<&str> = Vec::new();
    let mut edges = Vec::new();
    for call in calls {
        let from = package(call.caller);
        let to = match &call.callee {
            Callee::Package(i) => package(*i),
            Callee::Missing(name) => {
                let n = match missing.iter().position(|m| m == name) {
                    Some(n) => n,
                    None => {
                        missing.push(name);
                        missing.len() - 1
                    }
                };
                let label = escape(&format!("{name} (introuvable)"));
                (format!("m{n}"), format!("[\"{label}\"]:::missing"))
            }
        };
        for (key, shape) in [&from, &to] {
            let declared = format!("  {key}{shape}");
            if !nodes.contains(&declared) {
                nodes.push(declared);
            }
        }
        edges.push(format!(
            "  {} -->|\"{}\"| {}",
            from.0,
            escape(&call.task),
            to.0
        ));
    }
    let mut lines = vec!["flowchart LR".to_owned()];
    lines.extend(nodes);
    lines.extend(edges);
    if !missing.is_empty() {
        lines.push("  classDef missing stroke-dasharray:4 3".to_owned());
    }
    lines.join("\n") + "\n"
}

/// `flowchart LR` : jeux de données à part (partagés entre packages), un
/// sous-graphe par package et un par Data Flow.
pub fn mermaid(lineage: &Lineage) -> String {
    let g = &lineage.graph;
    let mut lines = vec![
        "flowchart LR".to_owned(),
        "  %% Pointillés : requête construite par expression (valeur à la conception).".to_owned(),
    ];
    for i in g.node_indices() {
        if matches!(g[i], Node::Dataset { .. }) {
            lines.push(format!("  {}", node(i, &g[i])));
        }
    }
    for (p, name) in lineage.packages.iter().enumerate() {
        lines.extend(package(lineage, p, name));
    }
    for e in g.edge_references() {
        let arrow = match e.weight() {
            w if w.creates => "-. crée .->",
            w if w.from_expression => "-.->",
            _ => "-->",
        };
        lines.push(format!(
            "  n{} {arrow} n{}",
            e.source().index(),
            e.target().index()
        ));
    }
    for note in &lineage.notes {
        let package = lineage
            .packages
            .get(note.package)
            .map_or("?", String::as_str);
        let note = format!("{package} : {} : {}", note.step, note.reason);
        lines.push(format!("  %% Non suivi : {}", clean(&note)));
    }
    lines.join("\n") + "\n"
}

fn package(lineage: &Lineage, p: usize, name: &str) -> Vec<String> {
    let g = &lineage.graph;
    let members: Vec<NodeIndex> = g
        .node_indices()
        .filter(|&i| g[i].package() == Some(p))
        .collect();
    if members.is_empty() {
        return Vec::new();
    }
    let mut flows: Vec<&str> = Vec::new();
    for &i in &members {
        if let Node::Component { flow, .. } = &g[i] {
            if !flows.contains(&flow.as_str()) {
                flows.push(flow);
            }
        }
    }
    let mut lines = vec![format!("  subgraph p{p}[\"{}\"]", escape(name))];
    for (f, flow) in flows.iter().enumerate() {
        lines.push(format!("    subgraph p{p}f{f}[\"{}\"]", escape(flow)));
        for &i in &members {
            if matches!(&g[i], Node::Component { flow: of, .. } if of == flow) {
                lines.push(format!("      {}", node(i, &g[i])));
            }
        }
        lines.push("    end".to_owned());
    }
    for &i in &members {
        if matches!(g[i], Node::SqlTask { .. }) {
            lines.push(format!("    {}", node(i, &g[i])));
        }
    }
    lines.push("  end".to_owned());
    lines
}

fn node(i: NodeIndex, n: &Node) -> String {
    let label = match n.label().trim() {
        "" => "(sans nom)".to_owned(),
        text => escape(text),
    };
    let id = i.index();
    match n {
        Node::Dataset {
            kind: DatasetKind::Table,
            ..
        } => format!("n{id}[(\"{label}\")]"),
        Node::Dataset {
            kind: DatasetKind::File,
            ..
        } => format!("n{id}[/\"{label}\"/]"),
        Node::Dataset {
            kind: DatasetKind::Entity,
            ..
        } => format!("n{id}[[\"{label}\"]]"),
        Node::Dataset {
            kind: DatasetKind::Procedure,
            ..
        } => format!("n{id}>\"{label}\"]"),
        Node::Component { .. } => format!("n{id}[\"{label}\"]"),
        Node::SqlTask { .. } => format!("n{id}{{{{\"{label}\"}}}}"),
    }
}

/// Libellé entre guillemets : `#` d'abord, il sert aux codes d'entité Mermaid.
fn escape(text: &str) -> String {
    clean(text)
        .replace('#', "#35;")
        .replace('"', "#quot;")
        .replace('<', "#lt;")
        .replace('>', "#gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unnamed_node_still_gets_a_label() {
        let unnamed = Node::Component {
            package: 0,
            flow: "DFT".into(),
            name: String::new(),
        };
        assert_eq!(node(NodeIndex::new(3), &unnamed), "n3[\"(sans nom)\"]");
    }

    #[test]
    fn labels_cannot_break_out_of_their_quotes() {
        assert_eq!(escape("a\"b<c>#1\nd"), "a#quot;b#lt;c#gt;#35;1 d");
    }
}
