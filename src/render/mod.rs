//! Sorties de l'inventaire.

mod markdown;
mod mermaid;

pub use markdown::markdown;
pub use mermaid::mermaid;

use crate::inventory::Inventory;
use crate::lint::{Finding, Severity};

pub fn json(inventory: &Inventory) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(inventory)
}

/// Un constat par ligne (`gravité[règle] fichier : emplacement : message`), puis le total.
pub fn findings(findings: &[Finding]) -> String {
    let count = |severity| findings.iter().filter(|f| f.severity == severity).count();
    let lines: String = findings
        .iter()
        .map(|f| {
            let line = format!(
                "{}[{}] {} : {} : {}",
                f.severity, f.rule, f.file, f.location, f.message
            );
            line.replace(['\r', '\n'], " ") + "\n"
        })
        .collect();
    format!(
        "{lines}{} erreur(s), {} avertissement(s), {} info(s)\n",
        count(Severity::Erreur),
        count(Severity::Avertissement),
        count(Severity::Info)
    )
}
