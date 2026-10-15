//! `INSERT … SELECT *` : les colonnes correspondent par position, une colonne
//! ajoutée ou réordonnée d'un côté casse le chargement sans prévenir.

use super::{finding, tasks, Finding, Rule, Severity};
use crate::inventory::Inventory;
use crate::lineage::sql::inserts_select_star;
use crate::model::SqlOrigin;

pub struct InsertSelectEtoile;

impl Rule for InsertSelectEtoile {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        tasks(inventory)
            .filter(|(_, e)| {
                e.sql.as_ref().is_some_and(|sql| {
                    matches!(sql.origin, SqlOrigin::Direct | SqlOrigin::Expression)
                        && inserts_select_star(&sql.text)
                })
            })
            .map(|(p, e)| {
                finding(
                    "insert-select-etoile",
                    Severity::Avertissement,
                    &p.file,
                    e.path().to_owned(),
                    "INSERT … SELECT * : lister les colonnes des deux côtés",
                )
            })
            .collect()
    }
}
