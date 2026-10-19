//! Colonnes inutiles dans le flux : source OLE DB en mode « Table ou vue », ou
//! requête `SELECT *` (source, Lookup). Microsoft, *Data Flow Performance
//! Features* : réviser la requête pour n'inclure que les colonnes utilisées ;
//! un SELECT est plus performant que le mode « Table ou vue ».

use super::{components, finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::sql::query_selects_star;
use crate::lineage::Lineage;
use crate::lineage::{access_mode, sql_command};

pub struct ColonnesInutiles;

const DOC: RuleDoc = RuleDoc {
    id: "colonnes-inutiles",
    severity: Severity::Avertissement,
    title: "Source ou recherche qui lit toutes les colonnes",
    why: "Une source en mode table ou vue, ou une requête SELECT *, fait passer dans le flux des colonnes inutiles : plus de mémoire par ligne, moins de lignes par tampon, et un package qui casse quand la table change.",
    fix: "Écrire une requête SELECT qui ne liste que les colonnes utilisées par la suite du flux.",
};

impl Rule for ColonnesInutiles {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        components(inventory)
            .filter_map(|(p, location, c)| {
                let message = if c.class_id == "Microsoft.OLEDBSource" && access_mode(c) == 0 {
                    "source en mode table ou vue : écrire un SELECT des seules colonnes utiles"
                } else if sql_command(c).is_some_and(query_selects_star) {
                    "SELECT * : lister les seules colonnes utiles"
                } else {
                    return None;
                };
                Some(finding(&DOC, &p.file, location, message))
            })
            .collect()
    }
}
