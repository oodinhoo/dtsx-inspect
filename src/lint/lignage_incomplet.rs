//! Ce que le lignage n'a pas pu suivre (SQL non analysé,
//! requête en variable…) : autant d'angles morts dans la documentation.

use super::{finding, Finding, Rule, Severity};
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
                Some(finding(
                    "lignage-incomplet",
                    Severity::Info,
                    &package.file,
                    note.step.clone(),
                    note.reason.as_str(),
                ))
            })
            .collect()
    }
}
