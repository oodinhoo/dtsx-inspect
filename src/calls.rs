//! Package calls: each Execute Package task and the package it runs.

use crate::inventory::Inventory;
use crate::model::{Executable, Package};

/// Call of a package by an Execute Package task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// Index of the calling package in `Inventory::packages`.
    pub caller: usize,
    /// Path of the task in the caller.
    pub task: String,
    pub callee: Callee,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callee {
    /// Index of the called package in `Inventory::packages`.
    Package(usize),
    /// Not in the analyzed folder: expected file name, or no target set.
    Missing(String),
}

/// Calls of all packages, in package order then task order.
pub fn calls(inventory: &Inventory) -> Vec<Call> {
    inventory
        .packages
        .iter()
        .enumerate()
        .flat_map(|(caller, p)| {
            p.executables
                .iter()
                .filter(|e| e.executable_type.contains("ExecutePackageTask"))
                .map(move |e| Call {
                    caller,
                    task: e.path().to_owned(),
                    callee: callee(inventory, p, e),
                })
        })
        .collect()
}

fn callee(inventory: &Inventory, caller: &Package, task: &Executable) -> Callee {
    // Project reference: a package of the same project, hence of the same folder.
    if let Some(name) = &task.called_package {
        let wanted = file_name(name);
        let home = folder(&caller.file);
        return find(inventory, |p| {
            folder(&p.file) == home && file_name(&p.file).eq_ignore_ascii_case(wanted)
        })
        .map_or_else(|| Callee::Missing(wanted.to_owned()), Callee::Package);
    }
    // External reference: the file of a FILE connection, found by its name.
    // shortcut: two same-named packages in two projects: the first one wins.
    let target = task
        .connections
        .first()
        .and_then(|r| r.resolved.as_deref())
        .and_then(|name| caller.connections.iter().find(|m| m.name == name))
        .and_then(|m| m.target.as_deref());
    let Some(path) = target else {
        return Callee::Missing("no target set".to_owned());
    };
    let wanted = file_name(path);
    find(inventory, |p| {
        file_name(&p.file).eq_ignore_ascii_case(wanted)
    })
    .map_or_else(|| Callee::Missing(wanted.to_owned()), Callee::Package)
}

fn find(inventory: &Inventory, matches: impl Fn(&Package) -> bool) -> Option<usize> {
    inventory.packages.iter().position(matches)
}

/// Folder of a displayed path (`Project/x.dtsx` -> `Project`), empty at the root.
fn folder(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// File name of a Windows or displayed path (`D:\a\x.dtsx`, `Project/x.dtsx`).
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path).trim()
}
