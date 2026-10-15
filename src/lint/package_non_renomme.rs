//! Package resté sous le nom par défaut `Package` : indiscernable dans les
//! journaux d'exécution et le catalogue SSIS.

use super::{finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;

pub struct PackageNonRenomme;

const DOC: RuleDoc = RuleDoc {
    id: "package-non-renomme",
    severity: Severity::Avertissement,
    title: "Package resté nommé « Package »",
    why: "Le nom interne du package apparaît dans le catalogue SSIS et les journaux : plusieurs packages « Package » y sont indiscernables.",
    fix: "Dans les propriétés du package, donner à Name le nom du fichier .dtsx.",
};

impl Rule for PackageNonRenomme {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        inventory
            .packages
            .iter()
            .filter(|p| p.name == "Package")
            .map(|p| {
                finding(
                    &DOC,
                    &p.file,
                    "package".to_owned(),
                    "nom par défaut « Package », le renommer comme son fichier",
                )
            })
            .collect()
    }
}
