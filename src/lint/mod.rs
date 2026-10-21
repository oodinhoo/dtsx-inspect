//! Lint rules (SSIS anti-patterns): one rule = one file, `Rule` trait.

mod default_name;
mod disabled_task;
mod incomplete_lineage;
mod insert_select_star;
mod no_fast_load;
mod obsolete_provider;
mod row_by_row_command;
mod sort_in_data_flow;
mod unrenamed_package;
mod unresolved_connection;
mod unused_columns;

use std::fmt;

use crate::inventory::Inventory;
use crate::lineage::Lineage;
use crate::model::{Component, Executable, Package};
use default_name::DefaultName;
use disabled_task::DisabledTask;
use incomplete_lineage::IncompleteLineage;
use insert_select_star::InsertSelectStar;
use no_fast_load::NoFastLoad;
use obsolete_provider::ObsoleteProvider;
use row_by_row_command::RowByRowCommand;
use sort_in_data_flow::SortInDataFlow;
use unrenamed_package::UnrenamedPackage;
use unresolved_connection::UnresolvedConnection;
use unused_columns::UnusedColumns;

// shortcut: every rule always on; a `--rules rules.toml` (enabling, severity) will
// come when a rule bothers someone.
const RULES: [&dyn Rule; 11] = [
    &DefaultName,
    &DisabledTask,
    &IncompleteLineage,
    &InsertSelectStar,
    &NoFastLoad,
    &ObsoleteProvider,
    &RowByRowCommand,
    &SortInDataFlow,
    &UnrenamedPackage,
    &UnresolvedConnection,
    &UnusedColumns,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        })
    }
}

/// Finding of a rule. Never contains SQL or a connection string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule: &'static str,
    pub severity: Severity,
    /// Package file, or `project` for a project connection.
    pub file: String,
    /// Task, component or connection concerned.
    pub location: String,
    pub message: String,
}

/// Card of a rule, shown on the "Lint rules" page of the report. Its findings take
/// its identifier and severity.
#[derive(Debug)]
pub struct RuleDoc {
    pub id: &'static str,
    pub severity: Severity,
    /// What the rule reports.
    pub title: &'static str,
    /// Why it is a problem.
    pub why: &'static str,
    /// How to fix it.
    pub fix: &'static str,
}

pub trait Rule {
    fn doc(&self) -> &'static RuleDoc;
    fn check(&self, inventory: &Inventory, lineage: &Lineage) -> Vec<Finding>;
}

/// Cards of all rules, in the alphabetical order of their file.
pub fn rules() -> impl Iterator<Item = &'static RuleDoc> {
    RULES.iter().map(|r| r.doc())
}

/// Tasks and containers of all packages.
fn tasks(inventory: &Inventory) -> impl Iterator<Item = (&Package, &Executable)> {
    inventory
        .packages
        .iter()
        .flat_map(|p| p.executables.iter().map(move |e| (p, e)))
}

/// Data Flow components of all packages, with their location (`Data Flow\Component`),
/// for the rules about a component.
fn components(inventory: &Inventory) -> impl Iterator<Item = (&Package, String, &Component)> {
    inventory.packages.iter().flat_map(|p| {
        p.executables.iter().flat_map(move |e| {
            e.components
                .iter()
                .map(move |c| (p, format!("{}\\{}", e.path(), c.name), c))
        })
    })
}

fn finding(doc: &RuleDoc, file: &str, location: String, message: impl Into<String>) -> Finding {
    Finding {
        rule: doc.id,
        severity: doc.severity,
        file: file.to_owned(),
        location,
        message: message.into(),
    }
}

/// All rules, sorted by file then location.
pub fn lint(inventory: &Inventory, lineage: &Lineage) -> Vec<Finding> {
    let mut findings: Vec<Finding> = RULES
        .iter()
        .flat_map(|r| r.check(inventory, lineage))
        .collect();
    findings.sort_by(|a, b| (&a.file, &a.location, a.rule).cmp(&(&b.file, &b.location, b.rule)));
    findings
}
