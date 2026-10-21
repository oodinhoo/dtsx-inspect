//! OLE DB destination without fast load (`AccessMode` 0 or 1): one insert per row
//! instead of a bulk load. The "Table or view - fast load" mode (`AccessMode` 3 or 4)
//! is the recommended usage.

use super::{components, finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::access_mode;
use crate::lineage::Lineage;

pub struct NoFastLoad;

const DOC: RuleDoc = RuleDoc {
    id: "no-fast-load",
    severity: Severity::Warning,
    title: "OLE DB destination inserting row by row",
    why: "In \"Table or view\" or \"Table or view - variable\" mode, the destination sends one INSERT per row: the load is dozens of times slower and fills the transaction log.",
    fix: "Choose \"Table or view - fast load\", then tune the batch sizes (Rows per batch, Maximum insert commit size).",
};

impl Rule for NoFastLoad {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        components(inventory)
            .filter(|(_, _, c)| {
                c.class_id == "Microsoft.OLEDBDestination" && matches!(access_mode(c), 0 | 1)
            })
            .map(|(p, location, _)| {
                finding(
                    &DOC,
                    &p.file,
                    location,
                    "row-by-row insert: choose \"Table or view - fast load\"",
                )
            })
            .collect()
    }
}
