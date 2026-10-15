//! Référence de connexion introuvable (connexion supprimée ou renommée), ou
//! marquée `{GUID}:invalid` par SSDT : la connexion manquait à l'ouverture du
//! package et la référence n'a pas été réassociée depuis.

use super::{components, finding, tasks, Finding, Rule, Severity};
use crate::inventory::Inventory;
use crate::model::ConnectionRef;

pub struct ConnexionNonResolue;

impl Rule for ConnexionNonResolue {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        let tasks = tasks(inventory).map(|(p, e)| (p, e.path().to_owned(), &e.connections));
        let parts = components(inventory).map(|(p, location, c)| (p, location, &c.connections));
        tasks
            .chain(parts)
            .flat_map(|(p, location, refs)| {
                refs.iter().filter_map(move |r| {
                    let message = message(r)?;
                    Some(finding(
                        "connexion-non-resolue",
                        Severity::Erreur,
                        &p.file,
                        location.clone(),
                        message,
                    ))
                })
            })
            .collect()
    }
}

fn message(r: &ConnectionRef) -> Option<String> {
    match &r.resolved {
        None => Some(format!("connexion {} introuvable", r.reference)),
        Some(name) if r.reference.ends_with(":invalid") => Some(format!(
            "référence vers « {name} » marquée invalide : la réassocier dans SSDT"
        )),
        Some(_) => None,
    }
}
