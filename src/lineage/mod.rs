//! Lineage: datasets -> components / SQL tasks -> datasets graph.

pub mod columns;
pub mod order;
pub mod projection;
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
    /// Called stored procedure; its tables come from its definition (`.sql`).
    Procedure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// Table, file or Dynamics entity, shared across packages.
    Dataset {
        kind: DatasetKind,
        label: String,
        /// Server of a table or a procedure, read from the connection string.
        server: Option<String>,
    },
    /// Data Flow component (`flow` = readable path of the Data Flow).
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

    /// Package of a component or a task; a dataset has none.
    pub fn package(&self) -> Option<usize> {
        match self {
            Node::Component { package, .. } | Node::SqlTask { package, .. } => Some(*package),
            Node::Dataset { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Edge {
    /// Link inferred from a query built by an expression (design-time value).
    pub from_expression: bool,
    /// Table creation (`CREATE TABLE`): no data goes through this link.
    pub creates: bool,
}

/// What could not be traced (SQL not analyzed, query in a variable…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Index in `Lineage::packages`: two packages can have the same name.
    pub package: usize,
    /// Task or component.
    pub step: String,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct Lineage {
    pub graph: DiGraph<Node, Edge>,
    /// Package names, indexed like `Node::*::package`.
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

/// Keeps only what feeds or consumes the datasets whose label contains `needle`
/// (case-insensitive). `None` if none matches.
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
    /// `CREATE TABLE`: the task prepares the table without writing data to it.
    Create,
}

/// Dataset, identified by its kind, its server and its case-insensitive label.
struct DatasetRef {
    kind: DatasetKind,
    label: String,
    server: Option<String>,
}

/// Database targeted by a connection: server and database of its string (design-time
/// value), falling back to the connection name. Two connections to the same database
/// thus give the same nodes, and two same-named databases on two servers distinct ones.
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
                    let note = format!("{role} without an identified dataset");
                    self.note(package_index, &c.name, &note);
                }
                Ok(found) => {
                    // shortcut: an expression on a property the access mode does not
                    // read counts too; follow the property actually read if it matters.
                    let from_expression = !c.expressions.is_empty();
                    self.attach(package_index, &c.name, node, found, from_expression);
                }
                Err(note) => self.note(package_index, &c.name, &note),
            }
        }
    }

    fn sql_task(&mut self, index: usize, package: &Package, task: &Executable, sql: &SqlStatement) {
        let (text, from_expression) = match statement(package, task, sql) {
            Ok(found) => found,
            Err(note) => return self.note(index, &task.name, &note),
        };
        let Ok(found) = sql::tables(text) else {
            return self.note(index, &task.name, "SQL not analyzed");
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
        self.attach(index, &task.name, node, found, from_expression);
    }

    /// Links the step to its datasets; a procedure without a single definition is
    /// reported.
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
                    [] => Some(format!("stored procedure {name}, not traced")),
                    [_] => None,
                    several => {
                        let files: Vec<&str> = several.iter().map(|s| s.file.as_str()).collect();
                        Some(format!(
                            "stored procedure {name} defined in several scripts ({}), not traced",
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

    /// Definitions (`.sql`) of the procedure or view of a node, found by its label.
    /// The database is ignored: scripts rarely specify it.
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

    /// Tables of the procedures and views defined in a `.sql` script, and procedures
    /// they call: added at the end of the graph, they are read by the same loop. A
    /// duplicate definition is not traced (no guessing).
    // shortcut: a procedure called only by a procedure has no package: without a
    // single definition, it stays a node without a note.
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
            // The body lives in the database and on the server of the procedure or view.
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

/// Query run by an SQL task, and whether it is computed by an expression; `Err`: why
/// it is not traced. Read from a variable: its value; computed by an expression, the
/// design-time value, as for a query in an expression.
pub(super) fn statement<'a>(
    package: &'a Package,
    task: &Executable,
    sql: &'a SqlStatement,
) -> Result<(&'a str, bool), String> {
    match sql.origin {
        SqlOrigin::Direct => Ok((sql.text.as_str(), false)),
        SqlOrigin::Expression => Ok((sql.text.as_str(), true)),
        SqlOrigin::Variable => variable(package, task, &sql.text)
            .ok_or_else(|| format!("query in variable {}, not traced", sql.text)),
        SqlOrigin::File => Err("query in a file, not traced".to_owned()),
    }
}

/// Value of the variable `name` seen from the task (the closest wins, as in SSIS),
/// and whether it is computed by an expression. Masked value: nothing.
fn variable<'p>(package: &'p Package, task: &Executable, name: &str) -> Option<(&'p str, bool)> {
    let path = task.path();
    let visible =
        |scope: &str| scope.is_empty() || path == scope || path.starts_with(&format!("{scope}\\"));
    let v = package
        .variables
        .iter()
        .filter(|v| v.name.eq_ignore_ascii_case(name.trim()) && visible(&v.scope))
        .max_by_key(|v| v.scope.len())?;
    let value = v.value.as_deref().filter(|value| *value != "***")?;
    Some((value, v.expression.is_some()))
}

/// Tables created, read, written, then called procedures. Creations first: a table
/// created then filled keeps its write link.
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

/// Datasets read or written by a Data Flow component.
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

/// KingswaySoft source: single entity (`SourceType` 0) or FetchXML query (1).
fn dynamics(
    c: &Component,
    connection: Option<&str>,
    direction: Direction,
) -> Result<Vec<(DatasetRef, Direction)>, String> {
    if direction == Direction::Write {
        return Err("Dynamics destination not traced".to_owned());
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

/// `AccessMode` of the component, 0 (table) when absent.
pub fn access_mode(c: &Component) -> u32 {
    c.properties
        .get("AccessMode")
        .and_then(|a| a.parse().ok())
        .unwrap_or(0)
}

/// Query actually run by a component: `SqlCommand` in SQL command mode
/// (`AccessMode` 2), and always for Lookup and OLE DB Command. A command left over
/// from a former access mode is not kept.
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

/// OLE DB, ADO.NET, Lookup, OLE DB Command…: query (`SqlCommand`) or table (`OpenRowset`).
fn table_or_query(
    c: &Component,
    database: Option<&Database>,
    direction: Direction,
) -> Result<Vec<(DatasetRef, Direction)>, String> {
    let class = c.class_id.as_str();
    let props = &c.properties;
    let access = access_mode(c);
    if let Some(command) = sql_command(c) {
        let found = sql::tables(command).map_err(|_| "SQL not analyzed".to_owned())?;
        // Destination in SQL command mode: the SELECT designates the written table.
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
    // AccessMode "…from a variable" (OLE DB: source 1 and 3, destination 1 and 4).
    let from_variable = match class {
        "Microsoft.OLEDBSource" => matches!(access, 1 | 3),
        "Microsoft.OLEDBDestination" => matches!(access, 1 | 4),
        _ => false,
    };
    if from_variable {
        return Err("table or query in a variable, not traced".to_owned());
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

/// Table: the database comes from the three-part name, otherwise from the connection;
/// the server, from the connection.
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

/// Entities of a FetchXML query (`<entity name>` and `<link-entity name>`).
fn fetch_entities(fetch_xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(fetch_xml);
    let mut entities: Vec<String> = Vec::new();
    // shortcut: a malformed FetchXML gives the entities read up to the error.
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
