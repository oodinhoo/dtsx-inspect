//! Destination OLE DB sans chargement rapide (`AccessMode` 0 ou 1) : une
//! insertion par ligne au lieu d'un chargement en masse. Le mode « Table ou vue -
//! chargement rapide » (`AccessMode` 3 ou 4) est l'usage recommandé.

use super::{components, finding, Finding, Rule, Severity};
use crate::inventory::Inventory;
use crate::lineage::access_mode;

pub struct ChargementSansFastLoad;

impl Rule for ChargementSansFastLoad {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        components(inventory)
            .filter(|(_, _, c)| {
                c.class_id == "Microsoft.OLEDBDestination" && matches!(access_mode(c), 0 | 1)
            })
            .map(|(p, location, _)| {
                finding(
                    "chargement-sans-fast-load",
                    Severity::Avertissement,
                    &p.file,
                    location,
                    "insertion ligne à ligne : choisir « Table ou vue - chargement rapide »",
                )
            })
            .collect()
    }
}
