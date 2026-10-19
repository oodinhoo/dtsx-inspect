//! Fiche de mapping source → cible en CSV (Excel français : `;` et BOM UTF-8). Une ligne
//! par colonne de table ou de fichier écrite et par colonne lue qui la donne, dans un
//! même package : le package, sa tâche ou son Data Flow, les transformations et un statut.

use std::collections::BTreeSet;

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

use super::columns::describe;
use super::html::project_of;
use crate::inventory::Inventory;
use crate::lineage::columns::{ColumnLineage, ColumnNode, How};

const HEADER: [&str; 9] = [
    "Projet",
    "Package",
    "Tâche ou Data Flow",
    "Colonne cible",
    "Serveur cible",
    "Colonne source",
    "Serveur source",
    "Transformations",
    "Statut",
];

/// Statuts d'une ligne, du plus sûr au moins sûr ; la dernière cellule de chaque ligne.
pub(super) const STATUSES: [&str; 4] = [
    "directe",
    "transformée",
    "par nom (SELECT *)",
    "origine inconnue",
];

/// Une ligne du mapping, cellules dans l'ordre de `HEADER`.
pub(super) type Row = [String; 9];

/// Une colonne lue (aucune si l'étape n'a pas d'entrée connue) et les façons dont elle
/// donne la colonne écrite, de la lecture à l'écriture.
type Origin = (Option<NodeIndex>, Vec<How>);

pub fn mapping_csv(inventory: &Inventory, lineage: &ColumnLineage) -> String {
    to_csv(&mapping_rows(inventory, lineage))
}

/// Les lignes du mapping, rangées : même entrée, même fichier.
pub(super) fn mapping_rows(inventory: &Inventory, lineage: &ColumnLineage) -> BTreeSet<Row> {
    let g = &lineage.graph;
    let mut rows: BTreeSet<Row> = BTreeSet::new();
    for target in g.node_indices() {
        if !matches!(g[target], ColumnNode::Dataset { .. }) {
            continue;
        }
        for write in g.edges_directed(target, Direction::Incoming) {
            let step = write.source();
            let ColumnNode::Step { package, flow, .. } = &g[step] else {
                continue;
            };
            let mut found = Vec::new();
            origins(
                lineage,
                step,
                &mut vec![*write.weight()],
                &mut vec![step],
                &mut found,
            );
            let file = inventory
                .packages
                .get(*package)
                .map_or("", |p| p.file.as_str());
            let name = lineage.packages.get(*package).map_or("?", String::as_str);
            for (source, mut hows) in found {
                hows.reverse();
                let labels: Vec<&str> = hows.iter().map(|h| h.label()).collect();
                rows.insert([
                    project_of(file).to_owned(),
                    name.to_owned(),
                    flow.clone(),
                    describe(lineage, target),
                    server(lineage, Some(target)),
                    source.map_or_else(String::new, |s| describe(lineage, s)),
                    server(lineage, source),
                    labels.join(" → "),
                    status(source, &hows).to_owned(),
                ]);
            }
        }
    }
    rows
}

/// CSV pour Excel français : BOM, `;`, fins de ligne `\r\n`.
pub(super) fn to_csv(rows: &BTreeSet<Row>) -> String {
    let header = HEADER.map(str::to_owned);
    let mut out = String::from("\u{feff}");
    for row in std::iter::once(&header).chain(rows) {
        let cells: Vec<String> = row.iter().map(|c| cell(c)).collect();
        out.push_str(&cells.join(";"));
        out.push_str("\r\n");
    }
    out
}

/// Vers l'amont, d'étape en étape du package, jusqu'aux colonnes de table ou de fichier
/// lues ; `hows` va de la colonne écrite vers l'amont.
fn origins(
    lineage: &ColumnLineage,
    node: NodeIndex,
    hows: &mut Vec<How>,
    path: &mut Vec<NodeIndex>,
    found: &mut Vec<Origin>,
) {
    let g = &lineage.graph;
    let mut incoming = g.edges_directed(node, Direction::Incoming).peekable();
    if incoming.peek().is_none() {
        found.push((None, hows.clone()));
        return;
    }
    for edge in incoming {
        let from = edge.source();
        hows.push(*edge.weight());
        match g[from] {
            ColumnNode::Dataset { .. } => found.push((Some(from), hows.clone())),
            // Une boucle entre étapes s'arrête à la deuxième visite.
            ColumnNode::Step { .. } if !path.contains(&from) => {
                path.push(from);
                origins(lineage, from, hows, path, found);
                path.pop();
            }
            ColumnNode::Step { .. } => {}
        }
        hows.pop();
    }
}

fn server(lineage: &ColumnLineage, node: Option<NodeIndex>) -> String {
    match node.map(|i| &lineage.graph[i]) {
        Some(ColumnNode::Dataset {
            server: Some(server),
            ..
        }) => server.clone(),
        _ => String::new(),
    }
}

/// Ce que vaut la ligne : un `SELECT *` rapproche les colonnes par leur nom, pas par
/// leur position comme SQL Server.
fn status(source: Option<NodeIndex>, hows: &[How]) -> &'static str {
    let [direct, transformed, by_name, unknown] = STATUSES;
    if source.is_none() {
        unknown
    } else if hows.contains(&How::SelectStar) {
        by_name
    } else if hows.iter().all(|h| matches!(h, How::Read | How::Write)) {
        direct
    } else {
        transformed
    }
}

/// Une cellule ne doit ni ouvrir une autre ligne ou colonne, ni s'exécuter comme une
/// formule Excel (`=`, `+`, `-`, `@` en tête : une apostrophe en fait du texte).
fn cell(text: &str) -> String {
    let text = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if text.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lineage::columns::column_lineage;

    fn mapping(dir: &str) -> String {
        let (inventory, _) = crate::inventory(std::path::Path::new(dir));
        mapping_csv(&inventory, &column_lineage(&inventory))
    }

    #[test]
    fn one_row_per_written_column_and_column_it_comes_from() {
        let csv = mapping("tests/fixtures/colonnes_sql");

        let mut lines = csv
            .strip_prefix('\u{feff}')
            .expect("BOM pour Excel")
            .lines();
        assert_eq!(
            lines.next(),
            Some(
                "Projet;Package;Tâche ou Data Flow;Colonne cible;Serveur cible;\
                 Colonne source;Serveur source;Transformations;Statut"
            )
        );
        let rows: Vec<&str> = lines.collect();
        for row in [
            ";Chargement;DFT Staging;Entrepot : stg.Clients.email;srv-demo;\
             Entrepot : ext.Clients.email;srv-demo;lecture → écriture;directe",
            ";Chargement;SQL Finaliser;Entrepot : dbo.Clients.email;srv-demo;\
             Entrepot : stg.Clients.email;srv-demo;SELECT * → écriture;par nom (SELECT *)",
            ";Chargement;SQL Resumer;Entrepot : dbo.Resume.courriel;srv-demo;\
             Entrepot : stg.Clients.email;srv-demo;calcul SQL → écriture;transformée",
        ] {
            assert!(rows.contains(&row), "{row}\n---\n{csv}");
        }
        // Rangées : même entrée, même fichier, d'une analyse à l'autre.
        let cells: Vec<Vec<&str>> = rows.iter().map(|r| r.split(';').collect()).collect();
        let mut sorted = cells.clone();
        sorted.sort();
        assert_eq!(cells, sorted);
    }

    #[test]
    fn columns_without_known_origin_get_their_own_row() {
        let csv = mapping("tests/fixtures/colonnes");

        assert!(
            csv.lines()
                .any(|l| l.ends_with(";origine inconnue") && l.contains(";;;")),
            "{csv}"
        );
    }

    #[test]
    fn never_writes_connection_strings_or_passwords() {
        let csv = mapping("tests/fixtures/lineage");

        assert!(csv.lines().count() > 1, "{csv}");
        let lower = csv.to_lowercase();
        assert!(
            !lower.contains("password") && !lower.contains("pwd="),
            "{csv}"
        );
    }

    #[test]
    fn cells_cannot_break_the_row_or_run_as_a_formula() {
        assert_eq!(cell("a;b"), "\"a;b\"");
        assert_eq!(cell("dit \"x\""), "\"dit \"\"x\"\"\"");
        assert_eq!(cell("ligne\nsuite"), "\"ligne\nsuite\"");
        assert_eq!(cell("=SOMME(A1)"), "'=SOMME(A1)");
        assert_eq!(cell("@x"), "'@x");
        assert_eq!(cell("dbo.Clients"), "dbo.Clients");
    }
}
