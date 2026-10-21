//! Connection reference not found (connection deleted or renamed), or marked
//! `{GUID}:invalid` by SSDT: the connection was missing when the package was opened
//! and the reference has not been reassigned since.

use super::{components, finding, tasks, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::Lineage;
use crate::model::ConnectionRef;

pub struct UnresolvedConnection;

const DOC: RuleDoc = RuleDoc {
    id: "unresolved-connection",
    severity: Severity::Error,
    title: "Connection reference not found or marked invalid",
    why: "The task or component points to a deleted or renamed connection, or one SSDT marked \":invalid\" because it could not find it on opening: the package fails at validation or at run time.",
    fix: "Open the package in SSDT and reassign the connection of the task or component, or recreate the missing connection manager.",
};

impl Rule for UnresolvedConnection {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        let tasks = tasks(inventory).map(|(p, e)| (p, e.path().to_owned(), &e.connections));
        let parts = components(inventory).map(|(p, location, c)| (p, location, &c.connections));
        tasks
            .chain(parts)
            .flat_map(|(p, location, refs)| {
                refs.iter().filter_map(move |r| {
                    let message = message(r)?;
                    Some(finding(&DOC, &p.file, location.clone(), message))
                })
            })
            .collect()
    }
}

fn message(r: &ConnectionRef) -> Option<String> {
    match &r.resolved {
        None => Some(format!("connection {} not found", r.reference)),
        Some(name) if r.reference.ends_with(":invalid") => Some(format!(
            "reference to \"{name}\" marked invalid: reassign it in SSDT"
        )),
        Some(_) => None,
    }
}
