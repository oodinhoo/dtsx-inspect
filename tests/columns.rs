use std::path::PathBuf;

use dtsx_inspect::model::{Column, Component, Package};
use dtsx_inspect::parser::parse_package;

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

fn package(relative: &str) -> Package {
    let xml = std::fs::read_to_string(fixture(relative)).expect("readable fixture");
    parse_package(&xml).expect("valid package")
}

fn component<'a>(p: &'a Package, name: &str) -> &'a Component {
    p.executables
        .iter()
        .flat_map(|e| &e.components)
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("component {name}"))
}

/// Column name of a `…Columns[name]` identifier.
fn short(id: &str) -> String {
    id.rsplit_once(".Columns[")
        .map_or(id, |(_, c)| c.trim_end_matches(']'))
        .to_owned()
}

fn shorts(ids: &[String]) -> Vec<String> {
    ids.iter().map(|id| short(id)).collect()
}

fn named(columns: &[Column]) -> Vec<(&str, Option<&str>)> {
    columns
        .iter()
        .map(|c| (c.name.as_str(), c.external.as_deref()))
        .collect()
}

#[test]
fn reads_source_columns_and_the_file_columns_they_come_from() {
    let p = package("columns/Clients.dtsx");

    assert_eq!(
        named(&component(&p, "FF Clients").outputs),
        vec![
            ("id", Some("id")),
            ("email", Some("email")),
            ("pays_code", Some("pays_code")),
        ]
    );
}

#[test]
fn reads_how_transformations_compute_their_columns() {
    let p = package("columns/Clients.dtsx");

    let derived = &component(&p, "DER Email").outputs[0];
    assert_eq!(derived.name, "email_norm");
    assert_eq!(shorts(&derived.sources), ["email"]);
    // The error output (`ErrorCode`…) is not data: ignored.
    let converted = &component(&p, "DCV Id").outputs;
    assert_eq!(named(converted), vec![("id_int", None)]);
    assert_eq!(shorts(&converted[0].sources), ["id"]);
    let lookup = &component(&p, "LKP Pays").outputs[0];
    assert_eq!(lookup.reference.as_deref(), Some("libelle"));
    // Script component: no known origin.
    assert!(component(&p, "SCR Note").outputs[0].sources.is_empty());
}

#[test]
fn reads_destination_columns_and_the_table_columns_they_fill() {
    let p = package("columns/Clients.dtsx");

    let filled: Vec<(String, Option<&str>)> = component(&p, "DST Clients")
        .inputs
        .iter()
        .map(|c| (short(&c.lineage), c.external.as_deref()))
        .collect();
    assert_eq!(
        filled,
        vec![
            ("id_int".to_owned(), Some("id")),
            ("email_norm".to_owned(), Some("email")),
            ("pays_libelle".to_owned(), Some("pays")),
            ("note".to_owned(), Some("note")),
        ]
    );
}

#[test]
fn union_all_inputs_name_the_output_column_they_feed() {
    let p = package("columns/Export.dtsx");

    let input = &component(&p, "UNI Fusion").inputs[0];
    assert_eq!(short(&input.lineage), "email");
    assert_eq!(
        input.feeds.as_deref().map(short).as_deref(),
        Some("courriel")
    );
}

fn trace(needle: &str) -> Option<String> {
    let (inv, errors) = dtsx_inspect::inventory(&fixture("columns"));
    assert!(errors.is_empty(), "{errors:?}");
    dtsx_inspect::render::column_trace(
        &dtsx_inspect::lineage::columns::column_lineage(&inv),
        needle,
    )
}

#[test]
fn traces_a_table_column_back_to_its_file_and_on_to_other_packages() {
    // Upstream: written from a derived column, computed from the file read.
    // Downstream: read again by two other packages. Export (same server and database,
    // other connection) merges it through Union All and writes it to a file;
    // Synthese reads it through the query of its source, under the alias `courriel`.
    let expected = [
        "Entrepot : dbo.Clients.email (srv-demo)",
        "  ← write: DER Email.email_norm (Clients › DFT Clients)",
        "    ← expression: FF Clients.email (Clients › DFT Clients)",
        r"      ← read: D:\demo\clients.csv.email",
        "  → read: SRC Clients.email (Export › DFT Export)",
        "    → Union All: UNI Fusion.courriel (Export › DFT Export)",
        r"      → write: D:\demo\export.csv.courriel",
        "  → read: SRC Commandes.courriel (Synthese › DFT Synthese)",
        "    → write: Entrepot : dbo.Synthese.email",
    ];

    assert_eq!(
        trace("dbo.Clients.email").expect("column found"),
        expected.join("\n") + "\n"
    );
}

#[test]
fn conversions_and_lookups_are_followed() {
    let id = trace("dbo.Clients.id").expect("column found");
    assert!(id.contains("← conversion: FF Clients.id"), "{id}");

    let pays = trace("dbo.Clients.pays").expect("column found");
    assert!(
        pays.contains("← lookup: Entrepot : ref.Pays.libelle"),
        "{pays}"
    );
}

#[test]
fn a_column_without_known_origin_says_so() {
    let note = trace("dbo.Clients.note").expect("column found");
    assert!(
        note.contains("SCR Note.note (Clients › DFT Clients), unknown origin"),
        "{note}"
    );
}

#[test]
fn an_unknown_column_is_none() {
    assert_eq!(trace("dbo.Clients.missing"), None);
}

#[test]
fn source_queries_lead_to_the_table_columns_of_their_select() {
    // `c.email AS courriel` comes from dbo.Clients.email, itself loaded by another
    // package from a file: the chain goes on.
    let email = trace("dbo.Synthese.email").expect("column found");
    assert!(
        email.contains("← write: SRC Commandes.courriel (Synthese › DFT Synthese)"),
        "{email}"
    );
    assert!(
        email.contains("← read: Entrepot : dbo.Clients.email"),
        "{email}"
    );
    assert!(email.contains("← read: D:"), "{email}");

    let pays = trace("dbo.Synthese.pays").expect("column found");
    assert!(
        pays.contains("← SQL expression: Entrepot : ref.Pays.libelle"),
        "{pays}"
    );

    let montant = trace("dbo.Synthese.montant").expect("column found");
    assert!(
        montant.contains("← read: Entrepot : dbo.Commandes.montant"),
        "{montant}"
    );
}

fn sql_trace(needle: &str) -> String {
    let (inv, errors) = dtsx_inspect::inventory(&fixture("columns_sql"));
    assert!(errors.is_empty(), "{errors:?}");
    dtsx_inspect::render::column_trace(
        &dtsx_inspect::lineage::columns::column_lineage(&inv),
        needle,
    )
    .unwrap_or_else(|| panic!("column {needle}"))
}

#[test]
fn insert_select_star_tasks_carry_columns_by_name() {
    // `INSERT INTO dbo.Archive SELECT * FROM dbo.Clients` comes before the task filling
    // dbo.Clients from the staging table: the chain is traced all the same.
    let expected = [
        "Entrepot : dbo.Archive.email (srv-demo)",
        "  ← write: SQL Archiver.email (Chargement › SQL Archiver)",
        "    ← SELECT *: Entrepot : dbo.Clients.email",
        "      ← write: SQL Finaliser.email (Chargement › SQL Finaliser)",
        "        ← SELECT *: Entrepot : stg.Clients.email",
        "          ← write: SRC Externe.email (Chargement › DFT Staging)",
        "            ← read: Entrepot : ext.Clients.email",
    ];

    assert_eq!(sql_trace("dbo.Archive.email"), expected.join("\n") + "\n");
}

#[test]
fn insert_with_a_column_list_pairs_columns_by_position() {
    let courriel = sql_trace("dbo.Resume.courriel");
    assert!(
        courriel.contains(
            "← write: SQL Resumer.courriel (Chargement › SQL Resumer)\n    \
             ← SQL expression: Entrepot : stg.Clients.email"
        ),
        "{courriel}"
    );
    let numero = sql_trace("dbo.Resume.numero");
    assert!(
        numero.contains("← read: Entrepot : stg.Clients.id"),
        "{numero}"
    );
}
