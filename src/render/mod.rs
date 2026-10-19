//! Sorties de l'inventaire.

mod columns;
mod graph;
mod html;
mod mapping;
mod markdown;
mod mermaid;

pub use columns::column_trace;
pub use html::html;
pub use mapping::mapping_csv;
pub use markdown::markdown;
pub use mermaid::{calls_mermaid, mermaid};

use crate::inventory::Inventory;
use crate::lint::{Finding, Severity};

/// Une ligne de sortie ne doit pas pouvoir en ouvrir une autre.
fn clean(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// `1 tâche`, `2 tâches` (en français, 0 est au singulier).
fn plural(count: usize, word: &str) -> String {
    let s = if count > 1 { "s" } else { "" };
    format!("{count} {word}{s}")
}

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
            clean(&line) + "\n"
        })
        .collect();
    format!(
        "{lines}{} erreur(s), {} avertissement(s), {} info(s)\n",
        count(Severity::Erreur),
        count(Severity::Avertissement),
        count(Severity::Info)
    )
}
