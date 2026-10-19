//! Ordre d'exécution déduit des données : un package passe après ceux qui écrivent les
//! tables, fichiers ou entités qu'il lit (directement, par une vue ou une procédure).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use petgraph::algo::{condensation, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction::{Incoming, Outgoing};

use super::{DatasetKind, Lineage, Node};

/// Au-delà, une table écrite par autant de packages est un journal ou une table de
/// paramètres : elle relierait tout à tout, elle est écartée.
pub const HUB_WRITERS: usize = 5;

#[derive(Debug, Default)]
pub struct ExecutionOrder {
    /// Vagues, la première sans dépendance ; chaque élément est un package, ou plusieurs
    /// qui dépendent les uns des autres (cycle). Indices de `Lineage::packages`.
    pub waves: Vec<Vec<Vec<usize>>>,
    /// Pour chaque package, ceux qui doivent passer avant lui et par quelles tables.
    pub after: BTreeMap<usize, BTreeMap<usize, BTreeSet<String>>>,
    /// Packages sans aucune dépendance de données, ni avant ni après.
    pub independent: Vec<usize>,
    /// Tables écartées et nombre de packages qui les écrivent.
    pub hubs: Vec<(String, usize)>,
    /// Lues mais écrites par aucun package : sources externes, ou SQL non suivi.
    pub sources: Vec<String>,
}

pub fn execution_order(lineage: &Lineage) -> ExecutionOrder {
    let g = &lineage.graph;
    let is_dataset = |i: NodeIndex| matches!(g[i], Node::Dataset { .. });
    // Packages qui écrivent un jeu de données (une création de table n'écrit rien).
    let writers = |d: NodeIndex| -> BTreeSet<usize> {
        g.edges_directed(d, Incoming)
            .filter(|e| !e.weight().creates)
            .filter_map(|e| g[e.source()].package())
            .collect()
    };
    let hubs: HashMap<NodeIndex, usize> = g
        .node_indices()
        .filter(|&d| is_dataset(d))
        .map(|d| (d, writers(d).len()))
        .filter(|&(_, n)| n >= HUB_WRITERS)
        .collect();

    let mut after: BTreeMap<usize, BTreeMap<usize, BTreeSet<String>>> = BTreeMap::new();
    for step in g.node_indices() {
        let Some(reader) = g[step].package() else {
            continue;
        };
        // Ce que l'étape lit, et ce que lisent les procédures qu'elle appelle.
        let mut stack: Vec<NodeIndex> = g
            .neighbors_directed(step, Incoming)
            .filter(|&d| is_dataset(d))
            .collect();
        for called in g
            .neighbors_directed(step, Outgoing)
            .filter(|&d| is_procedure(&g[d]))
        {
            stack.extend(
                g.neighbors_directed(called, Incoming)
                    .filter(|&d| is_dataset(d)),
            );
        }
        // On remonte de jeu de données en jeu de données (vues, procédures) jusqu'aux
        // étapes qui les écrivent.
        let mut seen = HashSet::new();
        while let Some(d) = stack.pop() {
            if !seen.insert(d) || hubs.contains_key(&d) {
                continue;
            }
            for e in g
                .edges_directed(d, Incoming)
                .filter(|e| !e.weight().creates)
            {
                match g[e.source()].package() {
                    Some(writer) if writer != reader => {
                        let tables = after.entry(reader).or_default().entry(writer).or_default();
                        tables.insert(g[d].label().to_owned());
                    }
                    Some(_) => {}
                    None => stack.push(e.source()),
                }
            }
        }
    }

    // Un nœud par package relié ; les cycles fondus en un nœud, puis rangés par vagues.
    let mut packages: DiGraph<usize, ()> = DiGraph::new();
    let mut index: BTreeMap<usize, NodeIndex> = BTreeMap::new();
    let mut node = |graph: &mut DiGraph<usize, ()>, p: usize| {
        *index.entry(p).or_insert_with(|| graph.add_node(p))
    };
    for (&reader, before) in &after {
        let to = node(&mut packages, reader);
        for &writer in before.keys() {
            let from = node(&mut packages, writer);
            packages.add_edge(from, to, ());
        }
    }
    let linked: HashSet<usize> = packages.node_weights().copied().collect();
    let condensed = condensation(packages, true);
    let mut wave_of: HashMap<NodeIndex, usize> = HashMap::new();
    // `condensation` rend un graphe sans cycle : le tri topologique réussit toujours.
    for group in toposort(&condensed, None).unwrap_or_default() {
        let wave = condensed
            .neighbors_directed(group, Incoming)
            .filter_map(|before| wave_of.get(&before))
            .map(|w| w + 1)
            .max()
            .unwrap_or(0);
        wave_of.insert(group, wave);
    }
    let by_name = |p: &usize| lineage.packages.get(*p).cloned().unwrap_or_default();
    let mut waves: Vec<Vec<Vec<usize>>> =
        vec![Vec::new(); wave_of.values().max().map_or(0, |w| w + 1)];
    for (group, wave) in wave_of {
        let mut members = condensed[group].clone();
        members.sort_by_cached_key(by_name);
        waves[wave].push(members);
    }
    for wave in &mut waves {
        wave.sort_by_cached_key(|group| group.first().map(by_name));
    }
    let mut independent: Vec<usize> = (0..lineage.packages.len())
        .filter(|p| !linked.contains(p))
        .collect();
    independent.sort_by_cached_key(by_name);

    let mut hubs: Vec<(String, usize)> = hubs.into_iter().map(|(d, n)| (named(&g[d]), n)).collect();
    hubs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    // Lu (par un package, une vue ou une procédure) sans que rien ne l'écrive.
    let mut sources: Vec<String> = g
        .node_indices()
        .filter(|&d| is_dataset(d) && !is_procedure(&g[d]))
        .filter(|&d| g.edges_directed(d, Incoming).all(|e| e.weight().creates))
        .filter(|&d| g.neighbors_directed(d, Outgoing).next().is_some())
        .map(|d| named(&g[d]))
        .collect();
    sources.sort();
    sources.dedup();
    ExecutionOrder {
        waves,
        after,
        independent,
        hubs,
        sources,
    }
}

fn is_procedure(node: &Node) -> bool {
    matches!(
        node,
        Node::Dataset {
            kind: DatasetKind::Procedure,
            ..
        }
    )
}

/// `Base : schéma.table (serveur)` : deux bases homonymes sur deux serveurs restent distinctes.
fn named(node: &Node) -> String {
    match node {
        Node::Dataset {
            label,
            server: Some(server),
            ..
        } => format!("{label} ({server})"),
        other => other.label().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lineage::lineage;

    fn order() -> (ExecutionOrder, Vec<String>) {
        let (inventory, _) = crate::inventory(std::path::Path::new("tests/fixtures/ordre"));
        let lineage = lineage(&inventory);
        (execution_order(&lineage), lineage.packages)
    }

    #[test]
    fn a_package_runs_after_those_writing_what_it_reads() {
        let (order, names) = order();
        let waves: Vec<Vec<Vec<&str>>> = order
            .waves
            .iter()
            .map(|wave| {
                wave.iter()
                    .map(|group| group.iter().map(|&p| names[p].as_str()).collect())
                    .collect()
            })
            .collect();

        assert_eq!(
            waves,
            [
                vec![vec!["Aller", "Retour"], vec!["Extraire"]],
                vec![vec!["Charger"]],
                vec![vec!["Exporter"]],
            ]
        );
        let charger = names.iter().position(|n| n == "Charger").expect("Charger");
        let extraire = names
            .iter()
            .position(|n| n == "Extraire")
            .expect("Extraire");
        assert_eq!(
            order.after[&charger][&extraire],
            BTreeSet::from(["Entrepot : stg.Clients".to_owned()])
        );
    }

    #[test]
    fn a_table_written_by_many_packages_does_not_tie_them_together() {
        let (order, names) = order();

        assert_eq!(
            order.hubs,
            [("Entrepot : dbo.Journal (srv-demo)".to_owned(), 5)]
        );
        // Surveiller ne lit que le journal : il ne dépend de personne.
        let independent: Vec<&str> = order
            .independent
            .iter()
            .map(|&p| names[p].as_str())
            .collect();
        assert_eq!(independent, ["Isole", "Surveiller"]);
        // Chaque package est placé une fois : dans une vague ou parmi les indépendants.
        let placed: usize = order.waves.iter().flatten().map(Vec::len).sum();
        assert_eq!(placed + order.independent.len(), names.len());
    }

    #[test]
    fn tables_read_but_written_by_no_package_are_sources() {
        let (order, _) = order();

        assert_eq!(order.sources, ["Entrepot : ext.Clients (srv-demo)"]);
    }
}
