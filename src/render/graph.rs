//! Data of the interactive lineage (blocks, Data Flows, links), read by `lineage.js`.

use std::collections::HashMap;

use petgraph::visit::EdgeRef;
use petgraph::Direction;
use serde::Serialize;

use super::columns::describe;
use super::html::project_of;
use crate::inventory::Inventory;
use crate::lineage::columns::{ColumnLineage, ColumnNode, How};
use crate::lineage::{sql_command, DatasetKind, Lineage, Node, Note};
use crate::model::{Component, ConnectionRef, Executable, Package, Role, SqlOrigin};
use crate::parser::secrets::redact_secrets;

#[derive(Serialize)]
struct Graph<'a> {
    packages: &'a [String],
    /// File of each package: two packages can have the same name.
    files: Vec<&'a str>,
    /// Project of each package, the one of the sidebar.
    projects: Vec<&'a str>,
    /// The index of a block is its identifier in `edges`.
    nodes: Vec<Block>,
    groups: Vec<Group>,
    /// (from, to, query built by an expression, table creation)
    edges: Vec<(usize, usize, bool, bool)>,
    /// Column lineage: (column, full description, produced by a component); the index
    /// is the identifier of the column in `column_edges`.
    columns: Vec<(String, String, bool)>,
    /// (from, to, how)
    column_edges: Vec<(usize, usize, &'static str)>,
    /// Per table or file column: (package, `W` writes, `R` reads, `C` computes another
    /// column); empty for a component column.
    uses: Vec<Vec<(usize, &'static str)>>,
}

#[derive(Serialize)]
struct Block {
    kind: &'static str,
    title: String,
    sub: String,
    package: Option<usize>,
    group: Option<usize>,
    props: Vec<(&'static str, String)>,
    query: Option<Query>,
    note: Option<String>,
    expression: bool,
    /// Columns of the table or file, or produced by the component.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    columns: Vec<usize>,
}

/// Query shown in the details panel, passwords masked.
#[derive(Serialize)]
struct Query {
    title: &'static str,
    text: String,
}

/// Data Flow: frame around its components.
#[derive(Serialize, PartialEq)]
struct Group {
    title: String,
    package: usize,
}

/// JSON to place in a `<script>` tag: `<` is escaped in it.
pub(super) fn graph_json(
    inventory: &Inventory,
    lineage: &Lineage,
    columns: &ColumnLineage,
) -> String {
    let g = &lineage.graph;
    let mut groups = Vec::new();
    let mut nodes: Vec<Block> = g
        .node_indices()
        .map(|i| block(&g[i], inventory, &mut groups))
        .collect();
    attach_columns(inventory, lineage, columns, &mut nodes);
    for note in &lineage.notes {
        // shortcut: step found by its name in the package; two same-named steps in one
        // package share the note.
        let step = g.node_indices().find(|&i| {
            !matches!(g[i], Node::Dataset { .. })
                && g[i].package() == Some(note.package)
                && g[i].label() == note.step
        });
        match step {
            Some(i) => {
                let block = &mut nodes[i.index()];
                block.note = Some(match block.note.take() {
                    Some(previous) => format!("{previous}\n{}", note.reason),
                    None => note.reason.clone(),
                });
            }
            None => nodes.push(untracked(note, inventory)),
        }
    }
    let edges = g
        .edge_references()
        .map(|e| {
            let w = e.weight();
            let (from, to) = (e.source().index(), e.target().index());
            (from, to, w.from_expression, w.creates)
        })
        .collect();
    let graph = Graph {
        packages: &lineage.packages,
        files: inventory.packages.iter().map(|p| p.file.as_str()).collect(),
        projects: inventory
            .packages
            .iter()
            .map(|p| project_of(&p.file))
            .collect(),
        nodes,
        groups,
        edges,
        columns: columns
            .graph
            .node_indices()
            .map(|i| {
                let step = matches!(columns.graph[i], ColumnNode::Step { .. });
                (
                    column_name(&columns.graph[i]).to_owned(),
                    describe(columns, i),
                    step,
                )
            })
            .collect(),
        column_edges: columns
            .graph
            .edge_references()
            .map(|e| (e.source().index(), e.target().index(), e.weight().label()))
            .collect(),
        uses: column_uses(columns),
    };
    // Strings and integers: serialization cannot fail.
    serde_json::to_string(&graph)
        .unwrap_or_else(|_| "null".to_owned())
        .replace('<', "\\u003c")
}

/// Columns of each block: those of a table or file (same server, same label), those
/// produced by a component (same package, Data Flow and name) or filled by an SQL task.
fn attach_columns(
    inventory: &Inventory,
    lineage: &Lineage,
    columns: &ColumnLineage,
    blocks: &mut [Block],
) {
    let mut of_dataset: HashMap<(String, String), Vec<usize>> = HashMap::new();
    let mut of_component: HashMap<(usize, &str, &str), Vec<usize>> = HashMap::new();
    for i in columns.graph.node_indices() {
        match &columns.graph[i] {
            ColumnNode::Dataset {
                dataset, server, ..
            } => of_dataset
                .entry(dataset_key(dataset, server.as_deref()))
                .or_default()
                .push(i.index()),
            ColumnNode::Step {
                package,
                flow,
                component,
                ..
            } => of_component
                .entry((*package, flow, component))
                .or_default()
                .push(i.index()),
        }
    }
    for i in lineage.graph.node_indices() {
        let found = match &lineage.graph[i] {
            Node::Dataset { label, server, .. } => {
                of_dataset.get(&dataset_key(label, server.as_deref()))
            }
            Node::Component {
                package,
                flow,
                name,
            } => of_component.get(&(*package, flow.as_str(), name.as_str())),
            // SQL task: its columns carry the task path in place of a Data Flow.
            Node::SqlTask { package, name } => inventory
                .packages
                .get(*package)
                .and_then(|p| task(p, name))
                .and_then(|t| of_component.get(&(*package, t.path(), name.as_str()))),
        };
        if let (Some(found), Some(block)) = (found, blocks.get_mut(i.index())) {
            block.columns = found.clone();
        }
    }
}

/// Packages writing, reading or computing from each table or file column, according to
/// the component columns touching it.
fn column_uses(columns: &ColumnLineage) -> Vec<Vec<(usize, &'static str)>> {
    let g = &columns.graph;
    g.node_indices()
        .map(|i| {
            if !matches!(g[i], ColumnNode::Dataset { .. }) {
                return Vec::new();
            }
            let incoming = g
                .edges_directed(i, Direction::Incoming)
                .map(|e| (e.source(), *e.weight()));
            let outgoing = g
                .edges_directed(i, Direction::Outgoing)
                .map(|e| (e.target(), *e.weight()));
            let mut uses: Vec<(usize, &'static str)> = incoming
                .chain(outgoing)
                .filter_map(|(other, how)| match g[other] {
                    ColumnNode::Step { package, .. } => Some((package, mark(how))),
                    ColumnNode::Dataset { .. } => None,
                })
                .collect();
            uses.sort_unstable();
            uses.dedup();
            uses
        })
        .collect()
}

/// Role of a package for a table column, according to the link joining them.
fn mark(how: How) -> &'static str {
    match how {
        How::Write => "W",
        How::SqlExpression => "C",
        _ => "R",
    }
}

fn dataset_key(label: &str, server: Option<&str>) -> (String, String) {
    (
        server.unwrap_or_default().to_lowercase(),
        label.to_lowercase(),
    )
}

fn column_name(node: &ColumnNode) -> &str {
    match node {
        ColumnNode::Dataset { column, .. } | ColumnNode::Step { column, .. } => column,
    }
}

fn block(node: &Node, inventory: &Inventory, groups: &mut Vec<Group>) -> Block {
    match node {
        Node::Dataset {
            kind,
            label,
            server,
        } => dataset(*kind, label, server.as_deref()),
        Node::Component {
            package,
            flow,
            name,
        } => {
            let group = Group {
                title: flow.clone(),
                package: *package,
            };
            let index = groups.iter().position(|g| *g == group).unwrap_or_else(|| {
                groups.push(group);
                groups.len() - 1
            });
            let found = inventory
                .packages
                .get(*package)
                .and_then(|p| p.executables.iter().find(|e| e.path() == flow))
                .and_then(|e| e.components.iter().find(|c| &c.name == name));
            component(*package, index, name, found)
        }
        Node::SqlTask { package, name } => {
            let found = inventory.packages.get(*package).and_then(|p| task(p, name));
            sql_task(*package, name, found)
        }
    }
}

fn dataset(kind: DatasetKind, label: &str, server: Option<&str>) -> Block {
    let (kind, title, sub, mut props) = match kind {
        DatasetKind::File => {
            let (dir, file) = label.rsplit_once(['\\', '/']).unwrap_or(("", label));
            let sub = if dir.is_empty() {
                "File".to_owned()
            } else {
                format!("File · {dir}")
            };
            ("file", file, sub, vec![("Path", label.to_owned())])
        }
        DatasetKind::Table => {
            let (base, table) = label.split_once(" : ").unwrap_or(("?", label));
            let props = vec![("Database", base.to_owned()), ("Table", table.to_owned())];
            ("table", table, format!("Table · {base}"), props)
        }
        DatasetKind::Entity => {
            let (crm, entity) = label.split_once(" : ").unwrap_or(("?", label));
            let props = vec![
                ("Connection", crm.to_owned()),
                ("Entity", entity.to_owned()),
            ];
            ("entity", entity, format!("Entity · {crm}"), props)
        }
        DatasetKind::Procedure => {
            let (base, name) = label.split_once(" : ").unwrap_or(("?", label));
            let props = vec![
                ("Database", base.to_owned()),
                ("Procedure", name.to_owned()),
            ];
            ("proc", name, format!("Procedure · {base}"), props)
        }
    };
    // Two same-named databases on two servers: the server tells them apart.
    let sub = match server {
        Some(server) => {
            props.insert(0, ("Server", server.to_owned()));
            format!("{sub} · {server}")
        }
        None => sub,
    };
    Block {
        kind,
        title: title.to_owned(),
        sub,
        package: None,
        group: None,
        props,
        query: None,
        note: None,
        expression: false,
        columns: Vec::new(),
    }
}

fn component(package: usize, group: usize, name: &str, c: Option<&Component>) -> Block {
    let class = c.map_or("", |c| c.class_id.as_str());
    let mut props = vec![("Class", class.to_owned())];
    let mut query = None;
    if let Some(c) = c {
        push_connections(&mut props, &c.connections);
        let p = &c.properties;
        if let Some(table) = p.get("OpenRowset").or(p.get("TableOrViewName")) {
            props.push(("Table", table.clone()));
        }
        if let Some(entity) = p.get("SourceEntity") {
            props.push(("Entity", entity.clone()));
        }
        if !c.expressions.is_empty() {
            props.push(("By expression", c.expressions.join(", ")));
        }
        query = sql_command(c)
            .map(|sql| ("SQL query", sql))
            .or_else(|| p.get("FetchXML").map(|x| ("FetchXML", x.as_str())))
            .filter(|(_, text)| !text.trim().is_empty())
            .map(|(title, text)| Query {
                title,
                text: redact_secrets(text),
            });
    }
    props.retain(|(_, value)| !value.is_empty());
    Block {
        kind: match c.map(|c| c.role) {
            Some(Role::Source) => "source",
            Some(Role::Destination) => "dest",
            _ => "transform",
        },
        title: name.to_owned(),
        sub: class.rsplit(['.', ',']).next().unwrap_or(class).to_owned(),
        package: Some(package),
        group: Some(group),
        props,
        query,
        note: None,
        expression: c.is_some_and(|c| !c.expressions.is_empty()),
        columns: Vec::new(),
    }
}

fn sql_task(package: usize, name: &str, task: Option<&Executable>) -> Block {
    let mut props = Vec::new();
    let mut query = None;
    let mut expression = false;
    if let Some(e) = task {
        props.push(("Path", e.path().to_owned()));
        push_connections(&mut props, &e.connections);
        if let Some(sql) = &e.sql {
            expression = sql.origin == SqlOrigin::Expression;
            let (origin, title) = match sql.origin {
                SqlOrigin::Direct => ("typed in directly", "SQL query"),
                SqlOrigin::Expression => ("expression (design-time value)", "SQL query"),
                SqlOrigin::Variable => ("variable", "Variable"),
                SqlOrigin::File => ("file", "File connection"),
            };
            props.push(("Query", origin.to_owned()));
            query = Some(Query {
                title,
                text: redact_secrets(&sql.text),
            });
        }
    }
    Block {
        kind: "sql",
        title: name.to_owned(),
        sub: "SQL task".to_owned(),
        package: Some(package),
        group: None,
        props,
        query,
        note: None,
        expression,
        columns: Vec::new(),
    }
}

/// Step without a traced dataset (stored procedure, SQL not analyzed…).
fn untracked(note: &Note, inventory: &Inventory) -> Block {
    let task = inventory
        .packages
        .get(note.package)
        .and_then(|p| task(p, &note.step));
    Block {
        kind: "untracked",
        sub: "Not traced".to_owned(),
        note: Some(note.reason.clone()),
        ..sql_task(note.package, &note.step, task)
    }
}

fn task<'a>(package: &'a Package, name: &str) -> Option<&'a Executable> {
    package
        .executables
        .iter()
        .find(|e| e.name == name && e.sql.is_some())
}

fn push_connections(props: &mut Vec<(&'static str, String)>, refs: &[ConnectionRef]) {
    let names: Vec<&str> = refs
        .iter()
        .map(|r| r.resolved.as_deref().unwrap_or(&r.reference))
        .collect();
    props.push(("Connection", names.join(", ")));
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::lineage::columns::column_lineage;
    use crate::lineage::lineage;

    #[test]
    fn blocks_list_their_columns_and_the_column_graph_is_shipped() {
        let (inventory, _) = crate::inventory(Path::new("tests/fixtures/columns"));
        let json = graph_json(
            &inventory,
            &lineage(&inventory),
            &column_lineage(&inventory),
        );

        let graph: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let columns_of = |title: &str| {
            let block = graph["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|n| n["title"] == title)
                .cloned()
                .unwrap_or_default();
            let mut names: Vec<String> = block["columns"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|i| graph["columns"][i.as_u64()? as usize][0].as_str())
                .map(str::to_owned)
                .collect();
            names.sort();
            names
        };
        // Table: its columns, read or written by the packages.
        assert_eq!(columns_of("dbo.Clients"), ["email", "id", "note", "pays"]);
        // Component: the columns it produces.
        assert_eq!(columns_of("DER Email"), ["email_norm"]);
        let edges = graph["column_edges"].as_array().expect("column links");
        assert!(edges.iter().any(|e| e[2] == "expression"), "{edges:?}");
    }

    #[test]
    fn sql_tasks_list_the_columns_their_insert_fills() {
        let (inventory, _) = crate::inventory(Path::new("tests/fixtures/columns_sql"));
        let json = graph_json(
            &inventory,
            &lineage(&inventory),
            &column_lineage(&inventory),
        );

        let graph: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let block = graph["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|n| n["title"] == "SQL Finaliser")
            .cloned()
            .unwrap_or_default();
        let mut names: Vec<&str> = block["columns"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| graph["columns"][i.as_u64()? as usize][0].as_str())
            .collect();
        names.sort();
        assert_eq!(names, ["email", "id"]);
    }

    #[test]
    fn says_which_packages_write_or_read_each_table_column() {
        let (inventory, _) = crate::inventory(Path::new("tests/fixtures/columns"));
        let json = graph_json(
            &inventory,
            &lineage(&inventory),
            &column_lineage(&inventory),
        );

        let graph: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let uses_of = |full: &str| -> Vec<String> {
            let columns = graph["columns"].as_array().expect("columns");
            let i = columns.iter().position(|c| c[1] == full).expect(full);
            graph["uses"][i]
                .as_array()
                .into_iter()
                .flatten()
                .map(|u| {
                    let package = &graph["packages"][u[0].as_u64().unwrap_or_default() as usize];
                    format!(
                        "{}:{}",
                        package.as_str().unwrap_or("?"),
                        u[1].as_str().unwrap_or("?")
                    )
                })
                .collect()
        };
        assert_eq!(
            uses_of("Entrepot : dbo.Clients.email"),
            ["Clients:W", "Export:R", "Synthese:R"]
        );
        // Lookup: the reference table is read; computation in a query: `C`.
        assert_eq!(
            uses_of("Entrepot : ref.Pays.libelle"),
            ["Clients:R", "Synthese:C"]
        );
        // Component column: its package shows on its block.
        assert!(uses_of("DER Email.email_norm (Clients › DFT Clients)").is_empty());
    }

    #[test]
    fn gives_each_package_the_project_of_the_sidebar() {
        // Build copy only (`Standalone/obj/Development`): the project is `Standalone`, as in the
        // sidebar, and not the first folder of the path.
        let (inventory, _) = crate::inventory(Path::new("tests/fixtures/build_copy"));
        let json = graph_json(
            &inventory,
            &lineage(&inventory),
            &column_lineage(&inventory),
        );

        let graph: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            graph["projects"],
            serde_json::json!(["Project", "Standalone"])
        );
    }

    #[test]
    fn exposes_queries_and_keeps_the_json_inside_its_script_tag() {
        let (inventory, _) = crate::inventory(Path::new("tests/fixtures/lineage"));
        let json = graph_json(
            &inventory,
            &lineage(&inventory),
            &column_lineage(&inventory),
        );

        let graph: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let titled = |title: &str| {
            graph["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|n| n["title"] == title)
                .cloned()
                .unwrap_or_default()
        };
        assert!(titled("SQL Alimenter Clients")["query"]["text"]
            .as_str()
            .is_some_and(|sql| sql.contains("INSERT INTO dbo.Clients")));
        assert_eq!(titled("SQL Procedure")["kind"], "sql");
        assert_eq!(titled("dbo.ChargerClients")["kind"], "proc");
        assert_eq!(titled("SQL Import brut")["kind"], "untracked");
        assert!(!json.contains('<'), "{json}");
    }
}
