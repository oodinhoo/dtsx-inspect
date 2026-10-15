//! Tâche désactivée : code mort, ou oubli avant la mise en production.

use super::{finding, tasks, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;

pub struct TacheDesactivee;

const DOC: RuleDoc = RuleDoc {
    id: "tache-desactivee",
    severity: Severity::Info,
    title: "Tâche désactivée",
    why: "Code mort qui reste déployé, ou tâche qu'on a oublié de réactiver avant la mise en production.",
    fix: "Supprimer la tâche si elle ne sert plus ; sinon la réactiver, ou noter dans une annotation pourquoi elle est désactivée.",
};

impl Rule for TacheDesactivee {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        tasks(inventory)
            .filter(|(_, e)| e.disabled)
            .map(|(p, e)| {
                finding(
                    &DOC,
                    &p.file,
                    e.path().to_owned(),
                    "tâche désactivée, à supprimer si elle ne sert plus",
                )
            })
            .collect()
    }
}
