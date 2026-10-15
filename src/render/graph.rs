//! Données du lignage interactif (blocs, Data Flows, liens), lues par `lineage.js`.

use petgraph::visit::EdgeRef;
use serde::Serialize;

use super::html::project_of;
use crate::inventory::Inventory;
use crate::lineage::{sql_command, DatasetKind, Lineage, Node, Note};
use crate::model::{Component, ConnectionRef, Executable, Package, Role, SqlOrigin};
use crate::parser::secrets::redact_secrets;

#[derive(Serialize)]
struct Graph<'a> {
    packages: &'a [String],
    /// Fichier de chaque package : deux packages peuvent porter le même nom.
    files: Vec<&'a str>,
    /// Projet de chaque package, celui de l'arborescence.
    projects: Vec<&'a str>,
    /// L'indice d'un bloc est son identifiant dans `edges`.
    nodes: Vec<Block>,
    groups: Vec<Group>,
    /// (depuis, vers, requête construite par expression, création de table)
    edges: Vec<(usize, usize, bool, bool)>,
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
}

/// Requête affichée dans le panneau de détails, mots de passe masqués.
#[derive(Serialize)]
struct Query {
    title: &'static str,
    text: String,
}

/// Data Flow : cadre autour de ses composants.
#[derive(Serialize, PartialEq)]
struct Group {
    title: String,
    package: usize,
}

/// JSON à placer dans une balise `<script>` : `<` y est échappé.
pub(super) fn graph_json(inventory: &Inventory, lineage: &Lineage) -> String {
    let g = &lineage.graph;
    let mut groups = Vec::new();
    let mut nodes: Vec<Block> = g
        .node_indices()
        .map(|i| block(&g[i], inventory, &mut groups))
        .collect();
    for note in &lineage.notes {
        // ponytail: étape retrouvée par son nom dans le package ; deux homonymes
        // dans un même package se partagent la remarque.
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
    };
    // Des chaînes et des entiers : la sérialisation ne peut pas échouer.
    serde_json::to_string(&graph)
        .unwrap_or_else(|_| "null".to_owned())
        .replace('<', "\\u003c")
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
                "Fichier".to_owned()
            } else {
                format!("Fichier · {dir}")
            };
            ("file", file, sub, vec![("Chemin", label.to_owned())])
        }
        DatasetKind::Table => {
            let (base, table) = label.split_once(" : ").unwrap_or(("?", label));
            let props = vec![("Base", base.to_owned()), ("Table", table.to_owned())];
            ("table", table, format!("Table · {base}"), props)
        }
        DatasetKind::Entity => {
            let (crm, entity) = label.split_once(" : ").unwrap_or(("?", label));
            let props = vec![("Connexion", crm.to_owned()), ("Entité", entity.to_owned())];
            ("entity", entity, format!("Entité · {crm}"), props)
        }
        DatasetKind::Procedure => {
            let (base, name) = label.split_once(" : ").unwrap_or(("?", label));
            let props = vec![("Base", base.to_owned()), ("Procédure", name.to_owned())];
            ("proc", name, format!("Procédure · {base}"), props)
        }
    };
    // Deux bases homonymes sur deux serveurs : le serveur les distingue.
    let sub = match server {
        Some(server) => {
            props.insert(0, ("Serveur", server.to_owned()));
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
    }
}

fn component(package: usize, group: usize, name: &str, c: Option<&Component>) -> Block {
    let class = c.map_or("", |c| c.class_id.as_str());
    let mut props = vec![("Classe", class.to_owned())];
    let mut query = None;
    if let Some(c) = c {
        push_connections(&mut props, &c.connections);
        let p = &c.properties;
        if let Some(table) = p.get("OpenRowset").or(p.get("TableOrViewName")) {
            props.push(("Table", table.clone()));
        }
        if let Some(entity) = p.get("SourceEntity") {
            props.push(("Entité", entity.clone()));
        }
        if !c.expressions.is_empty() {
            props.push(("Par expression", c.expressions.join(", ")));
        }
        query = sql_command(c)
            .map(|sql| ("Requête SQL", sql))
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
    }
}

fn sql_task(package: usize, name: &str, task: Option<&Executable>) -> Block {
    let mut props = Vec::new();
    let mut query = None;
    let mut expression = false;
    if let Some(e) = task {
        props.push(("Chemin", e.path().to_owned()));
        push_connections(&mut props, &e.connections);
        if let Some(sql) = &e.sql {
            expression = sql.origin == SqlOrigin::Expression;
            let (origin, title) = match sql.origin {
                SqlOrigin::Direct => ("saisie directe", "Requête SQL"),
                SqlOrigin::Expression => ("expression (valeur à la conception)", "Requête SQL"),
                SqlOrigin::Variable => ("variable", "Variable"),
                SqlOrigin::File => ("fichier", "Connexion du fichier"),
            };
            props.push(("Requête", origin.to_owned()));
            query = Some(Query {
                title,
                text: redact_secrets(&sql.text),
            });
        }
    }
    Block {
        kind: "sql",
        title: name.to_owned(),
        sub: "Tâche SQL".to_owned(),
        package: Some(package),
        group: None,
        props,
        query,
        note: None,
        expression,
    }
}

/// Étape sans jeu de données suivi (procédure stockée, SQL non analysé…).
fn untracked(note: &Note, inventory: &Inventory) -> Block {
    let task = inventory
        .packages
        .get(note.package)
        .and_then(|p| task(p, &note.step));
    Block {
        kind: "untracked",
        sub: "Non suivi".to_owned(),
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
    props.push(("Connexion", names.join(", ")));
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::lineage::lineage;

    #[test]
    fn gives_each_package_the_project_of_the_sidebar() {
        // Copie de compilation seule (`Seul/obj/Development`) : le projet est `Seul`,
        // comme dans l'arborescence, et non le premier dossier du chemin.
        let (inventory, _) = crate::inventory(Path::new("tests/fixtures/build_copy"));
        let json = graph_json(&inventory, &lineage(&inventory));

        let graph: serde_json::Value = serde_json::from_str(&json).expect("JSON valide");
        assert_eq!(graph["projects"], serde_json::json!(["Projet", "Seul"]));
    }

    #[test]
    fn exposes_queries_and_keeps_the_json_inside_its_script_tag() {
        let (inventory, _) = crate::inventory(Path::new("tests/fixtures/lineage"));
        let json = graph_json(&inventory, &lineage(&inventory));

        let graph: serde_json::Value = serde_json::from_str(&json).expect("JSON valide");
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
