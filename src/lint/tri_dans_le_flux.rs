//! Transformation Tri : bloquante, tout le jeu de données passe en mémoire.
//! Microsoft, *Data Flow Performance Features* (« Avoid Unnecessary Sorting ») :
//! trier dans la requête source (ORDER BY) et déclarer la sortie triée (IsSorted).

use super::{components, finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;

pub struct TriDansLeFlux;

const DOC: RuleDoc = RuleDoc {
    id: "tri-dans-le-flux",
    severity: Severity::Avertissement,
    title: "Composant Tri dans un Data Flow",
    why: "Le tri est bloquant : il garde toutes les lignes en mémoire avant d'en laisser passer une, et déborde sur disque avec les gros volumes.",
    fix: "Trier dans la requête source (ORDER BY), puis déclarer la sortie triée (IsSorted, SortKeyPosition) dans l'éditeur avancé de la source.",
};

impl Rule for TriDansLeFlux {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        components(inventory)
            .filter(|(_, _, c)| c.class_id == "Microsoft.Sort")
            .map(|(p, location, _)| {
                finding(&DOC, &p.file,
                    location,
                    "tri bloquant en mémoire : trier dans la requête source (ORDER BY) et déclarer IsSorted",
                )
            })
            .collect()
    }
}
