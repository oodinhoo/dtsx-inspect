//! Ce que le lignage n'a pas pu suivre (SQL non analysé,
//! requête en variable…) : autant d'angles morts dans la documentation.

use super::{finding, Finding, Rule, RuleDoc, Severity};
use crate::inventory::Inventory;
use crate::lineage::lineage;

pub struct LignageIncomplet;

const DOC: RuleDoc = RuleDoc {
    id: "lignage-incomplet",
    severity: Severity::Info,
    title: "Étape que le lignage n'a pas pu suivre",
    why: "SQL non analysable, requête lue dans une variable ou un fichier, procédure stockée sans définition : la documentation du lignage a un angle mort à cet endroit.",
    fix: "Rien d'obligatoire. Pour combler le trou : ajouter au dossier analysé les scripts .sql des procédures, ou remplacer la requête en variable par une requête saisie directement.",
};

impl Rule for LignageIncomplet {
    fn doc(&self) -> &'static RuleDoc {
        &DOC
    }

    fn check(&self, inventory: &Inventory) -> Vec<Finding> {
        lineage(inventory)
            .notes
            .iter()
            .filter_map(|note| {
                let package = inventory.packages.get(note.package)?;
                Some(finding(
                    &DOC,
                    &package.file,
                    note.step.clone(),
                    note.reason.as_str(),
                ))
            })
            .collect()
    }
}
