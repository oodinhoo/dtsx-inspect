//! SSIS domain types, without logic.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Package,
    Project,
}

/// Connection manager. `target` never contains a secret
/// (see `parser::secrets`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionManager {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub scope: Scope,
    pub target: Option<String>,
}

impl ConnectionManager {
    /// Value of the first of `keys` present in the displayable string (`target`),
    /// case-insensitive: `setting(&["data source", "server"])`.
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

/// Raw reference to a connection (`{GUID}`, `{GUID}:external`,
/// `Project.ConnectionManagers[Name]`…) and the resolved name when known.
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
    /// `false` = third-party or unknown component (KingswaySoft…), kept with its `class_id`.
    pub known: bool,
    pub role: Role,
    pub connections: Vec<ConnectionRef>,
    /// Properties useful to the lineage (parser allow-list). Not in the JSON: only the
    /// details panel of the HTML report shows them, with passwords masked.
    #[serde(skip)]
    pub properties: BTreeMap<String, String>,
    /// Allow-listed properties set by a Data Flow expression
    /// (`[Component].[SqlCommand]`): their value is the design-time one.
    #[serde(skip)]
    pub expressions: Vec<String>,
    /// Columns read (inputs) and produced (outputs), for the column lineage.
    #[serde(skip)]
    pub inputs: Vec<Column>,
    #[serde(skip)]
    pub outputs: Vec<Column>,
}

/// Input or output column of a Data Flow component.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Column {
    /// Output: its identifier (`lineageId`). Input: the one of the upstream output
    /// column it reads.
    pub lineage: String,
    pub name: String,
    /// Table or file column (`…ExternalColumns[name]`): read by a source output,
    /// written by a destination input.
    pub external: Option<String>,
    /// Derived output: upstream columns it uses (expression, conversion).
    pub sources: Vec<String>,
    /// Lookup output: column copied from the reference table.
    pub reference: Option<String>,
    /// Union All input: output column it feeds.
    pub feeds: Option<String>,
}

/// Data Flow path between two components (`ref_id`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPath {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlOrigin {
    /// Typed in directly.
    Direct,
    /// Built by an expression: `text` is the value evaluated at design time.
    Expression,
    /// Read from a variable: `text` is the variable name.
    Variable,
    /// Read from a file: `text` is the file connection.
    File,
}

/// Query of an Execute SQL Task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlStatement {
    pub text: String,
    pub origin: SqlOrigin,
}

/// Task or container. The hierarchy is read from `ref_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Executable {
    pub ref_id: String,
    pub name: String,
    pub executable_type: String,
    pub disabled: bool,
    pub connections: Vec<ConnectionRef>,
    /// Execute Package Task: called project package (`<PackageName>`).
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
    /// Readable path: `Package\SEQ\Task` -> `SEQ\Task`.
    pub fn path(&self) -> &str {
        self.ref_id
            .strip_prefix("Package\\")
            .or_else(|| self.ref_id.strip_prefix("Package."))
            .unwrap_or(&self.ref_id)
    }
}

/// Package or task variable. Design-time value, secrets masked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Variable {
    /// Path of the task or container holding it; empty for the package.
    pub scope: String,
    /// Qualified name: `User::Name`.
    pub name: String,
    pub value: Option<String>,
    /// Expression computing it (`EvaluateAsExpression`); `value` is then its
    /// design-time result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
}

/// Package parameter. Design-time value, secrets masked; no value for a sensitive
/// parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Parameter {
    pub name: String,
    pub value: Option<String>,
    pub sensitive: bool,
}

/// Property computed by an expression, secrets masked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PropertyExpression {
    /// Task path, `connection Name` or `package`.
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
