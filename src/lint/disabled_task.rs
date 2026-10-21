//! Disabled task: dead code, or forgotten before going to production.

use super::{finding, tasks, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::Lineage;

pub struct DisabledTask;

const DOC: RuleDoc = RuleDoc {
    id: "disabled-task",
    severity: Severity::Info,
    title: "Disabled task",
    why: "Dead code that stays deployed, or a task someone forgot to enable again before going to production.",
    fix: "Delete the task if it is no longer used; otherwise enable it again, or note in an annotation why it is disabled.",
};

impl Rule for DisabledTask {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        tasks(inventory)
            .filter(|(_, e)| e.disabled)
            .map(|(p, e)| {
                finding(
                    &DOC,
                    &p.file,
                    e.path().to_owned(),
                    "disabled task, delete it if it is no longer used",
                )
            })
            .collect()
    }
}
