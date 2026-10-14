//! Tâche désactivée : code mort, ou oubli avant la mise en production.

use super::{Finding, Rule, Severity};
use crate::inventory::Inventory;

pub struct TacheDesactivee;

impl Rule for TacheDesactivee {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        inventory
            .packages
            .iter()
            .flat_map(|p| {
                p.executables
                    .iter()
                    .filter(|e| e.disabled)
                    .map(move |e| Finding {
                        rule: "tache-desactivee",
                        severity: Severity::Info,
                        file: p.file.clone(),
                        location: e.path().to_owned(),
                        message: "tâche désactivée, à supprimer si elle ne sert plus".to_owned(),
                    })
            })
            .collect()
    }
}
