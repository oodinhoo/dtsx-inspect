//! Types du domaine SSIS, sans logique.

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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Package {
    pub file: String,
    pub name: String,
    pub format_version: u32,
    pub connections: Vec<ConnectionManager>,
    pub executables: Vec<Executable>,
}
