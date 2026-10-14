//! Règles de lint (anti-patterns SSIS) : une règle = un fichier, trait `Rule`.

mod connexion_non_resolue;
mod fournisseur_obsolete;
mod insert_select_etoile;
mod lignage_incomplet;
mod package_non_renomme;
mod tache_desactivee;

use std::fmt;

use crate::inventory::Inventory;
use connexion_non_resolue::ConnexionNonResolue;
use fournisseur_obsolete::FournisseurObsolete;
use insert_select_etoile::InsertSelectEtoile;
use lignage_incomplet::LignageIncomplet;
use package_non_renomme::PackageNonRenomme;
use tache_desactivee::TacheDesactivee;

// ponytail: toutes les règles toujours actives ; un `--rules regles.toml`
// (activation, gravité) viendra quand une règle gênera quelqu'un.
const RULES: [&dyn Rule; 6] = [
    &ConnexionNonResolue,
    &FournisseurObsolete,
    &InsertSelectEtoile,
    &LignageIncomplet,
    &PackageNonRenomme,
    &TacheDesactivee,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Erreur,
    Avertissement,
    Info,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Severity::Erreur => "erreur",
            Severity::Avertissement => "avertissement",
            Severity::Info => "info",
        })
    }
}

/// Constat d'une règle. Ne contient jamais de SQL ni de chaîne de connexion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub rule: &'static str,
    pub severity: Severity,
    /// Fichier du package, ou `projet` pour une connexion de projet.
    pub file: String,
    /// Tâche, composant ou connexion concernés.
    pub location: String,
    pub message: String,
}

pub trait Rule {
    fn check(&self, inventory: &Inventory) -> Vec<Finding>;
}

/// Toutes les règles, triées par fichier puis emplacement.
pub fn lint(inventory: &Inventory) -> Vec<Finding> {
    let mut findings: Vec<Finding> = RULES.iter().flat_map(|r| r.check(inventory)).collect();
    findings.sort_by(|a, b| (&a.file, &a.location, a.rule).cmp(&(&b.file, &b.location, b.rule)));
    findings
}
