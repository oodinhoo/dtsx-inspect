//! Tâche ou composant resté sous le nom donné par le concepteur SSDT : illisible
//! dans les journaux et le lignage. Convention de nommage de Jamie Thomson
//! (préfixes DFT, SQL, SEQC, OLE_SRC, LKP…), reprise par la communauté SSIS.

use super::{components, finding, tasks, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;

pub struct NomParDefaut;

const DOC: RuleDoc = RuleDoc {
    id: "nom-par-defaut",
    severity: Severity::Info,
    title: "Tâche ou composant resté sous le nom donné par le concepteur",
    why: "« Data Flow Task 1 », « Derived Column »… : ces noms ne disent rien dans les journaux d'exécution, les messages d'erreur ni ce rapport.",
    fix: "Renommer selon la convention de l'équipe : un préfixe de type (DFT_, SQL_, LKP_, DER_…) suivi de ce que fait l'étape.",
};

/// Noms par défaut de SSDT, en anglais et en français, sans le numéro ajouté aux
/// suivants (`Multidiffusion 1`).
// ponytail: liste des tâches et composants courants ; compléter si un autre nom
// par défaut apparaît dans les packages.
const DEFAULT_NAMES: [&str; 47] = [
    "Data Flow Task",
    "Execute SQL Task",
    "Script Task",
    "Execute Package Task",
    "File System Task",
    "Send Mail Task",
    "Expression Task",
    "Sequence Container",
    "Foreach Loop Container",
    "For Loop Container",
    "Tâche de flux de données",
    "Tâche d'exécution de requêtes SQL",
    "Tâche de script",
    "Tâche d'exécution de package",
    "Tâche de système de fichiers",
    "Conteneur de séquences",
    "Conteneur de boucles Foreach",
    "OLE DB Source",
    "OLE DB Destination",
    "OLE DB Command",
    "Flat File Source",
    "Flat File Destination",
    "ADO NET Source",
    "ADO NET Destination",
    "Excel Source",
    "Excel Destination",
    "Derived Column",
    "Data Conversion",
    "Conditional Split",
    "Lookup",
    "Multicast",
    "Union All",
    "Row Count",
    "Sort",
    "Merge Join",
    "Script Component",
    "Source OLE DB",
    "Destination OLE DB",
    "Commande OLE DB",
    "Colonne dérivée",
    "Conversion de données",
    "Fractionnement conditionnel",
    "Recherche",
    "Multidiffusion",
    "Unir tout",
    "Nombre de lignes",
    "Trier",
];

fn is_default(name: &str) -> bool {
    let base = name
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end();
    DEFAULT_NAMES.iter().any(|d| d.eq_ignore_ascii_case(base))
}

const MESSAGE: &str =
    "nom par défaut du concepteur : le renommer selon la convention (DFT_, SQL_, LKP_…)";

impl Rule for NomParDefaut {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        let tasks = tasks(inventory)
            .filter(|(_, e)| is_default(&e.name))
            .map(|(p, e)| finding(&DOC, &p.file, e.path().to_owned(), MESSAGE));
        let parts = components(inventory)
            .filter(|(_, _, c)| is_default(&c.name))
            .map(|(p, location, _)| finding(&DOC, &p.file, location, MESSAGE));
        tasks.chain(parts).collect()
    }
}
