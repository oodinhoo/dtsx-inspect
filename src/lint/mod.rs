//! Règles de lint (anti-patterns SSIS) : une règle = un fichier, trait `Rule`.

mod chargement_sans_fast_load;
mod colonnes_inutiles;
mod commande_ligne_a_ligne;
mod connexion_non_resolue;
mod fournisseur_obsolete;
mod insert_select_etoile;
mod lignage_incomplet;
mod nom_par_defaut;
mod package_non_renomme;
mod tache_desactivee;
mod tri_dans_le_flux;

use std::fmt;

use crate::inventory::Inventory;
use crate::model::{Component, Executable, Package};
use chargement_sans_fast_load::ChargementSansFastLoad;
use colonnes_inutiles::ColonnesInutiles;
use commande_ligne_a_ligne::CommandeLigneALigne;
use connexion_non_resolue::ConnexionNonResolue;
use fournisseur_obsolete::FournisseurObsolete;
use insert_select_etoile::InsertSelectEtoile;
use lignage_incomplet::LignageIncomplet;
use nom_par_defaut::NomParDefaut;
use package_non_renomme::PackageNonRenomme;
use tache_desactivee::TacheDesactivee;
use tri_dans_le_flux::TriDansLeFlux;

// ponytail: toutes les règles toujours actives ; un `--rules regles.toml`
// (activation, gravité) viendra quand une règle gênera quelqu'un.
const RULES: [&dyn Rule; 11] = [
    &ChargementSansFastLoad,
    &ColonnesInutiles,
    &CommandeLigneALigne,
    &ConnexionNonResolue,
    &FournisseurObsolete,
    &InsertSelectEtoile,
    &LignageIncomplet,
    &NomParDefaut,
    &PackageNonRenomme,
    &TacheDesactivee,
    &TriDansLeFlux,
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

/// Fiche d'une règle, montrée dans la page « Règles de lint » du rapport. Ses
/// constats en reprennent l'identifiant et la gravité.
#[derive(Debug)]
pub struct RuleDoc {
    pub id: &'static str,
    pub severity: Severity,
    /// Ce que la règle signale.
    pub title: &'static str,
    /// Pourquoi c'est un problème.
    pub why: &'static str,
    /// Comment le corriger.
    pub fix: &'static str,
}

pub trait Rule {
    fn doc(&self) -> &'static RuleDoc;
    fn check(&self, inventory: &Inventory) -> Vec<Finding>;
}

/// Fiches de toutes les règles, dans l'ordre alphabétique de leur fichier.
pub fn rules() -> impl Iterator<Item = &'static RuleDoc> {
    RULES.iter().map(|r| r.doc())
}

/// Tâches et conteneurs de tous les packages.
fn tasks(inventory: &Inventory) -> impl Iterator<Item = (&Package, &Executable)> {
    inventory
        .packages
        .iter()
        .flat_map(|p| p.executables.iter().map(move |e| (p, e)))
}

/// Composants de Data Flow de tous les packages, avec leur emplacement
/// (`Data Flow\Composant`), pour les règles qui portent sur un composant.
fn components(inventory: &Inventory) -> impl Iterator<Item = (&Package, String, &Component)> {
    inventory.packages.iter().flat_map(|p| {
        p.executables.iter().flat_map(move |e| {
            e.components
                .iter()
                .map(move |c| (p, format!("{}\\{}", e.path(), c.name), c))
        })
    })
}

fn finding(doc: &RuleDoc, file: &str, location: String, message: impl Into<String>) -> Finding {
    Finding {
        rule: doc.id,
        severity: doc.severity,
        file: file.to_owned(),
        location,
        message: message.into(),
    }
}

/// Toutes les règles, triées par fichier puis emplacement.
pub fn lint(inventory: &Inventory) -> Vec<Finding> {
    let mut findings: Vec<Finding> = RULES.iter().flat_map(|r| r.check(inventory)).collect();
    findings.sort_by(|a, b| (&a.file, &a.location, a.rule).cmp(&(&b.file, &b.location, b.rule)));
    findings
}
