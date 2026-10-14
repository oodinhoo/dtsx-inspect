//! Référence de connexion introuvable : connexion supprimée ou renommée
//! (SSIS la marque `{GUID}:invalid`), le package échouera à l'exécution.

use super::{Finding, Rule, Severity};
use crate::inventory::Inventory;

pub struct ConnexionNonResolue;

impl Rule for ConnexionNonResolue {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        let mut findings = Vec::new();
        for p in &inventory.packages {
            for e in &p.executables {
                let task = e.connections.iter().map(|r| (e.path().to_owned(), r));
                let components = e.components.iter().flat_map(|c| {
                    let location = format!("{}\\{}", e.path(), c.name);
                    c.connections.iter().map(move |r| (location.clone(), r))
                });
                let dead = task.chain(components).filter(|(_, r)| r.resolved.is_none());
                for (location, r) in dead {
                    findings.push(Finding {
                        rule: "connexion-non-resolue",
                        severity: Severity::Erreur,
                        file: p.file.clone(),
                        location,
                        message: format!("connexion {} introuvable", r.reference),
                    });
                }
            }
        }
        findings
    }
}
