use std::path::PathBuf;

use dtsx_inspect::model::{Column, Component, Package};
use dtsx_inspect::parser::parse_package;

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

fn package(relative: &str) -> Package {
    let xml = std::fs::read_to_string(fixture(relative)).expect("fixture lisible");
    parse_package(&xml).expect("package valide")
}

fn component<'a>(p: &'a Package, name: &str) -> &'a Component {
    p.executables
        .iter()
        .flat_map(|e| &e.components)
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("composant {name}"))
}

/// Nom de colonne d'un identifiant `…Columns[nom]`.
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
    let p = package("colonnes/Clients.dtsx");

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
    let p = package("colonnes/Clients.dtsx");

    let derived = &component(&p, "DER Email").outputs[0];
    assert_eq!(derived.name, "email_norm");
    assert_eq!(shorts(&derived.sources), ["email"]);
    // La sortie d'erreur (`ErrorCode`…) n'est pas une donnée : ignorée.
    let converted = &component(&p, "DCV Id").outputs;
    assert_eq!(named(converted), vec![("id_int", None)]);
    assert_eq!(shorts(&converted[0].sources), ["id"]);
    let lookup = &component(&p, "LKP Pays").outputs[0];
    assert_eq!(lookup.reference.as_deref(), Some("libelle"));
    // Composant script : aucune origine connue.
    assert!(component(&p, "SCR Note").outputs[0].sources.is_empty());
}

#[test]
fn reads_destination_columns_and_the_table_columns_they_fill() {
    let p = package("colonnes/Clients.dtsx");

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
    let p = package("colonnes/Export.dtsx");

    let input = &component(&p, "UNI Fusion").inputs[0];
    assert_eq!(short(&input.lineage), "email");
    assert_eq!(
        input.feeds.as_deref().map(short).as_deref(),
        Some("courriel")
    );
}

fn trace(needle: &str) -> Option<String> {
    let (inv, errors) = dtsx_inspect::inventory(&fixture("colonnes"));
    assert!(errors.is_empty(), "{errors:?}");
    dtsx_inspect::render::column_trace(
        &dtsx_inspect::lineage::columns::column_lineage(&inv),
        needle,
    )
}

#[test]
fn traces_a_table_column_back_to_its_file_and_on_to_other_packages() {
    // Amont : écrite depuis une colonne dérivée, calculée depuis le fichier lu.
    // Aval : relue par deux autres packages. Export (même serveur et même base,
    // autre connexion) la fusionne par Unir tout et l'écrit dans un fichier ;
    // Synthese la lit par la requête de sa source, sous l'alias `courriel`.
    let expected = [
        "Entrepot : dbo.Clients.email (srv-demo)",
        "  ← écriture : DER Email.email_norm (Clients › DFT Clients)",
        "    ← expression : FF Clients.email (Clients › DFT Clients)",
        r"      ← lecture : D:\demo\clients.csv.email",
        "  → lecture : SRC Clients.email (Export › DFT Export)",
        "    → Unir tout : UNI Fusion.courriel (Export › DFT Export)",
        r"      → écriture : D:\demo\export.csv.courriel",
        "  → lecture : SRC Commandes.courriel (Synthese › DFT Synthese)",
        "    → écriture : Entrepot : dbo.Synthese.email",
    ];

    assert_eq!(
        trace("dbo.Clients.email").expect("colonne trouvée"),
        expected.join("\n") + "\n"
    );
}

#[test]
fn conversions_and_lookups_are_followed() {
    let id = trace("dbo.Clients.id").expect("colonne trouvée");
    assert!(id.contains("← conversion : FF Clients.id"), "{id}");

    let pays = trace("dbo.Clients.pays").expect("colonne trouvée");
    assert!(
        pays.contains("← recherche : Entrepot : ref.Pays.libelle"),
        "{pays}"
    );
}

#[test]
fn a_column_without_known_origin_says_so() {
    let note = trace("dbo.Clients.note").expect("colonne trouvée");
    assert!(
        note.contains("SCR Note.note (Clients › DFT Clients), origine inconnue"),
        "{note}"
    );
}

#[test]
fn an_unknown_column_is_none() {
    assert_eq!(trace("dbo.Clients.inexistante"), None);
}

#[test]
fn source_queries_lead_to_the_table_columns_of_their_select() {
    // `c.email AS courriel` vient de dbo.Clients.email, elle-même chargée par un
    // autre package depuis un fichier : la chaîne continue.
    let email = trace("dbo.Synthese.email").expect("colonne trouvée");
    assert!(
        email.contains("← écriture : SRC Commandes.courriel (Synthese › DFT Synthese)"),
        "{email}"
    );
    assert!(
        email.contains("← lecture : Entrepot : dbo.Clients.email"),
        "{email}"
    );
    assert!(email.contains("← lecture : D:"), "{email}");

    let pays = trace("dbo.Synthese.pays").expect("colonne trouvée");
    assert!(
        pays.contains("← calcul SQL : Entrepot : ref.Pays.libelle"),
        "{pays}"
    );

    let montant = trace("dbo.Synthese.montant").expect("colonne trouvée");
    assert!(
        montant.contains("← lecture : Entrepot : dbo.Commandes.montant"),
        "{montant}"
    );
}
