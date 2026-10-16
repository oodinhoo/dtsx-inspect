//! Types du domaine SSIS, sans logique.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Package,
    Project,
}

/// Gestionnaire de connexion. `target` ne contient jamais de secret
/// (voir `parser::secrets`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionManager {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub scope: Scope,
    pub target: Option<String>,
}

impl ConnectionManager {
    /// Valeur de la première des `keys` présente dans la chaîne affichable (`target`),
    /// casse ignorée : `setting(&["data source", "server"])`.
    pub fn setting(&self, keys: &[&str]) -> Option<&str> {
        let target = self.target.as_deref()?;
        keys.iter().find_map(|key| {
            target.split(';').find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                k.trim().eq_ignore_ascii_case(key).then(|| v.trim())
            })
        })
    }
}

/// Référence brute vers une connexion (`{GUID}`, `{GUID}:external`,
/// `Project.ConnectionManagers[Nom]`…) et le nom résolu s'il est connu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionRef {
    pub reference: String,
    pub resolved: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Source,
    Transform,
    Destination,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Component {
    pub ref_id: String,
    pub name: String,
    pub class_id: String,
    /// `false` = composant tiers ou inconnu (KingswaySoft…), gardé avec son `class_id`.
    pub known: bool,
    pub role: Role,
    pub connections: Vec<ConnectionRef>,
    /// Propriétés utiles au lignage (liste blanche du parseur). Hors du JSON : seul
    /// le panneau de détails du rapport HTML les montre, mots de passe masqués.
    #[serde(skip)]
    pub properties: BTreeMap<String, String>,
    /// Propriétés de la liste blanche définies par une expression du Data Flow
    /// (`[Composant].[SqlCommand]`) : leur valeur est celle de la conception.
    #[serde(skip)]
    pub expressions: Vec<String>,
    /// Colonnes lues (entrées) et produites (sorties), pour le lignage par colonne.
    #[serde(skip)]
    pub inputs: Vec<Column>,
    #[serde(skip)]
    pub outputs: Vec<Column>,
}

/// Colonne d'entrée ou de sortie d'un composant de Data Flow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Column {
    /// Sortie : son identifiant (`lineageId`). Entrée : celui de la colonne de
    /// sortie amont qu'elle lit.
    pub lineage: String,
    pub name: String,
    /// Colonne de la table ou du fichier (`…ExternalColumns[nom]`) : lue par une
    /// sortie de source, écrite par une entrée de destination.
    pub external: Option<String>,
    /// Sortie calculée : colonnes amont citées (expression, conversion).
    pub sources: Vec<String>,
    /// Sortie de recherche : colonne copiée de la table de référence.
    pub reference: Option<String>,
    /// Entrée d'Unir tout : colonne de sortie qu'elle alimente.
    pub feeds: Option<String>,
}

/// Lien du Data Flow entre deux composants (`ref_id`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPath {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlOrigin {
    /// Saisie directe.
    Direct,
    /// Construite par une expression : `text` est la valeur évaluée à la conception.
    Expression,
    /// Lue dans une variable : `text` est le nom de la variable.
    Variable,
    /// Lue dans un fichier : `text` est la connexion du fichier.
    File,
}

/// Requête d'une Execute SQL Task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlStatement {
    pub text: String,
    pub origin: SqlOrigin,
}

/// Tâche ou conteneur. La hiérarchie se lit dans `ref_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Executable {
    pub ref_id: String,
    pub name: String,
    pub executable_type: String,
    pub disabled: bool,
    pub connections: Vec<ConnectionRef>,
    /// Execute Package Task : package du projet appelé (`<PackageName>`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub called_package: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<Component>,
    #[serde(skip)]
    pub paths: Vec<DataPath>,
    #[serde(skip)]
    pub sql: Option<SqlStatement>,
}

impl Executable {
    /// Chemin lisible : `Package\SEQ\Tâche` -> `SEQ\Tâche`.
    pub fn path(&self) -> &str {
        self.ref_id
            .strip_prefix("Package\\")
            .or_else(|| self.ref_id.strip_prefix("Package."))
            .unwrap_or(&self.ref_id)
    }
}

/// Variable du package ou d'une tâche. Valeur de conception, secrets masqués.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Variable {
    /// Chemin de la tâche ou du conteneur qui la porte ; vide pour le package.
    pub scope: String,
    /// Nom qualifié : `User::Nom`.
    pub name: String,
    pub value: Option<String>,
    /// Expression qui la calcule (`EvaluateAsExpression`) ; `value` en est alors le
    /// résultat à la conception.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
}

/// Paramètre du package. Valeur de conception, secrets masqués ; aucune valeur
/// pour un paramètre sensible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Parameter {
    pub name: String,
    pub value: Option<String>,
    pub sensitive: bool,
}

/// Propriété calculée par une expression, secrets masqués.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PropertyExpression {
    /// Chemin de la tâche, `connexion Nom` ou `package`.
    pub owner: String,
    pub property: String,
    pub expression: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Package {
    pub file: String,
    pub name: String,
    pub format_version: u32,
    pub connections: Vec<ConnectionManager>,
    pub executables: Vec<Executable>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<Parameter>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub expressions: Vec<PropertyExpression>,
}
