//! OLE DB Command: one query run for each row of the flow. Microsoft (*OLE DB Command
//! Transformation*, *Data Flow Performance Features*): write the rows to a staging
//! table then run a set-based UPDATE.

use super::{components, finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::Lineage;

pub struct RowByRowCommand;

const DOC: RuleDoc = RuleDoc {
    id: "row-by-row-command",
    severity: Severity::Warning,
    title: "OLE DB Command in a Data Flow",
    why: "The component runs its query once per row: as many round trips to the server as rows, which gets very slow from a few thousand rows on.",
    fix: "Load the rows into a staging table, then apply a single set-based query (UPDATE … FROM, MERGE) in an SQL task after the Data Flow.",
};

impl Rule for RowByRowCommand {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        components(inventory)
            .filter(|(_, _, c)| c.class_id == "Microsoft.OLEDBCommand")
            .map(|(p, location, _)| {
                finding(
                    &DOC,
                    &p.file,
                    location,
                    "one query per row: load a staging table then run a single set-based query",
                )
            })
            .collect()
    }
}
