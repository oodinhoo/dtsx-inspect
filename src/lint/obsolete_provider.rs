//! Discontinued OLE DB provider: SQL Native Client (`SQLNCLI*`) and `SQLOLEDB`.

use std::collections::HashSet;

use super::{finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::Lineage;
use crate::model::{ConnectionManager, Scope};

pub struct ObsoleteProvider;

const DOC: RuleDoc = RuleDoc {
    id: "obsolete-provider",
    severity: Severity::Warning,
    title: "Discontinued OLE DB provider (SQLNCLI, SQLOLEDB)",
    why: "SQL Server Native Client and SQLOLEDB are no longer maintained by Microsoft: no security fixes, and no guaranteed support for recent TLS and SQL Server versions.",
    fix: "Switch the connection to the MSOLEDBSQL provider (Microsoft OLE DB Driver for SQL Server).",
};

const OBSOLETE: [&str; 2] = ["sqlncli", "sqloledb"];

impl Rule for ObsoleteProvider {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        // A project connection is copied into each package using it: reported once
        // per project, under the project folder.
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

/// Project folder of a package (`FINANCE/x.dtsx` -> `FINANCE`), `project` at the root.
fn project(file: &str) -> &str {
    file.rsplit_once('/')
        .map_or("project", |(folder, _)| folder)
}

fn obsolete(file: &str, c: &ConnectionManager) -> Option<Finding> {
    let provider = c.setting(&["provider"])?;
    let lower = provider.to_lowercase();
    OBSOLETE.iter().any(|o| lower.starts_with(o)).then(|| {
        finding(
            &DOC,
            file,
            format!("connection {}", c.name),
            format!("provider {provider} discontinued by Microsoft, switch to MSOLEDBSQL"),
        )
    })
}
