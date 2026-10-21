//! Package still named `Package` by default: indistinguishable in the execution logs
//! and the SSIS catalog.

use super::{finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::Lineage;

pub struct UnrenamedPackage;

const DOC: RuleDoc = RuleDoc {
    id: "unrenamed-package",
    severity: Severity::Warning,
    title: "Package still named \"Package\"",
    why: "The internal package name shows up in the SSIS catalog and the logs: several \"Package\" packages are indistinguishable there.",
    fix: "In the package properties, set Name to the name of the .dtsx file.",
};

impl Rule for UnrenamedPackage {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        inventory
            .packages
            .iter()
            .filter(|p| p.name == "Package")
            .map(|p| {
                finding(
                    &DOC,
                    &p.file,
                    "package".to_owned(),
                    "default name \"Package\", rename it after its file",
                )
            })
            .collect()
    }
}
