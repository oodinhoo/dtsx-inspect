//! Lignage par colonne dans les Data Flows : colonne de table ou de fichier lue,
//! colonnes calculées par les composants, colonne de table ou de fichier écrite.
//! Une colonne de table est un seul nœud pour tous les packages (même serveur,
//! même base) : le lignage passe ainsi d'un package à l'autre.

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};

use super::projection::{projection, Projection};
use super::{component_datasets, database_of, sql_command, table, Database, DatasetRef, Direction};
use crate::inventory::Inventory;
use crate::model::{Executable, Package, Role};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnNode {
    /// Colonne d'une table ou d'un fichier ; `dataset` est son libellé dans le lignage.
    Dataset {
        dataset: String,
        server: Option<String>,
        column: String,
    },
    /// Colonne produite par un composant de Data Flow.
    Step {
        package: usize,
        flow: String,
        component: String,
        column: String,
    },
}

/// Comment une colonne en donne une autre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Read,
    Write,
    Expression,
    Conversion,
    Union,
    Lookup,
    /// Calculée dans la requête de la source (fonction, opération…).
    SqlExpression,
}

impl How {
    pub fn label(self) -> &'static str {
        match self {
            How::Read => "lecture",
            How::Write => "écriture",
            How::Expression => "expression",
            How::Conversion => "conversion",
            How::Union => "Unir tout",
            How::Lookup => "recherche",
            How::SqlExpression => "calcul SQL",
        }
    }
}

#[derive(Debug, Default)]
pub struct ColumnLineage {
    pub graph: DiGraph<ColumnNode, How>,
    /// Noms des packages, indexés comme `ColumnNode::Step::package`.
    pub packages: Vec<String>,
}

pub fn column_lineage(inventory: &Inventory) -> ColumnLineage {
    let mut builder = Builder::default();
    for (index, package) in inventory.packages.iter().enumerate() {
        builder.lineage.packages.push(package.name.clone());
        for flow in package
            .executables
            .iter()
            .filter(|e| !e.components.is_empty())
        {
            builder.flow(index, package, flow);
        }
    }
    builder.lineage
}

#[derive(Default)]
struct Builder {
    lineage: ColumnLineage,
    /// (serveur, jeu de données, colonne) sans casse -> nœud.
    datasets: HashMap<(String, String, String), NodeIndex>,
}

impl Builder {
    fn flow(&mut self, index: usize, package: &Package, flow: &Executable) {
        // Toutes les sorties d'abord : une colonne peut citer celle d'un composant
        // décrit plus loin dans le XML.
        let mut steps = HashMap::new();
        for c in &flow.components {
            for out in &c.outputs {
                let node = self.lineage.graph.add_node(ColumnNode::Step {
                    package: index,
                    flow: flow.path().to_owned(),
                    component: c.name.clone(),
                    column: out.name.clone(),
                });
                steps.insert(out.lineage.as_str(), node);
            }
        }
        for c in &flow.components {
            let datasets = component_datasets(c, package).unwrap_or_default();
            // Requête d'une source ou d'une recherche : sa liste mène aux colonnes de table.
            let query = sql_command(c)
                .and_then(projection)
                .map(|p| (p, database_of(package, &c.connections)));
            let read = single(&datasets, Direction::Read);
            let written = single(&datasets, Direction::Write);
            let computed = match c.class_id.as_str() {
                "Microsoft.DataConvert" => How::Conversion,
                _ => How::Expression,
            };
            for out in &c.outputs {
                let Some(&node) = steps.get(out.lineage.as_str()) else {
                    continue;
                };
                for source in &out.sources {
                    if let Some(&from) = steps.get(source.as_str()) {
                        self.edge(from, node, computed);
                    }
                }
                // Source : colonne de table ou de fichier lue ; recherche : colonne
                // copiée de la table de référence.
                let origin = match c.role {
                    Role::Source => out.external.as_deref().map(|col| (col, How::Read)),
                    _ => out.reference.as_deref().map(|col| (col, How::Lookup)),
                };
                if let Some((column, how)) = origin {
                    self.read(query.as_ref(), read, column, how, node);
                }
            }
            for input in &c.inputs {
                let Some(&from) = steps.get(input.lineage.as_str()) else {
                    continue;
                };
                if let Some(&to) = input.feeds.as_deref().and_then(|f| steps.get(f)) {
                    self.edge(from, to, How::Union);
                }
                if c.role != Role::Destination {
                    continue;
                }
                if let (Some(d), Some(column)) = (written, input.external.as_deref()) {
                    let to = self.dataset(d, column);
                    self.edge(from, to, How::Write);
                }
            }
        }
    }

    /// Colonne de table lue par une source ou copiée par une recherche : par la liste
    /// du SELECT quand le composant exécute une requête, sinon dans son unique table.
    fn read(
        &mut self,
        query: Option<&(Projection, Option<Database>)>,
        table_read: Option<&DatasetRef>,
        column: &str,
        how: How,
        node: NodeIndex,
    ) {
        if let Some((projection, database)) = query {
            if let Some((from, computed)) = projection.origin(column) {
                let how = if computed { How::SqlExpression } else { how };
                for (t, col) in &from {
                    let source = self.dataset(&table(t, database.as_ref()), col);
                    self.edge(source, node, how);
                }
                return;
            }
        }
        if let Some(d) = table_read {
            let source = self.dataset(d, column);
            self.edge(source, node, how);
        }
    }

    fn dataset(&mut self, d: &DatasetRef, column: &str) -> NodeIndex {
        let server = d.server.as_deref().unwrap_or_default().to_lowercase();
        let key = (server, d.label.to_lowercase(), column.to_lowercase());
        let graph = &mut self.lineage.graph;
        *self.datasets.entry(key).or_insert_with(|| {
            graph.add_node(ColumnNode::Dataset {
                dataset: d.label.clone(),
                server: d.server.clone(),
                column: column.to_owned(),
            })
        })
    }

    fn edge(&mut self, from: NodeIndex, to: NodeIndex, how: How) {
        self.lineage.graph.update_edge(from, to, how);
    }
}

/// Le seul jeu de données lu (ou écrit) par le composant ; plusieurs : on ne devine pas.
fn single(datasets: &[(DatasetRef, Direction)], direction: Direction) -> Option<&DatasetRef> {
    match datasets
        .iter()
        .filter(|(_, d)| *d == direction)
        .collect::<Vec<_>>()[..]
    {
        [(d, _)] => Some(d),
        _ => None,
    }
}
