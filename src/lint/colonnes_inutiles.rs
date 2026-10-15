//! Colonnes inutiles dans le flux : source OLE DB en mode « Table ou vue », ou
//! requête `SELECT *` (source, Lookup). Microsoft, *Data Flow Performance
//! Features* : réviser la requête pour n'inclure que les colonnes utilisées ;
//! un SELECT est plus performant que le mode « Table ou vue ».

use super::{components, finding, Finding, Rule, Severity};
use crate::inventory::Inventory;
use crate::lineage::sql::query_selects_star;
use crate::lineage::{access_mode, sql_command};

pub struct ColonnesInutiles;

impl Rule for ColonnesInutiles {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        components(inventory)
            .filter_map(|(p, location, c)| {
                let message = if c.class_id == "Microsoft.OLEDBSource" && access_mode(c) == 0 {
                    "source en mode table ou vue : écrire un SELECT des seules colonnes utiles"
                } else if sql_command(c).is_some_and(query_selects_star) {
                    "SELECT * : lister les seules colonnes utiles"
                } else {
                    return None;
                };
                Some(finding(
                    "colonnes-inutiles",
                    Severity::Avertissement,
                    &p.file,
                    location,
                    message,
                ))
            })
            .collect()
    }
}
