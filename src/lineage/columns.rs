//! Column lineage: in Data Flows, table or file column read, columns derived by the
//! components, table or file column written; in SQL tasks, the `INSERT … SELECT` from
//! one table to another, in their query or in the procedures they call.
//! A table column is a single node for all packages (same server, same database): the
//! lineage thus goes from one package to another.

use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};

use super::projection::{insert_selects, projection, InsertSelect, Projection};
use super::sql::{same, tables, Defined, TableName};
use super::{
    component_datasets, database_of, defined_in, sql_command, statement, table, Database,
    DatasetRef, Direction,
};
use crate::inventory::{Inventory, SqlDefinition};
use crate::model::{Executable, Package, Role};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnNode {
    /// Column of a table or a file; `dataset` is its label in the lineage.
    Dataset {
        dataset: String,
        server: Option<String>,
        column: String,
    },
    /// Column produced by a Data Flow component.
    Step {
        package: usize,
        flow: String,
        component: String,
        column: String,
    },
}

/// How a column gives another one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Read,
    Write,
    Expression,
    Conversion,
    Union,
    Lookup,
    /// Computed in the source query (function, operation…).
    SqlExpression,
    /// `INSERT … SELECT *`: same-named column in the read table.
    SelectStar,
}

impl How {
    pub fn label(self) -> &'static str {
        match self {
            How::Read => "read",
            How::Write => "write",
            How::Expression => "expression",
            How::Conversion => "conversion",
            How::Union => "Union All",
            How::Lookup => "lookup",
            How::SqlExpression => "SQL expression",
            How::SelectStar => "SELECT *",
        }
    }
}

#[derive(Debug, Default)]
pub struct ColumnLineage {
    pub graph: DiGraph<ColumnNode, How>,
    /// Package names, indexed like `ColumnNode::Step::package`.
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
    // SQL tasks next: a `SELECT *` takes the known columns of the read table, which a
    // Data Flow or another task further in the packages can reveal. Loop again as long
    // as new columns appear.
    let inserts: Vec<_> = inventory
        .packages
        .iter()
        .enumerate()
        .flat_map(|(index, package)| package.executables.iter().map(move |e| (index, package, e)))
        .flat_map(|(index, package, task)| {
            task_inserts(&inventory.sql_scripts, package, task)
                .into_iter()
                .map(move |(insert, database)| (index, task, insert, database))
        })
        .collect();
    loop {
        let before = builder.lineage.graph.node_count();
        for (index, task, insert, database) in &inserts {
            builder.insert(*index, task, insert, database.as_ref());
        }
        if builder.lineage.graph.node_count() == before {
            return builder.lineage;
        }
    }
}

/// `INSERT … SELECT` run by an SQL task, each with its database: those of its query,
/// then those of the procedures it calls, directly or not, when a single `.sql` script
/// defines them (no guessing, as in the table lineage).
// shortcut: procedures called by a Data Flow (OLE DB Command, source `EXEC`) are not
// followed; do it if real packages load columns that way.
fn task_inserts(
    scripts: &[SqlDefinition],
    package: &Package,
    task: &Executable,
) -> Vec<(InsertSelect, Option<Database>)> {
    let Some((text, _)) = task
        .sql
        .as_ref()
        .and_then(|sql| statement(package, task, sql).ok())
    else {
        return Vec::new();
    };
    let database = database_of(package, &task.connections);
    let mut found: Vec<_> = insert_selects(text)
        .into_iter()
        .map(|insert| (insert, database.clone()))
        .collect();
    let called = tables(text).map(|t| t.procedures).unwrap_or_default();
    let mut calls: Vec<_> = called.into_iter().map(|p| (p, database.clone())).collect();
    let mut seen: Vec<TableName> = Vec::new();
    while let Some((procedure, caller)) = calls.pop() {
        // A loop of calls is read once.
        if seen.iter().any(|s| same(s, &procedure)) {
            continue;
        }
        seen.push(procedure.clone());
        let [body] = defined_in(scripts, &procedure, Defined::Procedure)[..] else {
            continue;
        };
        let database = body_database(&procedure, caller.as_ref());
        found.extend(body.inserts.iter().map(|i| (i.clone(), database.clone())));
        let nested = body.tables.procedures.iter();
        calls.extend(nested.map(|p| (p.clone(), database.clone())));
    }
    found
}

/// Database a called procedure runs in: the one of its three-part name, otherwise the
/// caller's, on the caller's server.
fn body_database(procedure: &TableName, caller: Option<&Database>) -> Option<Database> {
    let name = procedure
        .database
        .clone()
        .or_else(|| caller.map(|d| d.name.clone()))?;
    Some(Database {
        server: caller.and_then(|d| d.server.clone()),
        name,
    })
}

#[derive(Default)]
struct Builder {
    lineage: ColumnLineage,
    /// (server, dataset, column) case-insensitive -> node.
    datasets: HashMap<(String, String, String), NodeIndex>,
    /// (package, SQL task path, case-insensitive table and column written) -> node.
    sql_steps: HashMap<(usize, String, String, String), NodeIndex>,
}

impl Builder {
    fn flow(&mut self, index: usize, package: &Package, flow: &Executable) {
        // All outputs first: a column can use one of a component described further
        // in the XML.
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
            // Query of a source or a lookup: its list leads to the table columns.
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
                // Source: table or file column read; lookup: column copied from the
                // reference table.
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

    /// Table column read by a source or copied by a lookup: through the SELECT list
    /// when the component runs a query, otherwise in its single table.
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

    /// `INSERT … SELECT` of an SQL task: each filled column goes through a step named
    /// after the task.
    fn insert(
        &mut self,
        package: usize,
        task: &Executable,
        insert: &InsertSelect,
        database: Option<&Database>,
    ) {
        let target = table(&insert.target, database);
        for (column, sources) in self.filled(insert, database) {
            let step = self.sql_step(package, task, &target, &column);
            for (from, how) in sources {
                self.edge(from, step, how);
            }
            let to = self.dataset(&target, &column);
            self.edge(step, to, How::Write);
        }
    }

    /// Columns filled by an INSERT and the columns read for each. Named column list:
    /// matched by position with the SELECT. Without a list: by the SELECT output name,
    /// and a `SELECT *` takes by name the already known columns of its table (SQL Server
    /// matches by position: same name assumed).
    fn filled(
        &mut self,
        insert: &InsertSelect,
        database: Option<&Database>,
    ) -> Vec<(String, Vec<(NodeIndex, How)>)> {
        let p = &insert.projection;
        let mut filled = Vec::new();
        // Named list and `*`: positions can no longer be counted, no guessing.
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

    /// Already known columns of a table, sorted by name.
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

    /// Column filled by an SQL task: one step per task and per written column. Two
    /// tables filled under the same column name (staging then target) keep two steps,
    /// otherwise each would seem to come from the other.
    fn sql_step(
        &mut self,
        package: usize,
        task: &Executable,
        target: &DatasetRef,
        column: &str,
    ) -> NodeIndex {
        let key = (
            package,
            task.path().to_owned(),
            target.label.to_lowercase(),
            column.to_lowercase(),
        );
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

/// The only dataset read (or written) by the component; several: no guessing.
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
