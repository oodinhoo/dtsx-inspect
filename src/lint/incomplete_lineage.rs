//! What the lineage could not trace (SQL not analyzed, query in a variable…): as many
//! blind spots in the documentation.

use super::{finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::Lineage;

pub struct IncompleteLineage;

const DOC: RuleDoc = RuleDoc {
    id: "incomplete-lineage",
    severity: Severity::Info,
    title: "Step the lineage could not trace",
    why: "SQL that cannot be parsed, query read from a variable or a file, stored procedure without definition: the lineage documentation has a blind spot there.",
    fix: "Nothing mandatory. To fill the gap: add the .sql scripts of the procedures to the analyzed folder, or replace the query in a variable by a query typed in directly.",
};

impl Rule for IncompleteLineage {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, lineage: &Lineage) -> Vec<Finding> {
        lineage
            .notes
            .iter()
            .filter_map(|note| {
                let package = inventory.packages.get(note.package)?;
                Some(finding(
                    &DOC,
                    &package.file,
                    note.step.clone(),
                    note.reason.as_str(),
                ))
            })
            .collect()
    }
}
