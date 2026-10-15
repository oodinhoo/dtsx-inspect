//! Fournisseur OLE DB abandonné : SQL Native Client (`SQLNCLI*`) et `SQLOLEDB`.

use super::{finding, Finding, Rule, Severity};
use crate::inventory::Inventory;
use crate::model::{ConnectionManager, Scope};

pub struct FournisseurObsolete;

const OBSOLETE: [&str; 2] = ["sqlncli", "sqloledb"];

impl Rule for FournisseurObsolete {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        let project = inventory.project_connections.iter().map(|c| ("projet", c));
        // Les connexions de projet utilisées sont recopiées dans chaque package :
        // on ne garde que les locales pour ne les signaler qu'une fois.
        let local = inventory.packages.iter().flat_map(|p| {
            p.connections
                .iter()
                .filter(|c| c.scope == Scope::Package)
                .map(move |c| (p.file.as_str(), c))
        });
        project
            .chain(local)
            .filter_map(|(file, c)| obsolete(file, c))
            .collect()
    }
}

fn obsolete(file: &str, c: &ConnectionManager) -> Option<Finding> {
    let provider = c.target.as_deref()?.split(';').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case("provider")
            .then(|| value.trim())
    })?;
    let lower = provider.to_lowercase();
    OBSOLETE.iter().any(|o| lower.starts_with(o)).then(|| {
        finding(
            "fournisseur-obsolete",
            Severity::Avertissement,
            file,
            format!("connexion {}", c.name),
            format!("fournisseur {provider} abandonné par Microsoft, passer à MSOLEDBSQL"),
        )
    })
}
