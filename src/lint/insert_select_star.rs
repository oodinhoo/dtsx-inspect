//! `INSERT … SELECT *`: columns match by position, a column added or reordered on one
//! side silently breaks the load.

use super::{finding, tasks, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::sql::inserts_select_star;
use crate::lineage::Lineage;
use crate::model::SqlOrigin;

pub struct InsertSelectStar;

const DOC: RuleDoc = RuleDoc {
    id: "insert-select-star",
    severity: Severity::Warning,
    title: "INSERT … SELECT * in an SQL task",
    why: "Columns are matched by their position: a column added or reordered on one side shifts the data or makes the load fail without a clear message.",
    fix: "List the columns on both sides: INSERT INTO target (a, b) SELECT a, b FROM source.",
};

impl Rule for InsertSelectStar {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        tasks(inventory)
            .filter(|(_, e)| {
                e.sql.as_ref().is_some_and(|sql| {
                    matches!(sql.origin, SqlOrigin::Direct | SqlOrigin::Expression)
                        && inserts_select_star(&sql.text)
                })
            })
            .map(|(p, e)| {
                finding(
                    &DOC,
                    &p.file,
                    e.path().to_owned(),
                    "INSERT … SELECT *: list the columns on both sides",
                )
            })
            .collect()
    }
}
