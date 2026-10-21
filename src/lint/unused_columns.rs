//! Unused columns in the flow: OLE DB source in "Table or view" mode, or `SELECT *`
//! query (source, Lookup). Microsoft, *Data Flow Performance Features*: revise the
//! query to include only the columns used; a SELECT performs better than the "Table
//! or view" mode.

use super::{components, finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::sql::query_selects_star;
use crate::lineage::Lineage;
use crate::lineage::{access_mode, sql_command};

pub struct UnusedColumns;

const DOC: RuleDoc = RuleDoc {
    id: "unused-columns",
    severity: Severity::Warning,
    title: "Source or lookup reading every column",
    why: "A source in table or view mode, or a SELECT * query, brings unused columns into the flow: more memory per row, fewer rows per buffer, and a package that breaks when the table changes.",
    fix: "Write a SELECT query listing only the columns used further down the flow.",
};

impl Rule for UnusedColumns {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        components(inventory)
            .filter_map(|(p, location, c)| {
                let message = if c.class_id == "Microsoft.OLEDBSource" && access_mode(c) == 0 {
                    "source in table or view mode: write a SELECT of the useful columns only"
                } else if sql_command(c).is_some_and(query_selects_star) {
                    "SELECT *: list the useful columns only"
                } else {
                    return None;
                };
                Some(finding(&DOC, &p.file, location, message))
            })
            .collect()
    }
}
