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

/// Notes lisibles : `package : étape : raison`.
fn notes(l: &Lineage) -> Vec<String> {
    l.notes
        .iter()
        .map(|n| format!("{} : {} : {}", l.packages[n.package], n.step, n.reason))
        .collect()
}

#[test]
fn follows_data_from_files_and_entities_to_tables_across_packages() {
    let expected = [
        "ADO Clients -> FFD Export",
        "CMD Adresses -> DST Rejets",
        "CMD Adresses -> Entrepot : dbo.Adresses",
        "CMD Purge -> Entrepot : dbo.PurgerAdresse",
        "CRM : account -> CRM Comptes",
        "CRM : contact -> CRM Comptes",
        "CRM : lead -> CRM Prospects",
        "CRM Comptes -> UNI Fusion",
        "CRM Prospects -> UNI Fusion",
        "D:\\demo\\clients.csv -> FF Clients",
        "DST Comptes -> Entrepot : stg.Comptes",
        "DST Rejets -> Entrepot : stg.Rejets",
        "DST Staging -> Entrepot : stg.Clients",
        "Entrepot : dbo.ChargerClients -> Entrepot : dbo.Journaliser",
        "Entrepot : dbo.ChargerClients -> Entrepot : dbo.Synthese",
        "Entrepot : dbo.Clients -.-> ADO Clients",
        "Entrepot : dbo.Clients -.-> SQL Historiser",
        "Entrepot : dbo.Journaliser -> Entrepot : dbo.Journal",
        "Entrepot : ref.Pays -> LKP Pays",
        "Entrepot : ref.Pays -> SQL Alimenter Clients",
        "Entrepot : stg.Clients -> SQL Alimenter Clients",
        "Entrepot : stg.Delta -> Entrepot : dbo.ChargerClients",
        "Entrepot : stg.Delta -> SRC Delta",
        "FF Clients -> LKP Pays",
        "FFD Export -> D:\\demo\\export.csv",
        "LKP Pays -> DST Staging",
        "SQL Alimenter Clients -> Entrepot : dbo.Clients",
        "SQL Historiser -.-> Entrepot : dbo.Historique",
        "SQL Procedure -> Entrepot : dbo.ChargerClients",
        "SRC Delta -> CMD Adresses",
        "SRC Delta -> CMD Purge",
        "UNI Fusion -> DST Comptes",
    ];

    assert_eq!(edges(&build()), expected);
}

#[test]
fn sql_command_destinations_and_ole_db_commands_write_their_table() {
    let edges = edges(&build());

    for expected in [
        "DST Rejets -> Entrepot : stg.Rejets",
        "CMD Adresses -> Entrepot : dbo.Adresses",
    ] {
        assert!(
            edges.iter().any(|e| e == expected),
            "{expected} : {edges:?}"
        );
    }
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
    // `SQL Procedure` reste : elle appelle une procédure, nœud du lignage.
    assert_eq!(
        tasks,
        vec!["SQL Alimenter Clients", "SQL Historiser", "SQL Procedure"]
    );
}

#[test]
fn reports_what_could_not_be_followed() {
    assert_eq!(
        notes(&build()),
        vec![
            "Chargement : SQL Import brut : SQL non analysé",
            "Chargement : SQL Depuis variable : requête dans la variable User::Requete, non suivie",
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
        "Entrepot : dbo.Clients -.-> ADO Clients",
        "Entrepot : dbo.Clients -.-> SQL Historiser",
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
        notes(&focused),
        vec!["Export : SCR Source : source sans jeu de données identifié"]
    );
}

#[test]
fn focus_tells_homonymous_packages_apart() {
    let (inv, errors) = inventory(&fixture("homonymes"));
    assert!(errors.is_empty(), "{errors:?}");

    let focused = focus(&lineage(&inv), "dbo.A").expect("table trouvée");

    assert_eq!(
        notes(&focused),
        vec!["Chargement : SQL Procedure A : procédure stockée dbo.ProcA, non suivie"]
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

#[test]
fn table_creation_links_the_task_to_the_created_table() {
    let (inv, _) = inventory(&fixture("project"));
    let l = lineage(&inv);
    let created: Vec<String> = l
        .graph
        .edge_indices()
        .filter(|&e| l.graph[e].creates)
        .filter_map(|e| {
            let (from, to) = l.graph.edge_endpoints(e)?;
            Some(format!(
                "{} -> {}",
                l.graph[from].label(),
                l.graph[to].label()
            ))
        })
        .collect();
    assert_eq!(created.len(), 1, "{created:?}");
    assert!(created[0].ends_with(": dbo.Cible"), "{created:?}");
}

#[test]
fn procedures_defined_in_sql_scripts_are_followed() {
    let (inv, errors) = inventory(&fixture("procedures"));
    assert!(errors.is_empty(), "{errors:?}");

    let l = lineage(&inv);

    // Une procédure est un nœud : ses lectures y entrent, ses écritures et les
    // procédures qu'elle appelle en sortent. Une vue est alimentée par ses tables.
    // `dbo.Doublon`, défini deux fois, n'est pas développé : on ne devine pas.
    assert_eq!(
        edges(&l),
        vec![
            "Entrepot : dbo.Charger -> Entrepot : dbo.Clients",
            "Entrepot : dbo.Charger -> Entrepot : dbo.Journaliser",
            "Entrepot : dbo.Clients -> Entrepot : dbo.Purger",
            "Entrepot : dbo.Clients -> Entrepot : dbo.VClients",
            "Entrepot : dbo.Purger -> Entrepot : dbo.Archive",
            "Entrepot : dbo.VClients -> SQL Lire vue",
            "Entrepot : stg.Clients -> Entrepot : dbo.Charger",
            "SQL Charger -> Entrepot : dbo.Charger",
            "SQL Doublon -> Entrepot : dbo.Doublon",
            "SQL Inconnue -> Entrepot : dbo.Inconnue",
            "SQL Lire vue -> Entrepot : dbo.Rapport",
            "SQL Purger -> Entrepot : dbo.Purger",
        ]
    );
    // Seul l'appel d'un package est signalé : `dbo.Journaliser`, appelée par une
    // procédure, n'a pas de package auquel rattacher la note.
    assert_eq!(
        notes(&l),
        vec![
            "Appel : SQL Inconnue : procédure stockée dbo.Inconnue, non suivie",
            "Appel : SQL Doublon : procédure stockée dbo.Doublon définie dans plusieurs \
             scripts (Archive/dbo/Doublon.sql, Base/dbo/Doublon.sql), non suivie",
        ]
    );
}

#[test]
fn tables_are_identified_by_server_and_database_not_by_connection_name() {
    // `Entrepot` et `DW` visent la même base (casse et clés différentes) ; `Recette`
    // vise une base homonyme sur un autre serveur.
    let (inv, errors) = inventory(&fixture("serveurs"));
    assert!(errors.is_empty(), "{errors:?}");

    let l = lineage(&inv);

    let mut datasets: Vec<(Option<&str>, &str)> = l
        .graph
        .node_weights()
        .filter_map(|n| match n {
            Node::Dataset { server, label, .. } => Some((server.as_deref(), label.as_str())),
            _ => None,
        })
        .collect();
    datasets.sort();
    // Un seul `dbo.Clients` pour `Entrepot` et `DW` ; `dbo.Rapport`, vu seulement par
    // `DW`, garde l'écriture de sa chaîne (`SRV-DEMO`, `entrepot`).
    assert_eq!(
        datasets,
        vec![
            (Some("SRV-DEMO"), "entrepot : dbo.Rapport"),
            (Some("srv-demo"), "Entrepot : dbo.Clients"),
            (Some("srv-demo"), "Entrepot : stg.Clients"),
            (Some("srv-recette"), "Entrepot : dbo.Clients"),
            (Some("srv-recette"), "Entrepot : stg.Clients"),
        ]
    );
}
