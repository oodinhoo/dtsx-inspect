//! Fournisseur OLE DB abandonné : SQL Native Client (`SQLNCLI*`) et `SQLOLEDB`.

use super::{finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
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
