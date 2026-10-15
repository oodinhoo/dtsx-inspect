//! Transformation Tri : bloquante, tout le jeu de données passe en mémoire.
//! Microsoft, *Data Flow Performance Features* (« Avoid Unnecessary Sorting ») :
//! trier dans la requête source (ORDER BY) et déclarer la sortie triée (IsSorted).

use super::{components, finding, Finding, Rule, Severity};
use crate::inventory::Inventory;

pub struct TriDansLeFlux;

impl Rule for TriDansLeFlux {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        components(inventory)
            .filter(|(_, _, c)| c.class_id == "Microsoft.Sort")
            .map(|(p, location, _)| {
                finding(
                    "tri-dans-le-flux",
                    Severity::Avertissement,
                    &p.file,
                    location,
                    "tri bloquant en mémoire : trier dans la requête source (ORDER BY) et déclarer IsSorted",
                )
            })
            .collect()
    }
}
