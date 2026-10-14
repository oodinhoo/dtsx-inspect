//! `INSERT … SELECT *` : les colonnes correspondent par position, une colonne
//! ajoutée ou réordonnée d'un côté casse le chargement sans prévenir.

use super::{Finding, Rule, Severity};
use crate::inventory::Inventory;
use crate::lineage::sql::inserts_select_star;
use crate::model::SqlOrigin;

pub struct InsertSelectEtoile;

impl Rule for InsertSelectEtoile {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        inventory
            .packages
            .iter()
            .flat_map(|p| {
                p.executables
                    .iter()
                    .filter(|e| {
                        e.sql.as_ref().is_some_and(|sql| {
                            matches!(sql.origin, SqlOrigin::Direct | SqlOrigin::Expression)
                                && inserts_select_star(&sql.text)
                        })
                    })
                    .map(move |e| Finding {
                        rule: "insert-select-etoile",
                        severity: Severity::Avertissement,
                        file: p.file.clone(),
                        location: e.path().to_owned(),
                        message: "INSERT … SELECT * : lister les colonnes des deux côtés"
                            .to_owned(),
                    })
            })
            .collect()
    }
}
