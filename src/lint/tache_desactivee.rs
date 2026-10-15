//! Tâche désactivée : code mort, ou oubli avant la mise en production.

use super::{finding, tasks, Finding, Rule, Severity};
use crate::inventory::Inventory;

pub struct TacheDesactivee;

impl Rule for TacheDesactivee {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        tasks(inventory)
            .filter(|(_, e)| e.disabled)
            .map(|(p, e)| {
                finding(
                    "tache-desactivee",
                    Severity::Info,
                    &p.file,
                    e.path().to_owned(),
                    "tâche désactivée, à supprimer si elle ne sert plus",
                )
            })
            .collect()
    }
}
