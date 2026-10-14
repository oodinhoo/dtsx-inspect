//! Ce que le lignage n'a pas pu suivre (SQL non analysé, procédure stockée,
//! requête en variable…) : autant d'angles morts dans la documentation.

use super::{Finding, Rule, Severity};
use crate::inventory::Inventory;
use crate::lineage::lineage;

pub struct LignageIncomplet;

impl Rule for LignageIncomplet {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        lineage(inventory)
            .notes
            .iter()
            .filter_map(|note| {
                let package = inventory.packages.get(note.package)?;
                let (step, reason) = note.text.split_once(" : ").unwrap_or((&note.text, ""));
                Some(Finding {
                    rule: "lignage-incomplet",
                    severity: Severity::Info,
                    file: package.file.clone(),
                    location: step.to_owned(),
                    message: reason.to_owned(),
                })
            })
            .collect()
    }
}
