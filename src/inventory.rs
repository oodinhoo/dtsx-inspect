//! Builds the inventory of a file or a folder: reading, parsing, resolving
//! connection references.

use std::collections::BTreeMap;
use std::iter::once;
use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use crate::lineage::sql::{self, Defined, TableName, Tables};
use crate::model::{Component, ConnectionManager, ConnectionRef, Executable, Package};
use crate::parser::{parse_conmgr, parse_package, ParseError};

const PACKAGE_EXTENSION: &str = "dtsx";
const CONMGR_EXTENSION: &str = "conmgr";
const SQL_EXTENSION: &str = "sql";

#[derive(Debug, Default, Serialize)]
pub struct Inventory {
    pub project_connections: Vec<ConnectionManager>,
    pub packages: Vec<Package>,
    /// Procedures and views of the `.sql` scripts found (database project), to trace
    /// in the lineage the procedures called by the packages.
    #[serde(skip)]
    pub sql_scripts: Vec<SqlDefinition>,
}

/// Procedure or view defined in a `.sql` script.
#[derive(Debug)]
pub struct SqlDefinition {
    pub file: String,
    pub defined: Defined,
    pub name: TableName,
    pub tables: Tables,
}

#[derive(Debug, thiserror::Error)]
pub enum FileErrorKind {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Walk(#[from] walkdir::Error),
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error("no .dtsx package found")]
    NoPackage,
}

#[derive(Debug, thiserror::Error)]
#[error("{path}: {kind}")]
pub struct FileError {
    pub path: String,
    #[source]
    pub kind: FileErrorKind,
}

/// Inventories a `.dtsx` (with the `.conmgr` and `.sql` files of its folder) or a
/// folder (recursively). An unreadable file does not stop the others.
pub fn inventory(path: &Path) -> (Inventory, Vec<FileError>) {
    let (files, walk_errors) = collect_files(path);
    let (scripts, files): (Vec<_>, Vec<_>) = files
        .into_iter()
        .partition(|f| has_extension(&f.path, SQL_EXTENSION));
    let (sql_scripts, script_errors) = read_scripts(&scripts);
    let (conmgrs, packages): (Vec<_>, Vec<_>) = files
        .into_iter()
        .partition(|f| has_extension(&f.path, CONMGR_EXTENSION));
    let (project_connections, conmgr_errors) = parse_all(&conmgrs, parse_conmgr);
    // One SSIS project = one folder: its `.conmgr` files sit next to its `.dtsx` files.
    let mut by_project: BTreeMap<&Path, Vec<ConnectionManager>> = BTreeMap::new();
    for (found, cm) in &project_connections {
        by_project
            .entry(dir_of(&found.path))
            .or_default()
            .push(cm.clone());
    }
    let (packages, package_errors) = parse_all(&packages, parse_package);
    let packages: Vec<Package> = packages
        .into_iter()
        .map(|(found, p)| {
            let project = by_project
                .get(dir_of(&found.path))
                .map_or(&[][..], Vec::as_slice);
            let file = found.display.clone();
            resolve_package(Package { file, ..p }, project)
        })
        .collect();
    let project_connections = project_connections.into_iter().map(|(_, c)| c).collect();
    let errors: Vec<FileError> = walk_errors
        .into_iter()
        .chain(conmgr_errors)
        .chain(package_errors)
        .chain(script_errors)
        .collect();
    let errors = if errors.is_empty() && packages.is_empty() {
        vec![FileError {
            path: display(path, path),
            kind: FileErrorKind::NoPackage,
        }]
    } else {
        errors
    };
    (
        Inventory {
            project_connections,
            packages,
            sql_scripts,
        },
        errors,
    )
}

/// Finds the connection designated by an SSIS reference.
pub fn resolve<'a>(
    reference: &str,
    package: &'a [ConnectionManager],
    project: &'a [ConnectionManager],
) -> Option<&'a ConnectionManager> {
    let named = |prefix: &str| reference.strip_prefix(prefix)?.strip_suffix(']');
    if let Some(name) = named("Package.ConnectionManagers[") {
        return package.iter().find(|c| c.name == name);
    }
    if let Some(name) = named("Project.ConnectionManagers[") {
        return project.iter().find(|c| c.name == name);
    }
    // `:invalid`: SSDT marker (connection missing when opened), reported by the lint.
    let id = reference
        .trim_end_matches(":invalid")
        .trim_end_matches(":external");
    if !id.starts_with('{') {
        return None;
    }
    package
        .iter()
        .chain(project)
        .find(|c| c.id.eq_ignore_ascii_case(id))
}

/// SSIS file found, with its displayed path (relative, `/` separators).
struct Found {
    path: PathBuf,
    display: String,
}

fn collect_files(path: &Path) -> (Vec<Found>, Vec<FileError>) {
    if !path.is_file() {
        return walk(path, usize::MAX);
    }
    let dir = dir_of(path);
    let (siblings, errors) = walk(dir, 1);
    // Same path shape as walkdir, to deduplicate a `.conmgr` passed as argument.
    let package_path = dir.join(path.file_name().unwrap_or_default());
    let package = Found {
        path: package_path.clone(),
        display: display(dir, path),
    };
    let files = siblings
        .into_iter()
        .filter(|f| !has_extension(&f.path, PACKAGE_EXTENSION) && f.path != package_path)
        .chain(once(package))
        .collect();
    (files, errors)
}

/// Folder of a file, `.` for a name without folder.
fn dir_of(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

fn walk(root: &Path, max_depth: usize) -> (Vec<Found>, Vec<FileError>) {
    let mut found = Vec::new();
    let mut errors = Vec::new();
    let entries = WalkDir::new(root)
        .max_depth(max_depth)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !(e.file_type().is_dir() && is_build_copy(e.path())));
    for entry in entries {
        match entry {
            Ok(e) if e.file_type().is_file() && is_read(e.path()) => found.push(Found {
                display: display(root, e.path()),
                path: e.into_path(),
            }),
            Ok(_) => {}
            Err(err) => errors.push(FileError {
                path: display(root, err.path().unwrap_or(root)),
                kind: err.into(),
            }),
        }
    }
    (found, errors)
}

fn parse_all<T>(
    files: &[Found],
    parse: impl Fn(&str) -> Result<T, ParseError>,
) -> (Vec<(&Found, T)>, Vec<FileError>) {
    let mut parsed = Vec::new();
    let mut errors = Vec::new();
    for f in files {
        let result = std::fs::read_to_string(&f.path)
            .map_err(FileErrorKind::from)
            .and_then(|xml| parse(&xml).map_err(FileErrorKind::from));
        match result {
            Ok(value) => parsed.push((f, value)),
            Err(kind) => errors.push(FileError {
                path: f.display.clone(),
                kind,
            }),
        }
    }
    (parsed, errors)
}

/// Procedures and views of the scripts. A script without a definition gives none.
fn read_scripts(files: &[Found]) -> (Vec<SqlDefinition>, Vec<FileError>) {
    let mut found = Vec::new();
    let mut errors = Vec::new();
    for f in files {
        match std::fs::read(&f.path) {
            Ok(bytes) => found.extend(sql::definitions(&decode(&bytes)).into_iter().map(
                |(defined, name, tables)| SqlDefinition {
                    file: f.display.clone(),
                    defined,
                    name,
                    tables,
                },
            )),
            Err(error) => errors.push(FileError {
                path: f.display.clone(),
                kind: error.into(),
            }),
        }
    }
    (found, errors)
}

/// Text of a script: UTF-16 when its BOM says so (SSDT), UTF-8 otherwise; an invalid
/// byte (script saved as ANSI) becomes `�` without hindering the reading of names.
fn decode(bytes: &[u8]) -> String {
    if let Some(utf16) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let (pairs, _) = utf16.as_chunks::<2>();
        let units: Vec<u16> = pairs.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(bytes).into_owned()
}

/// Resolves each reference and adds to the package connections the project ones it
/// uses.
fn resolve_package(package: Package, project: &[ConnectionManager]) -> Package {
    let find = |r: &ConnectionRef| resolve(&r.reference, &package.connections, project);
    // shortcut: O(project connections × references), a few hundred at most per package.
    let used: Vec<ConnectionManager> = project
        .iter()
        .filter(|cm| {
            all_refs(&package.executables).any(|r| find(r).is_some_and(|hit| hit.id == cm.id))
        })
        .cloned()
        .collect();
    let resolve_all = |refs: Vec<ConnectionRef>| -> Vec<ConnectionRef> {
        refs.into_iter()
            .map(|r| ConnectionRef {
                resolved: find(&r).map(|c| c.name.clone()),
                ..r
            })
            .collect()
    };
    let executables = package
        .executables
        .into_iter()
        .map(|e| Executable {
            connections: resolve_all(e.connections),
            components: e
                .components
                .into_iter()
                .map(|c| Component {
                    connections: resolve_all(c.connections),
                    ..c
                })
                .collect(),
            ..e
        })
        .collect();
    Package {
        connections: package.connections.into_iter().chain(used).collect(),
        executables,
        ..package
    }
}

fn all_refs(executables: &[Executable]) -> impl Iterator<Item = &ConnectionRef> {
    executables.iter().flat_map(|e| {
        e.connections
            .iter()
            .chain(e.components.iter().flat_map(|c| &c.connections))
    })
}

/// `obj` or `bin` of a project whose source `.dtsx` files are in the parent folder:
/// Visual Studio build copy, which would count each package twice. Without sources
/// next to it (only the copy exists), the folder is read.
fn is_build_copy(dir: &Path) -> bool {
    let named = dir
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(is_build_dir);
    named
        && dir
            .parent()
            .and_then(|p| p.read_dir().ok())
            .is_some_and(|mut siblings| {
                siblings.any(|s| s.is_ok_and(|s| has_extension(&s.path(), PACKAGE_EXTENSION)))
            })
}

/// Folder where Visual Studio copies the packages when building (`obj`, `bin`).
pub(crate) fn is_build_dir(name: &str) -> bool {
    name.eq_ignore_ascii_case("obj") || name.eq_ignore_ascii_case("bin")
}

fn is_read(path: &Path) -> bool {
    [PACKAGE_EXTENSION, CONMGR_EXTENSION, SQL_EXTENSION]
        .iter()
        .any(|extension| has_extension(path, extension))
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
}

fn display(root: &Path, path: &Path) -> String {
    let relative = path
        .strip_prefix(root)
        .ok()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(path);
    relative.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Scope;

    fn cm(id: &str, name: &str, scope: Scope) -> ConnectionManager {
        ConnectionManager {
            id: id.into(),
            name: name.into(),
            kind: "OLEDB".into(),
            scope,
            target: None,
        }
    }

    fn fixtures() -> (Vec<ConnectionManager>, Vec<ConnectionManager>) {
        (
            vec![cm("{AAAA-1}", "Local", Scope::Package)],
            vec![cm("{BBBB-2}", "Shared", Scope::Project)],
        )
    }

    fn name_of(reference: &str) -> Option<String> {
        let (package, project) = fixtures();
        resolve(reference, &package, &project).map(|c| c.name.clone())
    }

    #[test]
    fn resolves_guids_case_insensitively_with_external_suffix() {
        assert_eq!(name_of("{aaaa-1}").as_deref(), Some("Local"));
        assert_eq!(name_of("{bbbb-2}:external").as_deref(), Some("Shared"));
    }

    #[test]
    fn resolves_references_marked_invalid_by_the_designer() {
        // SSDT adds `:invalid` when the connection was missing on opening; the GUID
        // still designates the connection (the lint reports the marker).
        assert_eq!(name_of("{bbbb-2}:invalid").as_deref(), Some("Shared"));
        assert_eq!(
            name_of("{bbbb-2}:external:invalid").as_deref(),
            Some("Shared")
        );
    }

    #[test]
    fn resolves_named_references_in_their_own_scope() {
        assert_eq!(
            name_of("Package.ConnectionManagers[Local]").as_deref(),
            Some("Local")
        );
        assert_eq!(
            name_of("Project.ConnectionManagers[Shared]").as_deref(),
            Some("Shared")
        );
        assert_eq!(name_of("Package.ConnectionManagers[Shared]"), None);
    }

    #[test]
    fn leaves_unknown_references_unresolved() {
        assert_eq!(name_of("{DEAD}"), None);
        assert_eq!(name_of("@[User::Connection]"), None);
        assert_eq!(name_of(""), None);
    }
}
