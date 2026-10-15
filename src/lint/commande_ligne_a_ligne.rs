//! Commande OLE DB : une requête exécutée pour chaque ligne du flux. Microsoft
//! (*OLE DB Command Transformation*, *Data Flow Performance Features*) :
//! écrire les lignes dans une table de transit puis faire un UPDATE ensembliste.

use super::{components, finding, Finding, Rule, Severity};
use crate::inventory::Inventory;

pub struct CommandeLigneALigne;

impl Rule for CommandeLigneALigne {
    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        components(inventory)
            .filter(|(_, _, c)| c.class_id == "Microsoft.OLEDBCommand")
            .map(|(p, location, _)| {
                finding(
                    "commande-ligne-a-ligne",
                    Severity::Avertissement,
                    &p.file,
                    location,
                    "une requête par ligne : charger une table de transit puis une seule requête ensembliste",
                )
            })
            .collect()
    }
}
