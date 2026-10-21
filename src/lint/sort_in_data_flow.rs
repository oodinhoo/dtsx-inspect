//! Sort transformation: blocking, the whole dataset goes into memory. Microsoft,
//! *Data Flow Performance Features* ("Avoid Unnecessary Sorting"): sort in the source
//! query (ORDER BY) and declare the output sorted (IsSorted).

use super::{components, finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::Lineage;

pub struct SortInDataFlow;

const DOC: RuleDoc = RuleDoc {
    id: "sort-in-data-flow",
    severity: Severity::Warning,
    title: "Sort component in a Data Flow",
    why: "Sorting is blocking: it keeps every row in memory before letting one through, and spills to disk with large volumes.",
    fix: "Sort in the source query (ORDER BY), then declare the output sorted (IsSorted, SortKeyPosition) in the advanced editor of the source.",
};

impl Rule for SortInDataFlow {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        components(inventory)
            .filter(|(_, _, c)| c.class_id == "Microsoft.Sort")
            .map(|(p, location, _)| {
                finding(
                    &DOC,
                    &p.file,
                    location,
                    "blocking in-memory sort: sort in the source query (ORDER BY) and declare IsSorted",
                )
            })
            .collect()
    }
}
