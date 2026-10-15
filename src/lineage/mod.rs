//! Lignage : graphe jeux de données -> composants / tâches SQL -> jeux de données.

pub mod sql;

use std::collections::{HashMap, HashSet};

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::{Dfs, Reversed, Walker};
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::inventory::{Inventory, SqlDefinition};
use crate::model::{
    Component, ConnectionManager, ConnectionRef, Executable, Package, Role, SqlOrigin, SqlStatement,
};
use sql::{Defined, TableName, Tables};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatasetKind {
    Table,
    File,
    Entity,
    /// Procédure stockée appelée ; ses tables viennent de sa définition (`.sql`).
    Procedure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// Table, fichier ou entité Dynamics, partagé entre les packages.
    Dataset {
        kind: DatasetKind,
        label: String,
        /// Serveur d'une table ou d'une procédure, lu dans la chaîne de connexion.
        server: Option<String>,
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
    /// Création de la table (`CREATE TABLE`) : aucune donnée ne passe par ce lien.
    pub creates: bool,
}

/// Ce qui n'a pas pu être suivi (SQL non analysé, requête en variable…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Index dans `Lineage::packages` : deux packages peuvent porter le même nom.
    pub package: usize,
    /// Tâche ou composant.
    pub step: String,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct Lineage {
    pub graph: DiGraph<Node, Edge>,
    /// Noms des packages, indexés comme `Node::*::package`.
    pub packages: Vec<String>,
    pub notes: Vec<Note>,
}

pub fn lineage(inventory: &Inventory) -> Lineage {
    let mut builder = Builder {
        scripts: &inventory.sql_scripts,
        ..Builder::default()
    };
    for (index, package) in inventory.packages.iter().enumerate() {
        builder.lineage.packages.push(package.name.clone());
        for e in &package.executables {
            if !e.components.is_empty() {
                builder.data_flow(index, package, e);
            }
            if let Some(sql) = &e.sql {
                builder.sql_task(index, package, e, sql);
            }
        }
    }
    builder.expand_definitions();
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
        keep.extend(Dfs::new(g, start).iter(g));
        keep.extend(Dfs::new(Reversed(g), start).iter(Reversed(g)));
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
    /// `CREATE TABLE` : la tâche prépare la table sans y écrire de données.
    Create,
}

/// Jeu de données, identifié par son genre, son serveur et son libellé sans casse.
struct DatasetRef {
    kind: DatasetKind,
    label: String,
    server: Option<String>,
}

/// Base visée par une connexion : serveur et base de sa chaîne (valeur de conception),
/// à défaut le nom de la connexion. Deux connexions vers la même base donnent donc
/// les mêmes nœuds, et deux bases homonymes sur deux serveurs des nœuds distincts.
struct Database {
    server: Option<String>,
    name: String,
}

fn database_of(package: &Package, refs: &[ConnectionRef]) -> Option<Database> {
    let name = refs.first()?.resolved.as_deref()?;
    let manager = package.connections.iter().find(|m| m.name == name);
    let setting = |keys: &[&str]| {
        manager
            .and_then(|m| m.setting(keys))
            .filter(|v| !v.is_empty())
    };
    Some(Database {
        server: setting(&["data source", "server"]).map(str::to_owned),
        name: setting(&["initial catalog", "database"])
            .unwrap_or(name)
            .to_owned(),
    })
}

#[derive(Default)]
struct Builder<'a> {
    lineage: Lineage,
    datasets: HashMap<(DatasetKind, String, String), NodeIndex>,
    scripts: &'a [SqlDefinition],
}

impl<'a> Builder<'a> {
    fn dataset(&mut self, d: DatasetRef) -> NodeIndex {
        let graph = &mut self.lineage.graph;
        let server = d.server.as_deref().unwrap_or_default().to_lowercase();
        let key = (d.kind, server, d.label.to_lowercase());
        *self.datasets.entry(key).or_insert_with(|| {
            graph.add_node(Node::Dataset {
                kind: d.kind,
                label: d.label,
                server: d.server,
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
            Direction::Write | Direction::Create => (step, dataset),
        };
        let edge = Edge {
            from_expression,
            creates: direction == Direction::Create,
        };
        self.lineage.graph.update_edge(from, to, edge);
    }

    fn data_flow(&mut self, package_index: usize, package: &Package, flow: &Executable) {
        let flow_path = flow.path();
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
                    self.attach(package_index, &c.name, node, found, from_expression);
                }
                Err(note) => self.note(package_index, &c.name, &note),
            }
        }
    }

    fn sql_task(&mut self, index: usize, package: &Package, task: &Executable, sql: &SqlStatement) {
        match sql.origin {
            SqlOrigin::Variable => {
                let note = format!("requête dans la variable {}, non suivie", sql.text);
                return self.note(index, &task.name, &note);
            }
            SqlOrigin::File => {
                return self.note(index, &task.name, "requête dans un fichier, non suivie")
            }
            SqlOrigin::Direct | SqlOrigin::Expression => {}
        }
        let Ok(found) = sql::tables(&sql.text) else {
            return self.note(index, &task.name, "SQL non analysé");
        };
        let database = database_of(package, &task.connections);
        let found = datasets(&found, database.as_ref());
        if found.is_empty() {
            return;
        }
        let node = self.lineage.graph.add_node(Node::SqlTask {
            package: index,
            name: task.name.clone(),
        });
        let from_expression = sql.origin == SqlOrigin::Expression;
        self.attach(index, &task.name, node, found, from_expression);
    }

    /// Relie l'étape à ses jeux de données ; une procédure sans définition unique
    /// est signalée.
    fn attach(
        &mut self,
        package: usize,
        step: &str,
        node: NodeIndex,
        found: Vec<(DatasetRef, Direction)>,
        from_expression: bool,
    ) {
        for (d, direction) in found {
            if d.kind == DatasetKind::Procedure {
                let name = d
                    .label
                    .split_once(" : ")
                    .map_or(d.label.as_str(), |(_, n)| n);
                let defined = self.definitions(&d.label, Defined::Procedure);
                let note = match defined.as_slice() {
                    [] => Some(format!("procédure stockée {name}, non suivie")),
                    [_] => None,
                    several => {
                        let files: Vec<&str> = several.iter().map(|s| s.file.as_str()).collect();
                        Some(format!(
                            "procédure stockée {name} définie dans plusieurs scripts ({}), non suivie",
                            files.join(", ")
                        ))
                    }
                };
                if let Some(note) = note {
                    self.note(package, step, &note);
                }
            }
            self.link(d, node, direction, from_expression);
        }
    }

    /// Définitions (`.sql`) de la procédure ou de la vue d'un nœud, retrouvées par
    /// son libellé. La base est ignorée : les scripts la précisent rarement.
    fn definitions(&self, label: &str, defined: Defined) -> Vec<&'a SqlDefinition> {
        let Some(name) = label
            .split_once(" : ")
            .and_then(|(_, name)| sql::object_name(name))
        else {
            return Vec::new();
        };
        self.scripts
            .iter()
            .filter(|d| d.defined == defined && sql::same(&d.name, &name))
            .collect()
    }

    /// Tables des procédures et des vues définies dans un script `.sql`, et
    /// procédures qu'elles appellent : ajoutées en fin de graphe, elles sont lues par
    /// la même boucle. Une définition en double n'est pas suivie (on ne devine pas).
    // ponytail: une procédure appelée seulement par une procédure n'a pas de package :
    // sans définition unique, elle reste un nœud sans note.
    fn expand_definitions(&mut self) {
        let mut next = 0;
        while next < self.lineage.graph.node_count() {
            let node = NodeIndex::new(next);
            next += 1;
            let (defined, label, server) = match &self.lineage.graph[node] {
                Node::Dataset {
                    kind: DatasetKind::Procedure,
                    label,
                    server,
                } => (Defined::Procedure, label, server),
                Node::Dataset {
                    kind: DatasetKind::Table,
                    label,
                    server,
                } => (Defined::View, label, server),
                _ => continue,
            };
            let [defined] = self.definitions(label, defined)[..] else {
                continue;
            };
            // Le corps vit dans la base et sur le serveur de la procédure ou de la vue.
            let database = label.split_once(" : ").map(|(db, _)| Database {
                server: server.clone(),
                name: db.to_owned(),
            });
            for (d, direction) in datasets(&defined.tables, database.as_ref()) {
                self.link(d, node, direction, false);
            }
        }
    }

    fn note(&mut self, package: usize, step: &str, reason: &str) {
        self.lineage.notes.push(Note {
            package,
            step: step.to_owned(),
            reason: reason.to_owned(),
        });
    }
}

/// Tables créées, lues, écrites puis procédures appelées. Créations d'abord : une
/// table créée puis remplie garde son lien d'écriture.
fn datasets(found: &Tables, database: Option<&Database>) -> Vec<(DatasetRef, Direction)> {
    let tables = [
        (&found.creates, Direction::Create),
        (&found.reads, Direction::Read),
        (&found.writes, Direction::Write),
    ];
    let tables = tables
        .into_iter()
        .flat_map(|(list, direction)| list.iter().map(move |t| (table(t, database), direction)));
    let procedures = found
        .procedures
        .iter()
        .map(|p| (procedure(p, database), Direction::Write));
    tables.chain(procedures).collect()
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
    table_or_query(c, database_of(package, &c.connections).as_ref(), direction)
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

/// `AccessMode` du composant, 0 (table) s'il est absent.
pub fn access_mode(c: &Component) -> u32 {
    c.properties
        .get("AccessMode")
        .and_then(|a| a.parse().ok())
        .unwrap_or(0)
}

/// Requête réellement exécutée par un composant : `SqlCommand` en mode commande
/// SQL (`AccessMode` 2), et toujours pour Lookup et OLE DB Command. Une commande
/// restée d'un ancien mode d'accès n'est pas retenue.
pub fn sql_command(c: &Component) -> Option<&str> {
    let always_sql = matches!(
        c.class_id.as_str(),
        "Microsoft.Lookup" | "Microsoft.OLEDBCommand"
    );
    c.properties
        .get("SqlCommand")
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .filter(|_| access_mode(c) == 2 || always_sql)
}

/// OLE DB, ADO.NET, Lookup, OLE DB Command… : requête (`SqlCommand`) ou table (`OpenRowset`).
fn table_or_query(
    c: &Component,
    database: Option<&Database>,
    direction: Direction,
) -> Result<Vec<(DatasetRef, Direction)>, String> {
    let class = c.class_id.as_str();
    let props = &c.properties;
    let access = access_mode(c);
    if let Some(command) = sql_command(c) {
        let found = sql::tables(command).map_err(|_| "SQL non analysé".to_owned())?;
        // Destination en mode commande SQL : le SELECT désigne la table écrite.
        let reads = found.reads.iter().map(|t| (table(t, database), direction));
        let writes = found
            .writes
            .iter()
            .map(|t| (table(t, database), Direction::Write));
        let procedures = found
            .procedures
            .iter()
            .map(|p| (procedure(p, database), Direction::Write));
        return Ok(reads.chain(writes).chain(procedures).collect());
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
        .map(|t| (table(&t, database), direction))
        .into_iter()
        .collect())
}

/// Table : la base vient du nom en trois parties, sinon de la connexion ; le serveur,
/// de la connexion.
fn table(t: &TableName, database: Option<&Database>) -> DatasetRef {
    let name = t
        .database
        .as_deref()
        .or(database.map(|d| d.name.as_str()))
        .unwrap_or("?");
    let schema = t.schema.as_deref().unwrap_or("dbo");
    DatasetRef {
        kind: DatasetKind::Table,
        label: format!("{name} : {schema}.{}", t.table),
        server: database.and_then(|d| d.server.clone()),
    }
}

fn procedure(p: &TableName, database: Option<&Database>) -> DatasetRef {
    DatasetRef {
        kind: DatasetKind::Procedure,
        ..table(p, database)
    }
}

fn file(manager: &ConnectionManager) -> DatasetRef {
    let label = manager
        .target
        .clone()
        .unwrap_or_else(|| manager.name.clone());
    DatasetRef {
        kind: DatasetKind::File,
        label,
        server: None,
    }
}

fn entity(crm: &str, name: &str) -> DatasetRef {
    DatasetRef {
        kind: DatasetKind::Entity,
        label: format!("{crm} : {name}"),
        server: None,
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
