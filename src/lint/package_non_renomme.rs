//! Package resté sous le nom par défaut `Package` : indiscernable dans les
//! journaux d'exécution et le catalogue SSIS.

use super::{finding, Finding, Rule, Severity};
use crate::inventory::Inventory;

pub struct PackageNonRenomme;

impl Rule for PackageNonRenomme {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        inventory
            .packages
            .iter()
            .filter(|p| p.name == "Package")
            .map(|p| {
                finding(
                    "package-non-renomme",
                    Severity::Avertissement,
                    &p.file,
                    "package".to_owned(),
                    "nom par défaut « Package », le renommer comme son fichier",
                )
            })
            .collect()
    }
}
