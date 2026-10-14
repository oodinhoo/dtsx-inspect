use std::path::PathBuf;

use dtsx_inspect::inventory;
use dtsx_inspect::lint::lint;

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

/// `règle|fichier|emplacement`, dans l'ordre du rapport.
fn findings(relative: &str) -> Vec<String> {
    let (inv, errors) = inventory(&fixture(relative));
    assert!(errors.is_empty(), "{errors:?}");
    lint(&inv)
        .iter()
        .map(|f| format!("{}|{}|{}", f.rule, f.file, f.location))
        .collect()
}

fn assert_contains(found: &[String], expected: &[&str]) {
    for e in expected {
        assert!(found.iter().any(|f| f == e), "{e} absent de {found:?}");
    }
}

#[test]
fn flags_obsolete_provider_dead_reference_and_disabled_task() {
    assert_contains(
        &findings("minimal.dtsx"),
        &[
            "fournisseur-obsolete|minimal.dtsx|connexion Entrepot",
            "connexion-non-resolue|minimal.dtsx|SQL Ancienne purge",
            "tache-desactivee|minimal.dtsx|SQL Ancienne purge",
        ],
    );
}

#[test]
fn project_connections_are_reported_once() {
    let found = findings("lineage");

    let providers: Vec<_> = found
        .iter()
        .filter(|f| f.starts_with("fournisseur-obsolete|"))
        .collect();
    assert_eq!(
        providers,
        ["fournisseur-obsolete|projet|connexion Entrepot"]
    );
}

#[test]
fn flags_insert_select_star_and_incomplete_lineage() {
    assert_contains(
        &findings("lineage"),
        &[
            "insert-select-etoile|Chargement.dtsx|SQL Historiser",
            "lignage-incomplet|Chargement.dtsx|SQL Import brut",
            "lignage-incomplet|Chargement.dtsx|SQL Procedure",
            "lignage-incomplet|Export.dtsx|SCR Source",
        ],
    );
}

#[test]
fn flags_package_left_with_default_name() {
    assert_eq!(
        findings("lint/sans_nom.dtsx"),
        ["package-non-renomme|sans_nom.dtsx|package"]
    );
}

#[test]
fn clean_package_has_no_finding() {
    assert!(findings("broken/valide.dtsx").is_empty());
}
