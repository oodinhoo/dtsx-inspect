//! Commande OLE DB : une requête exécutée pour chaque ligne du flux. Microsoft
//! (*OLE DB Command Transformation*, *Data Flow Performance Features*) :
//! écrire les lignes dans une table de transit puis faire un UPDATE ensembliste.

use super::{components, finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;

pub struct CommandeLigneALigne;

const DOC: RuleDoc = RuleDoc {
    id: "commande-ligne-a-ligne",
    severity: Severity::Avertissement,
    title: "OLE DB Command dans un Data Flow",
    why: "Le composant exécute sa requête une fois par ligne : autant d'allers-retours avec le serveur que de lignes, ce qui devient très lent dès quelques milliers de lignes.",
    fix: "Charger les lignes dans une table de transit, puis appliquer une seule requête ensembliste (UPDATE … FROM, MERGE) dans une tâche SQL après le Data Flow.",
};

impl Rule for CommandeLigneALigne {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        components(inventory)
            .filter(|(_, _, c)| c.class_id == "Microsoft.OLEDBCommand")
            .map(|(p, location, _)| {
                finding(&DOC, &p.file,
                    location,
                    "une requête par ligne : charger une table de transit puis une seule requête ensembliste",
                )
            })
            .collect()
    }
}
