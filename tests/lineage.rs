use std::path::PathBuf;

use dtsx_inspect::lineage::{focus, lineage, Lineage, Node};
use dtsx_inspect::{inventory, render};

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

fn build() -> Lineage {
    let (inv, errors) = inventory(&fixture("lineage"));
    assert!(errors.is_empty(), "{errors:?}");
    lineage(&inv)
}

/// Liens lisibles, triés : `source -> cible`, `-.->` pour une requête en expression.
fn edges(l: &Lineage) -> Vec<String> {
    let mut edges: Vec<String> = l
        .graph
        .edge_indices()
        .filter_map(|e| {
            let (from, to) = l.graph.edge_endpoints(e)?;
            let arrow = if l.graph[e].from_expression {
                "-.->"
            } else {
                "->"
            };
            Some(format!(
                "{} {arrow} {}",
                l.graph[from].label(),
                l.graph[to].label()
            ))
        })
        .collect();
    edges.sort();
    edges
}

#[test]
fn follows_data_from_files_and_entities_to_tables_across_packages() {
    let expected = [
        "ADO Clients -> FFD Export",
        "CRM : account -> CRM Comptes",
        "CRM : contact -> CRM Comptes",
        "CRM : lead -> CRM Prospects",
        "CRM Comptes -> UNI Fusion",
        "CRM Prospects -> UNI Fusion",
        "D:\\demo\\clients.csv -> FF Clients",
        "DST Comptes -> Entrepot : stg.Comptes",
        "DST Staging -> Entrepot : stg.Clients",
        "Entrepot : dbo.Clients -.-> SQL Historiser",
        "Entrepot : dbo.Clients -> ADO Clients",
        "Entrepot : ref.Pays -> LKP Pays",
        "Entrepot : ref.Pays -> SQL Alimenter Clients",
        "Entrepot : stg.Clients -> SQL Alimenter Clients",
        "FF Clients -> LKP Pays",
        "FFD Export -> D:\\demo\\export.csv",
        "LKP Pays -> DST Staging",
        "SQL Alimenter Clients -> Entrepot : dbo.Clients",
        "SQL Historiser -.-> Entrepot : dbo.Historique",
        "UNI Fusion -> DST Comptes",
    ];

    assert_eq!(edges(&build()), expected);
}

#[test]
fn a_table_shared_by_two_packages_is_a_single_node() {
    let l = build();

    let clients = l
        .graph
        .node_weights()
        .filter(|n| matches!(n, Node::Dataset { .. }) && n.label() == "Entrepot : dbo.Clients")
        .count();
    assert_eq!(clients, 1);
    assert_eq!(l.packages, vec!["Chargement", "Export"]);
}

#[test]
fn tasks_without_data_movement_are_left_out() {
    let l = build();

    let tasks: Vec<_> = l
        .graph
        .node_weights()
        .filter(|n| matches!(n, Node::SqlTask { .. }))
        .map(Node::label)
        .collect();
    assert_eq!(tasks, vec!["SQL Alimenter Clients", "SQL Historiser"]);
}

#[test]
fn reports_what_could_not_be_followed() {
    assert_eq!(
        build().notes,
        vec![
            "Chargement : SQL Import brut : SQL non analysé",
            "Chargement : SQL Depuis variable : requête dans la variable User::Requete, non suivie",
            "Chargement : SQL Procedure : procédure stockée dbo.ChargerClients, non suivie",
            "Export : SCR Source : source sans jeu de données identifié",
        ]
    );
}

#[test]
fn focus_keeps_what_feeds_and_what_reads_a_table() {
    let focused = focus(&build(), "dbo.Clients").expect("table trouvée");

    let expected = [
        "ADO Clients -> FFD Export",
        "D:\\demo\\clients.csv -> FF Clients",
        "DST Staging -> Entrepot : stg.Clients",
        "Entrepot : dbo.Clients -.-> SQL Historiser",
        "Entrepot : dbo.Clients -> ADO Clients",
        "Entrepot : ref.Pays -> LKP Pays",
        "Entrepot : ref.Pays -> SQL Alimenter Clients",
        "Entrepot : stg.Clients -> SQL Alimenter Clients",
        "FF Clients -> LKP Pays",
        "FFD Export -> D:\\demo\\export.csv",
        "LKP Pays -> DST Staging",
        "SQL Alimenter Clients -> Entrepot : dbo.Clients",
        "SQL Historiser -.-> Entrepot : dbo.Historique",
    ];
    assert_eq!(edges(&focused), expected);
}

#[test]
fn focus_ignores_case_and_other_branches_of_the_ancestors() {
    let focused = focus(&build(), "DBO.HISTORIQUE").expect("table trouvée");

    let edges = edges(&focused);
    assert!(edges.contains(&"SQL Historiser -.-> Entrepot : dbo.Historique".to_owned()));
    assert!(edges.contains(&"D:\\demo\\clients.csv -> FF Clients".to_owned()));
    assert!(
        !edges.iter().any(|e| e.contains("ADO Clients")),
        "{edges:?}"
    );
}

#[test]
fn focus_keeps_only_notes_of_the_packages_shown() {
    let focused = focus(&build(), "stg.Comptes").expect("table trouvée");

    assert_eq!(
        focused.notes,
        vec!["Export : SCR Source : source sans jeu de données identifié"]
    );
}

#[test]
fn focus_without_match_is_none() {
    assert!(focus(&build(), "inexistante").is_none());
}

#[test]
fn mermaid_lineage() {
    insta::assert_snapshot!(render::mermaid(&build()));
}
