//! `INSERT … SELECT *` : les colonnes correspondent par position, une colonne
//! ajoutée ou réordonnée d'un côté casse le chargement sans prévenir.

use super::{finding, tasks, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::sql::inserts_select_star;
use crate::lineage::Lineage;
use crate::model::SqlOrigin;

pub struct InsertSelectEtoile;

const DOC: RuleDoc = RuleDoc {
    id: "insert-select-etoile",
    severity: Severity::Avertissement,
    title: "INSERT … SELECT * dans une tâche SQL",
    why: "Les colonnes sont appariées par leur position : une colonne ajoutée ou réordonnée d'un côté décale les données ou fait échouer le chargement sans message clair.",
    fix: "Lister les colonnes des deux côtés : INSERT INTO cible (a, b) SELECT a, b FROM source.",
};

impl Rule for InsertSelectEtoile {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        tasks(inventory)
            .filter(|(_, e)| {
                e.sql.as_ref().is_some_and(|sql| {
                    matches!(sql.origin, SqlOrigin::Direct | SqlOrigin::Expression)
                        && inserts_select_star(&sql.text)
                })
            })
            .map(|(p, e)| {
                finding(
                    &DOC,
                    &p.file,
                    e.path().to_owned(),
                    "INSERT … SELECT * : lister les colonnes des deux côtés",
                )
            })
            .collect()
    }
}
