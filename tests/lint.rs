use std::path::PathBuf;

use dtsx_inspect::inventory;
use dtsx_inspect::lineage::{lineage, Lineage};
use dtsx_inspect::lint::lint;

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

/// `rule|file|location`, in report order.
fn findings(relative: &str) -> Vec<String> {
    let (inv, errors) = inventory(&fixture(relative));
    assert!(errors.is_empty(), "{errors:?}");
    lint(&inv, &lineage(&inv))
        .iter()
        .map(|f| format!("{}|{}|{}", f.rule, f.file, f.location))
        .collect()
}

fn assert_contains(found: &[String], expected: &[&str]) {
    for e in expected {
        assert!(found.iter().any(|f| f == e), "{e} missing from {found:?}");
    }
}

/// The demo project of the README stays readable and keeps its deliberate anti-patterns.
#[test]
fn demo_project_shows_its_findings() {
    assert_contains(
        &findings("../../examples/demo"),
        &[
            "obsolete-provider|RetailDW|connection Sales",
            "no-fast-load|RetailDW/11_Extract_Orders.dtsx|DFT Orders\\DST Orders",
            "sort-in-data-flow|RetailDW/11_Extract_Orders.dtsx|DFT Orders\\SRT Order",
            "default-name|RetailDW/12_Extract_Accounts.dtsx|DFT Accounts\\OLE DB Destination",
            "insert-select-star|RetailDW/20_Load_Warehouse.dtsx|SQL Load FactSales",
            "disabled-task|RetailDW/20_Load_Warehouse.dtsx|SQL Rebuild indexes",
        ],
    );
}

#[test]
fn flags_obsolete_provider_dead_reference_and_disabled_task() {
    assert_contains(
        &findings("minimal.dtsx"),
        &[
            "obsolete-provider|minimal.dtsx|connection Entrepot",
            "unresolved-connection|minimal.dtsx|SQL Ancienne purge",
            "disabled-task|minimal.dtsx|SQL Ancienne purge",
        ],
    );
}

#[test]
fn project_connections_are_reported_once() {
    let found = findings("lineage");

    let providers: Vec<_> = found
        .iter()
        .filter(|f| f.starts_with("obsolete-provider|"))
        .collect();
    assert_eq!(providers, ["obsolete-provider|project|connection Entrepot"]);
}

#[test]
fn flags_insert_select_star_and_incomplete_lineage() {
    assert_contains(
        &findings("lineage"),
        &[
            "insert-select-star|Chargement.dtsx|SQL Historiser",
            "incomplete-lineage|Chargement.dtsx|SQL Import brut",
            "incomplete-lineage|Export.dtsx|SCR Source",
        ],
    );
}

#[test]
fn flags_package_left_with_default_name() {
    assert_eq!(
        findings("lint/sans_nom.dtsx"),
        ["unrenamed-package|sans_nom.dtsx|package"]
    );
}

#[test]
fn clean_package_has_no_finding() {
    assert!(findings("broken/valide.dtsx").is_empty());
}

#[test]
fn flags_ssis_design_rules_from_microsoft_guidance() {
    let found = findings("lint/normes.dtsx");
    assert_contains(
        &found,
        &[
            r"unused-columns|normes.dtsx|Data Flow Task\SRC Table",
            r"unused-columns|normes.dtsx|Data Flow Task\LKP Pays",
            r"sort-in-data-flow|normes.dtsx|Data Flow Task\Trier",
            r"row-by-row-command|normes.dtsx|Data Flow Task\CMD Maj",
            r"no-fast-load|normes.dtsx|Data Flow Task\DST Lent",
            "default-name|normes.dtsx|Data Flow Task",
            r"default-name|normes.dtsx|Data Flow Task\Trier",
            r"default-name|normes.dtsx|Data Flow Task\Multidiffusion 1",
        ],
    );
    // Explicit query and fast load: nothing to report.
    for clean in ["SRC Requete", "DST Rapide"] {
        assert!(
            !found.iter().any(|f| f.ends_with(clean)),
            "{clean}: {found:?}"
        );
    }
}

#[test]
fn applies_component_rules_to_ssis_2012_packages() {
    assert_contains(
        &findings("format6"),
        &[
            r"sort-in-data-flow|Ancien.dtsx|DFT Anciens\TRI Clients",
            r"no-fast-load|Ancien.dtsx|DFT Anciens\DST Clients",
        ],
    );
}

#[test]
fn project_connections_are_reported_once_per_project() {
    let found = findings("multi");

    let providers: Vec<_> = found
        .iter()
        .filter(|f| f.starts_with("obsolete-provider|"))
        .collect();
    assert_eq!(
        providers,
        [
            "obsolete-provider|a|connection Cible",
            "obsolete-provider|b|connection Cible"
        ]
    );
}

#[test]
fn lint_reuses_the_lineage_it_is_given() {
    let (inv, errors) = inventory(&fixture("lineage"));
    assert!(errors.is_empty(), "{errors:?}");

    let found = lint(&inv, &Lineage::default());

    assert!(
        !found.iter().any(|f| f.rule == "incomplete-lineage"),
        "{found:?}"
    );
}
