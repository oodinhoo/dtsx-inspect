//! Inventory outputs.

mod columns;
mod diff;
mod graph;
mod html;
mod mapping;
mod markdown;
mod mermaid;
mod order;

pub use columns::column_trace;
pub use diff::mapping_diff;
pub use html::html;
pub use mapping::mapping_csv;
pub use markdown::markdown;
pub use mermaid::{calls_mermaid, mermaid};
pub use order::order_markdown;

use crate::inventory::Inventory;
use crate::lint::{Finding, Severity};

/// An output line must not be able to open another one.
fn clean(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// `1 task`, `0 tasks`, `2 tasks`.
fn plural(count: usize, word: &str) -> String {
    let s = if count == 1 { "" } else { "s" };
    format!("{count} {word}{s}")
}

/// `Project / Package`, or the package alone at the root of the analyzed folder.
fn qualified(project: &str, package: &str) -> String {
    if project.is_empty() {
        package.to_owned()
    } else {
        format!("{project} / {package}")
    }
}

pub fn json(inventory: &Inventory) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(inventory)
}

/// One finding per line (`severity[rule] file: location: message`), then the total.
pub fn findings(findings: &[Finding]) -> String {
    let count = |severity| findings.iter().filter(|f| f.severity == severity).count();
    let lines: String = findings
        .iter()
        .map(|f| {
            let line = format!(
                "{}[{}] {}: {}: {}",
                f.severity, f.rule, f.file, f.location, f.message
            );
            clean(&line) + "\n"
        })
        .collect();
    format!(
        "{lines}{} error(s), {} warning(s), {} info(s)\n",
        count(Severity::Error),
        count(Severity::Warning),
        count(Severity::Info)
    )
}
