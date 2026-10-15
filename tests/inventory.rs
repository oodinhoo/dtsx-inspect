use std::path::PathBuf;

use dtsx_inspect::model::{ConnectionRef, Package};
use dtsx_inspect::{inventory, render};

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

fn pair(r: &ConnectionRef) -> (&str, Option<&str>) {
    (r.reference.as_str(), r.resolved.as_deref())
}

/// Références de connexion du package, tâches puis composants, dans l'ordre.
fn resolved(package: &Package) -> Vec<(&str, Option<&str>)> {
    package
        .executables
        .iter()
        .flat_map(|e| {
            e.connections.iter().map(pair).chain(
                e.components
                    .iter()
                    .flat_map(|c| c.connections.iter().map(pair)),
            )
        })
        .collect()
}

#[test]
fn folder_reads_project_connections_and_resolves_every_reference() {
    let (inv, errors) = inventory(&fixture("project"));

    assert!(errors.is_empty(), "{errors:?}");
    let names: Vec<_> = inv
        .project_connections
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, vec!["Cible", "Source CSV"]);
    assert_eq!(inv.packages.len(), 1);
    assert_eq!(inv.packages[0].file, "Chargement.dtsx");
    assert_eq!(
        resolved(&inv.packages[0]),
        vec![
            ("{CCCCCCCC-0000-0000-0000-000000000001}", Some("Cible")),
            (
                "{CCCCCCCC-0000-0000-0000-000000000002}:external",
                Some("Source CSV")
            ),
            (
                "{cccccccc-0000-0000-0000-000000000001}:external",
                Some("Cible")
            ),
        ]
    );
}

#[test]
fn single_file_also_reads_sibling_conmgr() {
    let (inv, errors) = inventory(&fixture("project/Chargement.dtsx"));

    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(inv.project_connections.len(), 2);
    assert_eq!(inv.packages.len(), 1);
    assert_eq!(inv.packages[0].file, "Chargement.dtsx");
}

#[test]
fn conmgr_given_as_path_is_not_counted_twice() {
    let (inv, errors) = inventory(&fixture("project/Cible.conmgr"));

    assert_eq!(inv.project_connections.len(), 2);
    assert!(inv.packages.is_empty());
    assert_eq!(errors.len(), 1, "{errors:?}");
}

/// Deux projets voisins ont chacun une connexion `Cible` : chaque package ne
/// voit que celles de son dossier, et liste celles qu'il utilise.
#[test]
fn each_package_only_sees_the_connections_of_its_own_project() {
    let (inv, errors) = inventory(&fixture("multi"));

    assert!(errors.is_empty(), "{errors:?}");
    let used: Vec<_> = inv
        .packages
        .iter()
        .map(|p| {
            let targets: Vec<_> = p.connections.iter().map(|c| c.target.as_deref()).collect();
            (p.name.as_str(), targets)
        })
        .collect();
    assert_eq!(
        used,
        vec![
            ("A", vec![Some("Data Source=srv-a;Initial Catalog=Cible")]),
            ("B", vec![Some("Data Source=srv-b;Initial Catalog=Cible")]),
        ]
    );
    assert_eq!(
        resolved(&inv.packages[1]),
        vec![("Project.ConnectionManagers[Cible]", Some("Cible"))]
    );
}

#[test]
fn package_references_are_resolved_and_unknown_ones_kept_raw() {
    let (inv, errors) = inventory(&fixture("minimal.dtsx"));

    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(
        resolved(&inv.packages[0]),
        vec![
            ("{AAAAAAAA-0000-0000-0000-000000000001}", Some("Entrepot")),
            (
                "{AAAAAAAA-0000-0000-0000-000000000002}",
                Some("Fichier clients")
            ),
            ("{AAAAAAAA-0000-0000-0000-000000000001}", Some("Entrepot")),
            ("Package.ConnectionManagers[CRM]", Some("CRM")),
            ("{BBBBBBBB-0000-0000-0000-00000000DEAD}", None),
            ("{aaaaaaaa-0000-0000-0000-000000000001}", Some("Entrepot")),
        ]
    );
}

#[test]
fn broken_files_are_reported_without_stopping_the_others() {
    let (inv, errors) = inventory(&fixture("broken"));

    let names: Vec<_> = inv.packages.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["Valide"]);
    let paths: Vec<_> = errors.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(paths, vec!["ancien.dtsx", "corrompu.dtsx"]);
}

#[test]
fn missing_path_is_reported() {
    let (inv, errors) = inventory(&fixture("absent"));

    assert!(inv.packages.is_empty());
    assert_eq!(errors.len(), 1);
}

#[test]
fn markdown_minimal() {
    insta::assert_snapshot!(render::markdown(&inventory(&fixture("minimal.dtsx")).0));
}

#[test]
fn markdown_orchestrateur() {
    insta::assert_snapshot!(render::markdown(
        &inventory(&fixture("orchestrateur.dtsx")).0
    ));
}

#[test]
fn markdown_project() {
    insta::assert_snapshot!(render::markdown(&inventory(&fixture("project")).0));
}

#[test]
fn json_project() {
    let json = render::json(&inventory(&fixture("project")).0).expect("json");
    insta::assert_snapshot!(json);
}

#[test]
fn skips_build_copies_when_sources_are_next_to_them() {
    // `obj\Development` est recopié par Visual Studio : sans les ignorer, chaque
    // package compterait deux fois. Un projet sans sources garde sa copie.
    let (inv, errors) = inventory(&fixture("build_copy"));
    assert!(errors.is_empty(), "{errors:?}");
    let files: Vec<&str> = inv.packages.iter().map(|p| p.file.as_str()).collect();
    assert_eq!(
        files,
        vec!["Projet/Paquet.dtsx", "Seul/obj/Development/Seul.dtsx"]
    );
}

#[test]
fn reads_procedures_and_views_of_sql_scripts_whatever_their_encoding() {
    // Charger.sql : UTF-8 avec BOM ; Purger.sql : UTF-16 (fréquent sous SSDT).
    let (inv, errors) = inventory(&fixture("procedures"));
    assert!(errors.is_empty(), "{errors:?}");
    let scripts: Vec<(&str, String)> = inv
        .sql_scripts
        .iter()
        .map(|d| (d.file.as_str(), d.name.to_string()))
        .collect();
    assert_eq!(
        scripts,
        vec![
            ("Archive/dbo/Doublon.sql", "dbo.Doublon".to_owned()),
            ("Base/dbo/Charger.sql", "dbo.Charger".to_owned()),
            ("Base/dbo/Doublon.sql", "dbo.Doublon".to_owned()),
            ("Base/dbo/Purger.sql", "dbo.Purger".to_owned()),
            ("Base/dbo/VClients.sql", "dbo.VClients".to_owned()),
        ]
    );
}
