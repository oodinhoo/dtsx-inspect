//! Package resté sous le nom par défaut `Package` : indiscernable dans les
//! journaux d'exécution et le catalogue SSIS.

use super::{Finding, Rule, Severity};
use crate::inventory::Inventory;

pub struct PackageNonRenomme;

impl Rule for PackageNonRenomme {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        inventory
            .packages
            .iter()
            .filter(|p| p.name == "Package")
            .map(|p| Finding {
                rule: "package-non-renomme",
                severity: Severity::Avertissement,
                file: p.file.clone(),
                location: "package".to_owned(),
                message: "nom par défaut « Package », le renommer comme son fichier".to_owned(),
            })
            .collect()
    }
}
