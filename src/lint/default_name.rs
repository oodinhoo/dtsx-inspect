//! Task or component still named as the SSDT designer named it: unreadable in logs
//! and in the lineage. Naming convention by Jamie Thomson (DFT, SQL, SEQC, OLE_SRC,
//! LKP… prefixes), adopted by the SSIS community.

use super::{components, finding, tasks, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::Lineage;

pub struct DefaultName;

const DOC: RuleDoc = RuleDoc {
    id: "default-name",
    severity: Severity::Info,
    title: "Task or component still named as the designer named it",
    why: "\"Data Flow Task 1\", \"Derived Column\"…: these names say nothing in the execution logs, the error messages or this report.",
    fix: "Rename following the team convention: a type prefix (DFT_, SQL_, LKP_, DER_…) followed by what the step does.",
};

/// SSDT default names, in English and French, without the number added to the next
/// ones (`Multicast 1`).
// shortcut: list of the common tasks and components; extend it if another default
// name shows up in packages.
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
    "designer default name: rename it following the convention (DFT_, SQL_, LKP_…)";

impl Rule for DefaultName {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory, _lineage: &Lineage) -> Vec<Finding> {
        let tasks = tasks(inventory)
            .filter(|(_, e)| is_default(&e.name))
            .map(|(p, e)| finding(&DOC, &p.file, e.path().to_owned(), MESSAGE));
        let parts = components(inventory)
            .filter(|(_, _, c)| is_default(&c.name))
            .map(|(p, location, _)| finding(&DOC, &p.file, location, MESSAGE));
        tasks.chain(parts).collect()
    }
}
