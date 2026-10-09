//! Lignage : graphe jeux de données -> composants / tâches SQL -> jeux de données.

pub mod sql;

use std::collections::{HashMap, HashSet};

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::{Dfs, Reversed};
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::inventory::Inventory;
use crate::model::{
    Component, ConnectionManager, Executable, Package, Role, SqlOrigin, SqlStatement,
};
use sql::TableName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetKind {
    Table,
    File,
    Entity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// Table, fichier ou entité Dynamics, partagé entre les packages.
    Dataset {
        kind: DatasetKind,
        label: String,
    },
    /// Composant d'un Data Flow (`flow` = chemin lisible du Data Flow).
    Component {
        package: usize,
        flow: String,
        name: String,
    },
    SqlTask {
        package: usize,
        name: String,
    },
}

impl Node {
    pub fn label(&self) -> &str {
        match self {
            Node::Dataset { label, .. } => label,
            Node::Component { name, .. } | Node::SqlTask { name, .. } => name,
        }
    }

    /// Package d'un composant ou d'une tâche ; un jeu de données n'en a pas.
    pub fn package(&self) -> Option<usize> {
        match self {
            Node::Component { package, .. } | Node::SqlTask { package, .. } => Some(*package),
            Node::Dataset { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Edge {
    /// Lien déduit d'une requête construite par expression (valeur à la conception).
    pub from_expression: bool,
}

/// Ce qui n'a pas pu être suivi (SQL non analysé, requête en variable…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Index dans `Lineage::packages` : deux packages peuvent porter le même nom.
    pub package: usize,
    /// `étape : raison`.
    pub text: String,
}

#[derive(Debug, Default)]
pub struct Lineage {
    pub graph: DiGraph<Node, Edge>,
    /// Noms des packages, indexés comme `Node::*::package`.
    pub packages: Vec<String>,
    pub notes: Vec<Note>,
}

pub fn lineage(inventory: &Inventory) -> Lineage {
    let mut builder = Builder::default();
    for (index, package) in inventory.packages.iter().enumerate() {
        builder.lineage.packages.push(package.name.clone());
        for e in &package.executables {
            if !e.components.is_empty() {
                builder.data_flow(index, package, e);
            }
            if let Some(sql) = &e.sql {
                builder.sql_task(index, e, sql);
            }
        }
    }
    builder.lineage
}

/// Ne garde que ce qui alimente ou consomme les jeux de données dont le libellé
/// contient `needle` (sans casse). `None` si aucun ne correspond.
pub fn focus(lineage: &Lineage, needle: &str) -> Option<Lineage> {
    let g = &lineage.graph;
    let needle = needle.to_lowercase();
    let starts: Vec<NodeIndex> = g
        .node_indices()
        .filter(|&i| matches!(g[i], Node::Dataset { .. }))
        .filter(|&i| g[i].label().to_lowercase().contains(&needle))
        .collect();
    if starts.is_empty() {
        return None;
    }
    let mut keep = HashSet::new();
    for &start in &starts {
        let mut downstream = Dfs::new(g, start);
        while let Some(i) = downstream.next(g) {
            keep.insert(i);
        }
        let reversed = Reversed(g);
        let mut upstream = Dfs::new(reversed, start);
        while let Some(i) = upstream.next(reversed) {
            keep.insert(i);
        }
    }
    let graph = g.filter_map(|i, n| keep.contains(&i).then(|| n.clone()), |_, e| Some(*e));
    let shown: HashSet<usize> = graph.node_weights().filter_map(Node::package).collect();
    let notes = lineage
        .notes
        .iter()
        .filter(|note| shown.contains(&note.package))
        .cloned()
        .collect();
    Some(Lineage {
        graph,
        packages: lineage.packages.clone(),
        notes,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Read,
    Write,
}

/// Jeu de données identifié par `key` (sans casse), affiché par `label`.
struct DatasetRef {
    kind: DatasetKind,
    key: String,
    label: String,
}

#[derive(Default)]
struct Builder {
    lineage: Lineage,
    datasets: HashMap<String, NodeIndex>,
}

impl Builder {
    fn dataset(&mut self, d: DatasetRef) -> NodeIndex {
        let graph = &mut self.lineage.graph;
        *self.datasets.entry(d.key).or_insert_with(|| {
            graph.add_node(Node::Dataset {
                kind: d.kind,
                label: d.label,
            })
        })
    }

    fn link(
        &mut self,
        d: DatasetRef,
        step: NodeIndex,
        direction: Direction,
        from_expression: bool,
    ) {
        let dataset = self.dataset(d);
        let (from, to) = match direction {
            Direction::Read => (dataset, step),
            Direction::Write => (step, dataset),
        };
        self.lineage
            .graph
            .update_edge(from, to, Edge { from_expression });
    }

    fn data_flow(&mut self, package_index: usize, package: &Package, flow: &Executable) {
        let flow_path = flow
            .ref_id
            .strip_prefix("Package\\")
            .unwrap_or(&flow.ref_id);
        let mut nodes = HashMap::new();
        for c in &flow.components {
            let node = self.lineage.graph.add_node(Node::Component {
                package: package_index,
                flow: flow_path.to_owned(),
                name: c.name.clone(),
            });
            nodes.insert(c.ref_id.as_str(), node);
        }
        for p in &flow.paths {
            if let (Some(&from), Some(&to)) = (nodes.get(p.from.as_str()), nodes.get(p.to.as_str()))
            {
                self.lineage.graph.update_edge(from, to, Edge::default());
            }
        }
        for c in &flow.components {
            let Some(&node) = nodes.get(c.ref_id.as_str()) else {
                continue;
            };
            match component_datasets(c, package) {
                Ok(found) if found.is_empty() && c.role != Role::Transform => {
                    let role = if c.role == Role::Source {
                        "source"
                    } else {
                        "destination"
                    };
                    let note = format!("{role} sans jeu de données identifié");
                    self.note(package_index, &c.name, &note);
                }
                Ok(found) => {
                    // ponytail: une expression sur une propriété que le mode d'accès
                    // ne lit pas compte aussi ; suivre la propriété lue si ça gêne.
                    let from_expression = !c.expressions.is_empty();
                    for (d, direction) in found {
                        self.link(d, node, direction, from_expression);
                    }
                }
                Err(note) => self.note(package_index, &c.name, &note),
            }
        }
    }

    fn sql_task(&mut self, package: usize, task: &Executable, sql: &SqlStatement) {
        match sql.origin {
            SqlOrigin::Variable => {
                let note = format!("requête dans la variable {}, non suivie", sql.text);
                return self.note(package, &task.name, &note);
            }
            SqlOrigin::File => {
                return self.note(package, &task.name, "requête dans un fichier, non suivie")
            }
            SqlOrigin::Direct | SqlOrigin::Expression => {}
        }
        let Ok(found) = sql::tables(&sql.text) else {
            return self.note(package, &task.name, "SQL non analysé");
        };
        for procedure in &found.procedures {
            let note = format!("procédure stockée {procedure}, non suivie");
            self.note(package, &task.name, &note);
        }
        if found.reads.is_empty() && found.writes.is_empty() {
            return;
        }
        let connection = task.connections.first().and_then(|r| r.resolved.as_deref());
        let node = self.lineage.graph.add_node(Node::SqlTask {
            package,
            name: task.name.clone(),
        });
        let from_expression = sql.origin == SqlOrigin::Expression;
        let reads = found.reads.iter().map(|t| (t, Direction::Read));
        for (t, direction) in reads.chain(found.writes.iter().map(|t| (t, Direction::Write))) {
            self.link(table(t, connection), node, direction, from_expression);
        }
    }

    fn note(&mut self, package: usize, step: &str, note: &str) {
        let text = format!("{step} : {note}");
        self.lineage.notes.push(Note { package, text });
    }
}

/// Jeux de données lus ou écrits par un composant de Data Flow.
fn component_datasets(
    c: &Component,
    package: &Package,
) -> Result<Vec<(DatasetRef, Direction)>, String> {
    let direction = if c.role == Role::Destination {
        Direction::Write
    } else {
        Direction::Read
    };
    let connection = c.connections.first().and_then(|r| r.resolved.as_deref());
    if c.class_id.contains("FlatFile") {
        let manager =
            connection.and_then(|name| package.connections.iter().find(|m| m.name == name));
        return Ok(manager
            .map(file)
            .map(|d| (d, direction))
            .into_iter()
            .collect());
    }
    if c.class_id.contains("DynamicsCrm") {
        return dynamics(c, connection, direction);
    }
    table_or_query(c, connection, direction)
}

/// Source KingswaySoft : entité unique (`SourceType` 0) ou requête FetchXML (1).
fn dynamics(
    c: &Component,
    connection: Option<&str>,
    direction: Direction,
) -> Result<Vec<(DatasetRef, Direction)>, String> {
    if direction == Direction::Write {
        return Err("destination Dynamics non suivie".to_owned());
    }
    let props = &c.properties;
    let entities = match props.get("SourceType").map(String::as_str) {
        Some("1") => props
            .get("FetchXML")
            .map(|x| fetch_entities(x))
            .unwrap_or_default(),
        _ => props
            .get("SourceEntity")
            .filter(|e| !e.is_empty())
            .cloned()
            .into_iter()
            .collect(),
    };
    let crm = connection.unwrap_or("Dynamics");
    Ok(entities
        .into_iter()
        .map(|e| (entity(crm, &e), Direction::Read))
        .collect())
}

/// OLE DB, ADO.NET, Lookup, OLE DB Command… : requête (`SqlCommand`) ou table (`OpenRowset`).
fn table_or_query(
    c: &Component,
    connection: Option<&str>,
    direction: Direction,
) -> Result<Vec<(DatasetRef, Direction)>, String> {
    let class = c.class_id.as_str();
    let props = &c.properties;
    let access: u32 = props
        .get("AccessMode")
        .and_then(|a| a.parse().ok())
        .unwrap_or(0);
    let command = props.get("SqlCommand").filter(|s| !s.trim().is_empty());
    let always_sql = matches!(class, "Microsoft.Lookup" | "Microsoft.OLEDBCommand");
    if let Some(command) = command.filter(|_| access == 2 || always_sql) {
        let found = sql::tables(command).map_err(|_| "SQL non analysé".to_owned())?;
        // Destination en mode commande SQL : le SELECT désigne la table écrite.
        let reads = found
            .reads
            .iter()
            .map(|t| (table(t, connection), direction));
        let writes = found
            .writes
            .iter()
            .map(|t| (table(t, connection), Direction::Write));
        return Ok(reads.chain(writes).collect());
    }
    // AccessMode « …depuis une variable » (OLE DB : source 1 et 3, destination 1 et 4).
    let from_variable = match class {
        "Microsoft.OLEDBSource" => matches!(access, 1 | 3),
        "Microsoft.OLEDBDestination" => matches!(access, 1 | 4),
        _ => false,
    };
    if from_variable {
        return Err("table ou requête dans une variable, non suivie".to_owned());
    }
    let object = props
        .get("OpenRowset")
        .or(props.get("TableOrViewName"))
        .and_then(|name| sql::object_name(name));
    Ok(object
        .map(|t| (table(&t, connection), direction))
        .into_iter()
        .collect())
}

/// Table : la base vient du nom en trois parties, sinon de la connexion.
// ponytail: une même base via deux connexions de noms différents = deux nœuds ;
// passer à serveur/base de la cible si ça gêne.
fn table(t: &TableName, connection: Option<&str>) -> DatasetRef {
    let database = t.database.as_deref().or(connection).unwrap_or("?");
    let schema = t.schema.as_deref().unwrap_or("dbo");
    let label = format!("{database} : {schema}.{}", t.table);
    DatasetRef {
        kind: DatasetKind::Table,
        key: format!("table:{}", label.to_lowercase()),
        label,
    }
}

fn file(manager: &ConnectionManager) -> DatasetRef {
    let label = manager
        .target
        .clone()
        .unwrap_or_else(|| manager.name.clone());
    DatasetRef {
        kind: DatasetKind::File,
        key: format!("file:{}", label.to_lowercase()),
        label,
    }
}

fn entity(crm: &str, name: &str) -> DatasetRef {
    let label = format!("{crm} : {name}");
    DatasetRef {
        kind: DatasetKind::Entity,
        key: format!("entity:{}", label.to_lowercase()),
        label,
    }
}

/// Entités d'une requête FetchXML (`<entity name>` et `<link-entity name>`).
fn fetch_entities(fetch_xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(fetch_xml);
    let mut entities: Vec<String> = Vec::new();
    // ponytail: un FetchXML mal formé donne les entités lues jusqu'à l'erreur.
    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(e) | Event::Empty(e)
                if matches!(e.local_name().as_ref(), "entity" | "link-entity") =>
            {
                let name = crate::parser::attr(&e, "name").ok().flatten();
                if let Some(name) = name.filter(|n| !entities.contains(n)) {
                    entities.push(name);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    entities
}
