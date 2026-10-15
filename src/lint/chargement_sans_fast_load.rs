//! Destination OLE DB sans chargement rapide (`AccessMode` 0 ou 1) : une
//! insertion par ligne au lieu d'un chargement en masse. Le mode « Table ou vue -
//! chargement rapide » (`AccessMode` 3 ou 4) est l'usage recommandé.

use super::{components, finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::access_mode;

pub struct ChargementSansFastLoad;

const DOC: RuleDoc = RuleDoc {
    id: "chargement-sans-fast-load",
    severity: Severity::Avertissement,
    title: "Destination OLE DB en insertion ligne à ligne",
    why: "En mode « Table ou vue » ou « Table ou vue - variable », la destination envoie un INSERT par ligne : le chargement est des dizaines de fois plus lent et charge le journal de transactions.",
    fix: "Choisir « Table ou vue - chargement rapide » (fast load), puis régler la taille des lots (Rows per batch, Maximum insert commit size).",
};

impl Rule for ChargementSansFastLoad {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        components(inventory)
            .filter(|(_, _, c)| {
                c.class_id == "Microsoft.OLEDBDestination" && matches!(access_mode(c), 0 | 1)
            })
            .map(|(p, location, _)| {
                finding(
                    &DOC,
                    &p.file,
                    location,
                    "insertion ligne à ligne : choisir « Table ou vue - chargement rapide »",
                )
            })
            .collect()
    }
}
