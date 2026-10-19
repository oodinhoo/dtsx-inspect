//! Fournisseur OLE DB abandonné : SQL Native Client (`SQLNCLI*`) et `SQLOLEDB`.

use std::collections::HashSet;

use super::{finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::Lineage;
use crate::model::{ConnectionManager, Scope};

pub struct FournisseurObsolete;

const DOC: RuleDoc = RuleDoc {
    id: "fournisseur-obsolete",
    severity: Severity::Avertissement,
    title: "Fournisseur OLE DB abandonné (SQLNCLI, SQLOLEDB)",
    why: "SQL Server Native Client et SQLOLEDB ne sont plus maintenus par Microsoft : ni correctifs de sécurité, ni prise en charge garantie des versions récentes de TLS et de SQL Server.",
    fix: "Passer la connexion au fournisseur MSOLEDBSQL (Microsoft OLE DB Driver for SQL Server).",
};

const OBSOLETE: [&str; 2] = ["sqlncli", "sqloledb"];

impl Rule for FournisseurObsolete {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        // Une connexion de projet est recopiée dans chaque package qui l'utilise :
        // signalée une fois par projet, sous le dossier du projet.
        let mut seen = HashSet::new();
        inventory
            .packages
            .iter()
            .flat_map(|p| p.connections.iter().map(move |c| (p, c)))
            .filter_map(|(p, c)| {
                let file = match c.scope {
                    Scope::Package => p.file.as_str(),
                    Scope::Project => project(&p.file),
                };
                let first = c.scope == Scope::Package || seen.insert((file, c.id.as_str()));
                first.then(|| obsolete(file, c)).flatten()
            })
            .collect()
    }
}

/// Dossier du projet d'un package (`FINANCE/x.dtsx` -> `FINANCE`), `projet` à la racine.
fn project(file: &str) -> &str {
    file.rsplit_once('/').map_or("projet", |(folder, _)| folder)
}

fn obsolete(file: &str, c: &ConnectionManager) -> Option<Finding> {
    let provider = c.setting(&["provider"])?;
    let lower = provider.to_lowercase();
    OBSOLETE.iter().any(|o| lower.starts_with(o)).then(|| {
        finding(
            &DOC,
            file,
            format!("connexion {}", c.name),
            format!("fournisseur {provider} abandonné par Microsoft, passer à MSOLEDBSQL"),
        )
    })
}
