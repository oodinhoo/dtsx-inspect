//! XML SSIS -> modèle. Aucune logique métier ici.

pub mod secrets;

use std::collections::BTreeMap;

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use crate::model::{
    Component, ConnectionManager, ConnectionRef, DataPath, Executable, Package, Role, Scope,
    SqlOrigin, SqlStatement,
};
use secrets::summarize_connection_string;

/// Premier format SSIS 2012 (attributs `DTS:` au lieu d'éléments `DTS:Property`).
const MIN_FORMAT_VERSION: u32 = 6;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("XML invalide : {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("attribut XML invalide : {0}")]
    Attribute(#[from] quick_xml::events::attributes::AttrError),
    #[error("format de package {0} non supporté (SSIS 2012 ou plus récent requis)")]
    UnsupportedFormat(u32),
    #[error("ce fichier n'est pas un package SSIS")]
    NotAPackage,
    #[error("ce fichier n'est pas un gestionnaire de connexion SSIS")]
    NotAConnectionManager,
}

/// Lit un `.dtsx`. `file` est laissé vide : c'est à l'appelant de le remplir.
pub fn parse_package(xml: &str) -> Result<Package, ParseError> {
    let doc = read(xml)?;
    let root = doc.root.ok_or(ParseError::NotAPackage)?;
    if root.element != "Executable" || !root.executable_type.contains("Package") {
        return Err(ParseError::NotAPackage);
    }
    let format_version = doc.format_version.ok_or(ParseError::NotAPackage)?;
    if format_version < MIN_FORMAT_VERSION {
        return Err(ParseError::UnsupportedFormat(format_version));
    }
    Ok(Package {
        file: String::new(),
        name: root.name,
        format_version,
        connections: doc.connections,
        executables: doc.executables,
    })
}

/// Lit un `.conmgr` (gestionnaire de connexion de projet).
pub fn parse_conmgr(xml: &str) -> Result<ConnectionManager, ParseError> {
    let doc = read(xml)?;
    match (doc.root, doc.connections.into_iter().next()) {
        (Some(root), Some(cm)) if root.element == "ConnectionManager" => Ok(ConnectionManager {
            scope: Scope::Project,
            ..cm
        }),
        _ => Err(ParseError::NotAConnectionManager),
    }
}

struct Root {
    element: String,
    executable_type: String,
    name: String,
}

#[derive(Default)]
struct Document {
    root: Option<Root>,
    format_version: Option<u32>,
    connections: Vec<ConnectionManager>,
    executables: Vec<Executable>,
}

/// `DTS:ConnectionManager` en cours ; `depth` compte l'élément imbriqué
/// (`ObjectData/DTS:ConnectionManager`) qui porte la chaîne de connexion.
struct ConnectionDraft {
    manager: ConnectionManager,
    depth: usize,
}

/// Propriétés de composant utiles au lignage ; les autres ne sont pas lues.
const COMPONENT_PROPERTIES: [&str; 8] = [
    "AccessMode",
    "FetchXML",
    "OpenRowset",
    "SourceEntity",
    "SourceType",
    "SqlCommand",
    "TableOrViewName",
    "UserComponentTypeName",
];

/// Hôte des composants .NET (ADO.NET, Script, KingswaySoft…) : la vraie classe
/// est dans la propriété `UserComponentTypeName`.
const MANAGED_HOST: &str = "Microsoft.ManagedComponentHost";

struct ComponentDraft {
    ref_id: String,
    name: String,
    class_id: String,
    connections: Vec<ConnectionRef>,
    properties: BTreeMap<String, String>,
    has_input: bool,
    has_output: bool,
    /// Profondeur sous `<component>` : 2 = `<properties><property>` du composant
    /// (les propriétés de colonnes sont plus profondes).
    depth: usize,
}

impl ComponentDraft {
    /// Rôle déduit de la structure : marche aussi pour les composants tiers.
    fn finish(self) -> Component {
        let role = match (self.has_input, self.has_output) {
            (false, _) => Role::Source,
            (true, false) => Role::Destination,
            (true, true) => Role::Transform,
        };
        let class_id = match self.properties.get("UserComponentTypeName") {
            Some(user) if self.class_id == MANAGED_HOST => {
                user.split(',').next().unwrap_or(user).trim().to_owned()
            }
            _ => self.class_id,
        };
        Component {
            ref_id: self.ref_id,
            name: self.name,
            // ponytail: « connu » = composant Microsoft ; une liste par classe si un tiers doit compter.
            known: class_id.starts_with("Microsoft."),
            class_id,
            role,
            connections: self.connections,
            properties: self.properties,
            expressions: Vec::new(),
        }
    }
}

fn read(xml: &str) -> Result<Document, ParseError> {
    let mut reader = Reader::from_str(xml.trim_start_matches('\u{feff}'));
    reader.config_mut().expand_empty_elements = true;
    let mut state = Reading::default();
    loop {
        match reader.read_event()? {
            Event::Start(e) => state.start(&e)?,
            Event::End(e) => state.end(e.local_name().as_ref()),
            Event::Text(t) if state.capture.is_some() => {
                state.captured.push_str(&t.xml10_content())
            }
            Event::GeneralRef(r) if state.capture.is_some() => {
                state.captured.push_str(&entity(&r)?)
            }
            Event::Eof => return Ok(state.into_document()),
            _ => {}
        }
    }
}

/// `&amp;` -> `&`, `&#xE9;` -> `é` ; une entité inconnue reste telle quelle.
fn entity(r: &BytesRef) -> Result<String, ParseError> {
    if let Some(c) = r.resolve_char_ref()? {
        return Ok(c.to_string());
    }
    let name = r.xml10_content();
    Ok(resolve_predefined_entity(&name).map_or_else(|| format!("&{name};"), str::to_owned))
}

/// Élément dont on lit le texte. Le texte arrive en plusieurs morceaux
/// (`Text`, `GeneralRef`) dès qu'il contient une entité.
#[derive(Clone, Copy)]
enum Capture {
    FormatVersion,
    CalledPackage,
    PackageConnection,
    ComponentProperty(&'static str),
}

// ponytail: éléments repérés par nom local, sans résolution de namespace :
// les préfixes (`DTS:`, `SQLTask:`) sont fixes dans les packages SSIS.
#[derive(Default)]
struct Reading {
    doc: Document,
    open_executables: Vec<usize>,
    connection: Option<ConnectionDraft>,
    component: Option<ComponentDraft>,
    in_execute_package: bool,
    capture: Option<Capture>,
    captured: String,
    /// Propriétés définies par une expression : (tâche, `DTS:Name`), résolues à la
    /// fin car les composants d'un Data Flow sont lus après ses expressions.
    expressions: Vec<(usize, String)>,
}

impl Reading {
    fn start(&mut self, e: &BytesStart) -> Result<(), ParseError> {
        let name = e.local_name();
        let local = name.as_ref();
        if self.doc.root.is_none() {
            self.doc.root = Some(Root {
                element: local.to_owned(),
                executable_type: attr(e, "ExecutableType")?.unwrap_or_default(),
                name: attr(e, "ObjectName")?.unwrap_or_default(),
            });
            if local == "Executable" {
                return Ok(());
            }
        }
        if self.component.is_some() {
            return self.start_in_component(e, local);
        }
        match local {
            "Executable" => self.start_executable(e)?,
            "ConnectionManager" => self.start_connection(e)?,
            "Property"
                if self.open_executables.is_empty()
                    && attr(e, "Name")?.as_deref() == Some("PackageFormatVersion") =>
            {
                self.begin(Capture::FormatVersion)
            }
            "PropertyExpression" if self.connection.is_none() => {
                if let (Some(&i), Some(name)) = (self.open_executables.last(), attr(e, "Name")?) {
                    self.expressions.push((i, name));
                }
            }
            "ExecutePackageTask" => self.in_execute_package = true,
            "PackageName" if self.in_execute_package => self.begin(Capture::CalledPackage),
            // Référence externe : connexion FILE (ou OLE DB pour msdb) vers le package.
            "Connection" if self.in_execute_package => self.begin(Capture::PackageConnection),
            "SqlTaskData" => self.start_sql_task(e)?,
            "component" => self.component = Some(start_component(e)?),
            "path" => self.start_path(e)?,
            _ => {}
        }
        Ok(())
    }

    fn start_in_component(&mut self, e: &BytesStart, local: &str) -> Result<(), ParseError> {
        let Some(c) = self.component.as_mut() else {
            return Ok(());
        };
        c.depth += 1;
        match local {
            "connection" => {
                // Le GUID fait foi (c'est lui que SSIS résout) ; le nom de
                // `connectionManagerRefId` reste figé après un renommage.
                let reference = attr(e, "connectionManagerID")?
                    .filter(|r| !r.is_empty())
                    .or(attr(e, "connectionManagerRefId")?);
                c.connections.extend(reference.and_then(connection_ref));
            }
            "input" => c.has_input = true,
            "output" if attr(e, "isErrorOut")?.as_deref() != Some("true") => c.has_output = true,
            "property" if c.depth == 2 => {
                let name = attr(e, "name")?;
                if let Some(known) = COMPONENT_PROPERTIES
                    .iter()
                    .find(|p| Some(**p) == name.as_deref())
                {
                    self.begin(Capture::ComponentProperty(known));
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn start_sql_task(&mut self, e: &BytesStart) -> Result<(), ParseError> {
        let Some(&i) = self.open_executables.last() else {
            return Ok(());
        };
        let origin = match attr(e, "SqlStatementSourceType")?.as_deref() {
            Some("Variable") => SqlOrigin::Variable,
            Some("FileConnection") => SqlOrigin::File,
            _ => SqlOrigin::Direct,
        };
        let task = &mut self.doc.executables[i];
        task.connections
            .extend(attr(e, "Connection")?.and_then(connection_ref));
        task.sql = attr(e, "SqlStatementSource")?
            .filter(|text| !text.trim().is_empty())
            .map(|text| SqlStatement { text, origin });
        Ok(())
    }

    fn start_path(&mut self, e: &BytesStart) -> Result<(), ParseError> {
        let (Some(start), Some(end)) = (attr(e, "startId")?, attr(e, "endId")?) else {
            return Ok(());
        };
        if let (Some(&i), Some(from), Some(to)) =
            (self.open_executables.last(), owner(&start), owner(&end))
        {
            self.doc.executables[i].paths.push(DataPath {
                from: from.to_owned(),
                to: to.to_owned(),
            });
        }
        Ok(())
    }

    fn into_document(self) -> Document {
        let mut doc = self.doc;
        for (i, name) in self.expressions {
            let task = &mut doc.executables[i];
            if let Some((component, property)) = component_property(&name) {
                let named = task.components.iter_mut().filter(|c| c.name == component);
                named.for_each(|c| c.expressions.push(property.to_owned()));
            } else if name == "SqlStatementSource" {
                let direct = task.sql.as_mut().filter(|s| s.origin == SqlOrigin::Direct);
                if let Some(sql) = direct {
                    sql.origin = SqlOrigin::Expression;
                }
            }
        }
        doc
    }

    fn start_executable(&mut self, e: &BytesStart) -> Result<(), ParseError> {
        self.doc.executables.push(Executable {
            ref_id: attr(e, "refId")?.unwrap_or_default(),
            name: attr(e, "ObjectName")?.unwrap_or_default(),
            executable_type: attr(e, "ExecutableType")?.unwrap_or_default(),
            disabled: attr(e, "Disabled")?.is_some_and(|d| d.eq_ignore_ascii_case("true")),
            connections: Vec::new(),
            called_package: None,
            components: Vec::new(),
            paths: Vec::new(),
            sql: None,
        });
        self.open_executables.push(self.doc.executables.len() - 1);
        Ok(())
    }

    fn start_connection(&mut self, e: &BytesStart) -> Result<(), ParseError> {
        match self.connection.as_mut() {
            Some(draft) => {
                draft.depth += 1;
                if let Some(raw) = attr(e, "ConnectionString")? {
                    draft.manager.target = summarize_connection_string(&raw);
                }
            }
            None => {
                self.connection = Some(ConnectionDraft {
                    manager: ConnectionManager {
                        id: attr(e, "DTSID")?.unwrap_or_default(),
                        name: attr(e, "ObjectName")?.unwrap_or_default(),
                        kind: attr(e, "CreationName")?.unwrap_or_default(),
                        scope: Scope::Package,
                        target: None,
                    },
                    depth: 1,
                });
            }
        }
        Ok(())
    }

    fn end(&mut self, local: &str) {
        if local != "component" {
            if let Some(c) = self.component.as_mut() {
                c.depth = c.depth.saturating_sub(1);
            }
        }
        match local {
            "Executable" => {
                self.open_executables.pop();
            }
            "ConnectionManager" => {
                let finished = self.connection.as_mut().is_some_and(|draft| {
                    draft.depth -= 1;
                    draft.depth == 0
                });
                if let Some(draft) = self.connection.take_if(|_| finished) {
                    self.doc.connections.push(draft.manager);
                }
            }
            "Property" | "PackageName" | "Connection" | "property" => self.finish_capture(),
            "ExecutePackageTask" => self.in_execute_package = false,
            "component" => {
                if let (Some(c), Some(&i)) = (self.component.take(), self.open_executables.last()) {
                    self.doc.executables[i].components.push(c.finish());
                }
            }
            _ => {}
        }
    }

    fn begin(&mut self, capture: Capture) {
        self.capture = Some(capture);
        self.captured.clear();
    }

    fn finish_capture(&mut self) {
        let Some(capture) = self.capture.take() else {
            return;
        };
        let value = std::mem::take(&mut self.captured).trim().to_owned();
        match (capture, self.open_executables.last().copied()) {
            (Capture::ComponentProperty(name), _) => {
                if let Some(c) = self.component.as_mut() {
                    c.properties.insert(name.to_owned(), value);
                }
            }
            (Capture::FormatVersion, _) => self.doc.format_version = value.parse().ok(),
            (Capture::CalledPackage, Some(i)) => {
                self.doc.executables[i].called_package = Some(value).filter(|v| !v.is_empty());
            }
            (Capture::PackageConnection, Some(i)) => {
                self.doc.executables[i]
                    .connections
                    .extend(connection_ref(value));
            }
            (_, None) => {}
        }
    }
}

fn start_component(e: &BytesStart) -> Result<ComponentDraft, ParseError> {
    Ok(ComponentDraft {
        ref_id: attr(e, "refId")?.unwrap_or_default(),
        name: attr(e, "name")?.unwrap_or_default(),
        class_id: attr(e, "componentClassID")?.unwrap_or_default(),
        connections: Vec::new(),
        properties: BTreeMap::new(),
        has_input: false,
        has_output: false,
        depth: 0,
    })
}

/// Expression de Data Flow `[Composant].[Propriété]`, si la propriété est dans
/// la liste blanche.
fn component_property(name: &str) -> Option<(&str, &'static str)> {
    let (component, property) = name
        .strip_prefix('[')?
        .strip_suffix(']')?
        .split_once("].[")?;
    let known = COMPONENT_PROPERTIES.iter().find(|p| **p == property)?;
    Some((component, known))
}

/// Composant d'une extrémité de lien : `Package\DFT\Comp.Outputs[Sortie]` -> `Package\DFT\Comp`.
fn owner(endpoint: &str) -> Option<&str> {
    endpoint
        .rfind(".Outputs[")
        .or_else(|| endpoint.rfind(".Inputs["))
        .map(|i| &endpoint[..i])
}

fn connection_ref(reference: String) -> Option<ConnectionRef> {
    (!reference.is_empty()).then_some(ConnectionRef {
        reference,
        resolved: None,
    })
}

/// Valeur d'un attribut repéré par son nom local (`DTS:ObjectName` -> `ObjectName`).
pub(crate) fn attr(e: &BytesStart, local: &str) -> Result<Option<String>, ParseError> {
    for a in e.attributes() {
        let a = a?;
        if a.key.local_name().as_ref() == local {
            return Ok(Some(
                a.normalized_value(XmlVersion::Implicit1_0)?.into_owned(),
            ));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = include_str!("../../tests/fixtures/minimal.dtsx");

    fn minimal() -> Package {
        parse_package(MINIMAL).expect("fixture minimale valide")
    }

    fn refs(conns: &[crate::model::ConnectionRef]) -> Vec<&str> {
        conns.iter().map(|c| c.reference.as_str()).collect()
    }

    #[test]
    fn reads_package_name_and_format() {
        let p = minimal();
        assert_eq!(p.name, "Demo_Minimal");
        assert_eq!(p.format_version, 8);
        assert_eq!(p.file, "");
    }

    #[test]
    fn lists_package_connections_without_secrets() {
        let p = minimal();
        let summary: Vec<_> = p
            .connections
            .iter()
            .map(|c| {
                (
                    c.name.as_str(),
                    c.kind.as_str(),
                    c.scope,
                    c.target.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (
                    "Entrepot",
                    "OLEDB",
                    Scope::Package,
                    Some("Data Source=srv-demo;Initial Catalog=Entrepot;Provider=SQLNCLI11.1")
                ),
                (
                    "Fichier clients",
                    "FLATFILE",
                    Scope::Package,
                    Some(r"C:\demo\clients.csv")
                ),
                ("CRM", "DynamicsCRM", Scope::Package, None),
            ]
        );
        assert_eq!(
            p.connections[0].id,
            "{AAAAAAAA-0000-0000-0000-000000000001}"
        );
        assert!(!format!("{p:?}").contains("s3cr3t"));
    }

    #[test]
    fn lists_tasks_in_document_order_including_event_handlers() {
        let p = minimal();
        let tasks: Vec<_> = p
            .executables
            .iter()
            .map(|e| {
                (
                    e.ref_id.as_str(),
                    e.name.as_str(),
                    e.executable_type.as_str(),
                )
            })
            .collect();
        assert_eq!(
            tasks,
            vec![
                (
                    "Package\\SEQ Chargement",
                    "SEQ Chargement",
                    "STOCK:SEQUENCE"
                ),
                (
                    "Package\\SEQ Chargement\\SQL Vider staging",
                    "SQL Vider staging",
                    "Microsoft.ExecuteSQLTask"
                ),
                (
                    "Package\\SEQ Chargement\\DFT Clients",
                    "DFT Clients",
                    "Microsoft.Pipeline"
                ),
                (
                    "Package\\SQL Ancienne purge",
                    "SQL Ancienne purge",
                    "Microsoft.ExecuteSQLTask"
                ),
                (
                    "Package.EventHandlers[OnError]\\SQL Journaliser erreur",
                    "SQL Journaliser erreur",
                    "Microsoft.ExecuteSQLTask"
                ),
            ]
        );
    }

    #[test]
    fn reads_disabled_flag() {
        let p = minimal();
        let disabled: Vec<_> = p
            .executables
            .iter()
            .filter(|e| e.disabled)
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(disabled, vec!["SQL Ancienne purge"]);
    }

    #[test]
    fn reads_sql_task_connection_reference_unresolved() {
        let p = minimal();
        let sql = &p.executables[1];
        assert_eq!(
            refs(&sql.connections),
            vec!["{AAAAAAAA-0000-0000-0000-000000000001}"]
        );
        assert_eq!(sql.connections[0].resolved, None);
        assert!(p.executables[0].connections.is_empty());
    }

    #[test]
    fn lists_data_flow_components_with_structural_roles() {
        let p = minimal();
        let comps: Vec<_> = p.executables[2]
            .components
            .iter()
            .map(|c| (c.name.as_str(), c.role))
            .collect();
        assert_eq!(
            comps,
            vec![
                ("FF Clients", Role::Source),
                ("DER Ajout date", Role::Transform),
                ("DST Clients", Role::Destination),
                ("CRM Comptes", Role::Destination),
            ]
        );
        assert!(p
            .executables
            .iter()
            .filter(|e| e.ref_id != p.executables[2].ref_id)
            .all(|e| e.components.is_empty()));
    }

    #[test]
    fn keeps_unknown_component_class_id() {
        let p = minimal();
        let crm = &p.executables[2].components[3];
        assert!(crm
            .class_id
            .starts_with("KingswaySoft.IntegrationToolkit.DynamicsCrm.CrmDestination,"));
        assert_eq!(
            crm.ref_id,
            "Package\\SEQ Chargement\\DFT Clients\\CRM Comptes"
        );
        assert!(!crm.known);
        assert!(p.executables[2].components[..3].iter().all(|c| c.known));
    }

    #[test]
    fn component_connection_prefers_guid_over_ref_id() {
        // SSIS résout par GUID ; le nom de `connectionManagerRefId` peut être périmé.
        let p = minimal();
        let source = &p.executables[2].components[0];
        assert_eq!(
            refs(&source.connections),
            vec!["{AAAAAAAA-0000-0000-0000-000000000002}"]
        );
        assert!(p.executables[2].components[1].connections.is_empty());
    }

    #[test]
    fn component_connection_falls_back_to_ref_id_without_guid() {
        let p = minimal();
        let crm = &p.executables[2].components[3];
        assert_eq!(
            refs(&crm.connections),
            vec!["Package.ConnectionManagers[CRM]"]
        );
    }

    #[test]
    fn reads_package_called_by_execute_package_tasks() {
        let p = parse_package(include_str!("../../tests/fixtures/orchestrateur.dtsx"))
            .expect("orchestrateur");
        let calls: Vec<_> = p
            .executables
            .iter()
            .map(|e| (e.called_package.as_deref(), refs(&e.connections)))
            .collect();
        assert_eq!(
            calls,
            vec![
                (None, vec![]),
                (Some("01_Récupération & contrôle.dtsx"), vec![]),
                (None, vec!["{EEEEEEEE-0000-0000-0000-000000000001}"]),
            ]
        );
    }

    #[test]
    fn reads_self_closing_executables() {
        let p =
            parse_package(include_str!("../../tests/fixtures/broken/valide.dtsx")).expect("valide");
        assert_eq!(p.executables.len(), 1);
        assert_eq!(p.executables[0].executable_type, "Microsoft.ScriptTask");
    }

    #[test]
    fn rejects_legacy_format() {
        let err =
            parse_package(include_str!("../../tests/fixtures/broken/ancien.dtsx")).unwrap_err();
        assert!(matches!(err, ParseError::UnsupportedFormat(3)), "{err:?}");
    }

    #[test]
    fn rejects_malformed_xml() {
        let err =
            parse_package(include_str!("../../tests/fixtures/broken/corrompu.dtsx")).unwrap_err();
        assert!(matches!(err, ParseError::Xml(_)), "{err:?}");
    }

    #[test]
    fn rejects_non_package_root() {
        let err =
            parse_package(include_str!("../../tests/fixtures/project/Cible.conmgr")).unwrap_err();
        assert!(matches!(err, ParseError::NotAPackage), "{err:?}");
    }

    #[test]
    fn parses_project_connection_manager_without_secrets() {
        let cm = parse_conmgr(include_str!("../../tests/fixtures/project/Cible.conmgr"))
            .expect("conmgr");
        assert_eq!(
            cm,
            ConnectionManager {
                id: "{CCCCCCCC-0000-0000-0000-000000000001}".into(),
                name: "Cible".into(),
                kind: "OLEDB".into(),
                scope: Scope::Project,
                target: Some(
                    "Data Source=srv-demo;Initial Catalog=Cible;Provider=SQLNCLI11.1".into()
                ),
            }
        );
    }

    const CHARGEMENT: &str = include_str!("../../tests/fixtures/lineage/Chargement.dtsx");
    const EXPORT: &str = include_str!("../../tests/fixtures/lineage/Export.dtsx");

    #[test]
    fn managed_components_take_their_class_from_user_component_type_name() {
        let p = parse_package(EXPORT).expect("export");
        let classes: Vec<_> = p
            .executables
            .iter()
            .flat_map(|e| &e.components)
            .map(|c| (c.name.as_str(), c.class_id.as_str(), c.known))
            .collect();
        let crm = "KingswaySoft.IntegrationToolkit.DynamicsCrm.CrmSourceComponent";
        assert_eq!(
            classes,
            vec![
                ("ADO Clients", "Microsoft.DataReaderSourceAdapter", true),
                ("FFD Export", "Microsoft.FlatFileDestination", true),
                ("SCR Source", "Microsoft.ScriptComponentHost", true),
                ("CRM Comptes", crm, false),
                ("CRM Prospects", crm, false),
                ("UNI Fusion", "Microsoft.UnionAll", true),
                ("DST Comptes", "Microsoft.OLEDBDestination", true),
            ]
        );
    }

    #[test]
    fn keeps_only_allowlisted_component_level_properties() {
        let p = parse_package(CHARGEMENT).expect("chargement");
        let props: Vec<Vec<String>> = p.executables[0]
            .components
            .iter()
            .map(|c| {
                c.properties
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect()
            })
            .collect();
        assert_eq!(
            props,
            vec![
                vec![],
                vec!["SqlCommand=select * from [ref].[Pays]".to_owned()],
                vec![
                    "AccessMode=3".to_owned(),
                    "OpenRowset=[stg].[Clients]".to_owned(),
                    "SqlCommand=".to_owned(),
                ],
            ]
        );
    }

    #[test]
    fn decodes_fetch_xml_property() {
        let p = parse_package(EXPORT).expect("export");
        let fetch = &p.executables[1].components[0].properties["FetchXML"];
        assert!(fetch.starts_with("<fetch mapping=\"logical\""), "{fetch}");
        assert!(fetch.contains("<link-entity name=\"contact\""), "{fetch}");
    }

    #[test]
    fn reads_data_flow_paths_between_components() {
        let p = parse_package(EXPORT).expect("export");
        let paths: Vec<_> = p.executables[1]
            .paths
            .iter()
            .map(|d| (d.from.as_str(), d.to.as_str()))
            .collect();
        assert_eq!(
            paths,
            vec![
                (
                    "Package\\DFT CRM\\CRM Comptes",
                    "Package\\DFT CRM\\UNI Fusion"
                ),
                (
                    "Package\\DFT CRM\\CRM Prospects",
                    "Package\\DFT CRM\\UNI Fusion"
                ),
                (
                    "Package\\DFT CRM\\UNI Fusion",
                    "Package\\DFT CRM\\DST Comptes"
                ),
            ]
        );
    }

    #[test]
    fn reads_data_flow_expressions_of_allowlisted_properties() {
        let p = parse_package(EXPORT).expect("export");
        let expressions: Vec<_> = p.executables[0]
            .components
            .iter()
            .map(|c| (c.name.as_str(), c.expressions.clone()))
            .collect();
        assert_eq!(
            expressions,
            vec![
                ("ADO Clients", vec!["SqlCommand".to_owned()]),
                ("FFD Export", vec![]),
                ("SCR Source", vec![]),
            ]
        );
    }

    #[test]
    fn reads_sql_statement_and_its_origin() {
        let p = parse_package(CHARGEMENT).expect("chargement");
        let sql: Vec<_> = p
            .executables
            .iter()
            .filter_map(|e| {
                let s = e.sql.as_ref()?;
                let start: Vec<_> = s.text.split_whitespace().take(3).collect();
                Some((e.name.as_str(), s.origin, start.join(" ")))
            })
            .collect();
        let expected = [
            (
                "SQL Alimenter Clients",
                SqlOrigin::Direct,
                "INSERT INTO dbo.Clients",
            ),
            (
                "SQL Historiser",
                SqlOrigin::Expression,
                "INSERT INTO dbo.Historique",
            ),
            (
                "SQL Purger staging",
                SqlOrigin::Direct,
                "TRUNCATE TABLE stg.Clients",
            ),
            (
                "SQL Import brut",
                SqlOrigin::Direct,
                "BULK INSERT dbo.Import",
            ),
            ("SQL Depuis variable", SqlOrigin::Variable, "User::Requete"),
            (
                "SQL Procedure",
                SqlOrigin::Direct,
                "EXEC dbo.ChargerClients @mode",
            ),
        ];
        let expected: Vec<_> = expected
            .iter()
            .map(|(n, o, s)| (*n, *o, (*s).to_owned()))
            .collect();
        assert_eq!(sql, expected);
    }

    #[test]
    fn rejects_package_as_conmgr() {
        let err = parse_conmgr(MINIMAL).unwrap_err();
        assert!(matches!(err, ParseError::NotAConnectionManager), "{err:?}");
    }
}
