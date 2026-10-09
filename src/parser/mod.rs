//! XML SSIS -> modèle. Aucune logique métier ici.

pub mod secrets;

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use crate::model::{Component, ConnectionManager, ConnectionRef, Executable, Package, Role, Scope};
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

struct ComponentDraft {
    ref_id: String,
    name: String,
    class_id: String,
    connections: Vec<ConnectionRef>,
    has_input: bool,
    has_output: bool,
}

impl ComponentDraft {
    /// Rôle déduit de la structure : marche aussi pour les composants tiers.
    fn finish(self) -> Component {
        let role = match (self.has_input, self.has_output) {
            (false, _) => Role::Source,
            (true, false) => Role::Destination,
            (true, true) => Role::Transform,
        };
        Component {
            ref_id: self.ref_id,
            name: self.name,
            // ponytail: « connu » = composant Microsoft ; un enum par classe viendra avec le lignage.
            known: self.class_id.starts_with("Microsoft."),
            class_id: self.class_id,
            role,
            connections: self.connections,
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
            Event::Eof => return Ok(state.doc),
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
}

impl Reading {
    fn start(&mut self, e: &BytesStart) -> Result<(), ParseError> {
        let local = e.local_name();
        if self.doc.root.is_none() {
            self.doc.root = Some(Root {
                element: local.as_ref().to_owned(),
                executable_type: attr(e, "ExecutableType")?.unwrap_or_default(),
                name: attr(e, "ObjectName")?.unwrap_or_default(),
            });
            if local.as_ref() == "Executable" {
                return Ok(());
            }
        }
        match local.as_ref() {
            "Executable" => self.start_executable(e)?,
            "ConnectionManager" => self.start_connection(e)?,
            "Property"
                if self.open_executables.is_empty()
                    && attr(e, "Name")?.as_deref() == Some("PackageFormatVersion") =>
            {
                self.begin(Capture::FormatVersion)
            }
            "ExecutePackageTask" => self.in_execute_package = true,
            "PackageName" if self.in_execute_package => self.begin(Capture::CalledPackage),
            // Référence externe : connexion FILE (ou OLE DB pour msdb) vers le package.
            "Connection" if self.in_execute_package => self.begin(Capture::PackageConnection),
            "SqlTaskData" => {
                let reference = attr(e, "Connection")?;
                if let (Some(&i), Some(r)) = (self.open_executables.last(), reference) {
                    self.doc.executables[i]
                        .connections
                        .extend(connection_ref(r));
                }
            }
            "component" => self.component = Some(start_component(e)?),
            "connection" => {
                // Le GUID fait foi (c'est lui que SSIS résout) ; le nom de
                // `connectionManagerRefId` reste figé après un renommage.
                let reference = attr(e, "connectionManagerID")?
                    .filter(|r| !r.is_empty())
                    .or(attr(e, "connectionManagerRefId")?);
                if let (Some(c), Some(r)) = (self.component.as_mut(), reference) {
                    c.connections.extend(connection_ref(r));
                }
            }
            "input" => self.component.iter_mut().for_each(|c| c.has_input = true),
            "output" if attr(e, "isErrorOut")?.as_deref() != Some("true") => {
                self.component.iter_mut().for_each(|c| c.has_output = true);
            }
            _ => {}
        }
        Ok(())
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
            "Property" | "PackageName" | "Connection" => self.finish_capture(),
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
        has_input: false,
        has_output: false,
    })
}

fn connection_ref(reference: String) -> Option<ConnectionRef> {
    (!reference.is_empty()).then_some(ConnectionRef {
        reference,
        resolved: None,
    })
}

/// Valeur d'un attribut repéré par son nom local (`DTS:ObjectName` -> `ObjectName`).
fn attr(e: &BytesStart, local: &str) -> Result<Option<String>, ParseError> {
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
    use crate::model::{Role, Scope};

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

    #[test]
    fn rejects_package_as_conmgr() {
        let err = parse_conmgr(MINIMAL).unwrap_err();
        assert!(matches!(err, ParseError::NotAConnectionManager), "{err:?}");
    }
}
