//! Lignage par colonne : dans les Data Flows, colonne de table ou de fichier lue,
//! colonnes calculées par les composants, colonne de table ou de fichier écrite ; dans
//! les tâches SQL, les `INSERT … SELECT` d'une table vers une autre.
//! Une colonne de table est un seul nœud pour tous les packages (même serveur,
//! même base) : le lignage passe ainsi d'un package à l'autre.

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};

use super::projection::{insert_selects, projection, InsertSelect, Projection};
use super::{
    component_datasets, database_of, sql_command, statement, table, Database, DatasetRef, Direction,
};
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
    /// `INSERT … SELECT *` : colonne de même nom dans la table lue.
    SelectStar,
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
            How::SelectStar => "SELECT *",
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
    // Tâches SQL ensuite : un `SELECT *` reprend les colonnes connues de la table lue,
    // qu'un Data Flow ou une autre tâche, plus loin dans les packages, peut révéler. On
    // repasse tant que de nouvelles colonnes apparaissent.
    let inserts: Vec<_> = inventory
        .packages
        .iter()
        .enumerate()
        .flat_map(|(index, package)| package.executables.iter().map(move |e| (index, package, e)))
        .filter_map(|(index, package, task)| {
            let (text, _) = statement(package, task, task.sql.as_ref()?).ok()?;
            let inserts = insert_selects(text);
            let database = database_of(package, &task.connections);
            (!inserts.is_empty()).then_some((index, task, inserts, database))
        })
        .collect();
    loop {
        let before = builder.lineage.graph.node_count();
        for (index, task, inserts, database) in &inserts {
            for insert in inserts {
                builder.insert(*index, task, insert, database.as_ref());
            }
        }
        if builder.lineage.graph.node_count() == before {
            return builder.lineage;
        }
    }
}

#[derive(Default)]
struct Builder {
    lineage: ColumnLineage,
    /// (serveur, jeu de données, colonne) sans casse -> nœud.
    datasets: HashMap<(String, String, String), NodeIndex>,
    /// (package, chemin de la tâche SQL, colonne sans casse) -> nœud.
    sql_steps: HashMap<(usize, String, String), NodeIndex>,
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

    /// `INSERT … SELECT` d'une tâche SQL : chaque colonne remplie passe par une étape au
    /// nom de la tâche.
    fn insert(
        &mut self,
        package: usize,
        task: &Executable,
        insert: &InsertSelect,
        database: Option<&Database>,
    ) {
        let target = table(&insert.target, database);
        for (column, sources) in self.filled(insert, database) {
            let step = self.sql_step(package, task, &column);
            for (from, how) in sources {
                self.edge(from, step, how);
            }
            let to = self.dataset(&target, &column);
            self.edge(step, to, How::Write);
        }
    }

    /// Colonnes remplies par un INSERT et les colonnes lues pour chacune. Liste de
    /// colonnes nommée : appariées par position avec le SELECT. Sans liste : par le nom
    /// en sortie du SELECT, et un `SELECT *` reprend par leur nom les colonnes déjà
    /// connues de sa table (SQL Server apparie par position : même nom supposé).
    fn filled(
        &mut self,
        insert: &InsertSelect,
        database: Option<&Database>,
    ) -> Vec<(String, Vec<(NodeIndex, How)>)> {
        let p = &insert.projection;
        let mut filled = Vec::new();
        // Liste nommée et `*` : les positions ne se comptent plus, on ne devine pas.
        if !insert.columns.is_empty() && !p.stars.is_empty() {
            return filled;
        }
        for (i, c) in p.columns.iter().enumerate() {
            let name = insert.columns.get(i).unwrap_or(&c.name);
            if name.is_empty() {
                continue;
            }
            let how = if c.computed {
                How::SqlExpression
            } else {
                How::Read
            };
            let sources = c
                .from
                .iter()
                .map(|(t, col)| (self.dataset(&table(t, database), col), how))
                .collect();
            filled.push((name.clone(), sources));
        }
        for star in &p.stars {
            for (column, node) in self.known_columns(&table(star, database)) {
                filled.push((column, vec![(node, How::SelectStar)]));
            }
        }
        filled
    }

    /// Colonnes déjà connues d'une table, triées par nom.
    fn known_columns(&self, d: &DatasetRef) -> Vec<(String, NodeIndex)> {
        let server = d.server.as_deref().unwrap_or_default().to_lowercase();
        let label = d.label.to_lowercase();
        let mut found: Vec<(String, NodeIndex)> = self
            .datasets
            .iter()
            .filter(|((s, l, _), _)| *s == server && *l == label)
            .filter_map(|(_, &node)| match &self.lineage.graph[node] {
                ColumnNode::Dataset { column, .. } => Some((column.clone(), node)),
                ColumnNode::Step { .. } => None,
            })
            .collect();
        found.sort();
        found
    }

    /// Colonne remplie par une tâche SQL : une étape par tâche et par colonne.
    fn sql_step(&mut self, package: usize, task: &Executable, column: &str) -> NodeIndex {
        let key = (package, task.path().to_owned(), column.to_lowercase());
        let graph = &mut self.lineage.graph;
        *self.sql_steps.entry(key).or_insert_with(|| {
            graph.add_node(ColumnNode::Step {
                package,
                flow: task.path().to_owned(),
                component: task.name.clone(),
                column: column.to_owned(),
            })
        })
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
