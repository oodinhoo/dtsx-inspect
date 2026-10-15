//! Appels de packages : chaque tâche Execute Package et le package qu'elle lance.

use crate::inventory::Inventory;
use crate::model::{Executable, Package};

/// Appel d'un package par une tâche Execute Package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// Index du package appelant dans `Inventory::packages`.
    pub caller: usize,
    /// Chemin de la tâche dans l'appelant.
    pub task: String,
    pub callee: Callee,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callee {
    /// Index du package appelé dans `Inventory::packages`.
    Package(usize),
    /// Absent du dossier analysé : nom du fichier attendu, ou cible non renseignée.
    Missing(String),
}

/// Appels de tous les packages, dans l'ordre des packages puis de leurs tâches.
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
    // Référence de projet : un package du même projet, donc du même dossier.
    if let Some(name) = &task.called_package {
        let wanted = file_name(name);
        let home = folder(&caller.file);
        return find(inventory, |p| {
            folder(&p.file) == home && file_name(&p.file).eq_ignore_ascii_case(wanted)
        })
        .map_or_else(|| Callee::Missing(wanted.to_owned()), Callee::Package);
    }
    // Référence externe : le fichier d'une connexion FILE, retrouvé par son nom.
    // ponytail: deux packages de même nom dans deux projets : le premier l'emporte.
    let target = task
        .connections
        .first()
        .and_then(|r| r.resolved.as_deref())
        .and_then(|name| caller.connections.iter().find(|m| m.name == name))
        .and_then(|m| m.target.as_deref());
    let Some(path) = target else {
        return Callee::Missing("cible non renseignée".to_owned());
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

/// Dossier d'un chemin affiché (`Projet/x.dtsx` -> `Projet`), vide à la racine.
fn folder(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// Nom de fichier d'un chemin Windows ou affiché (`D:\a\x.dtsx`, `Projet/x.dtsx`).
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path).trim()
}
