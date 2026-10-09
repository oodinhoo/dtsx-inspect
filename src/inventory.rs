//! Assemble l'inventaire d'un fichier ou d'un dossier : lecture, parsing,
//! résolution des références de connexion.

use std::collections::BTreeMap;
use std::iter::once;
use std::path::{Path, PathBuf};

use serde::Serialize;
use walkdir::WalkDir;

use crate::model::{Component, ConnectionManager, ConnectionRef, Executable, Package};
use crate::parser::{parse_conmgr, parse_package, ParseError};

const PACKAGE_EXTENSION: &str = "dtsx";
const CONMGR_EXTENSION: &str = "conmgr";

#[derive(Debug, Default, Serialize)]
pub struct Inventory {
    pub project_connections: Vec<ConnectionManager>,
    pub packages: Vec<Package>,
}

#[derive(Debug, thiserror::Error)]
pub enum FileErrorKind {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Walk(#[from] walkdir::Error),
    #[error(transparent)]
    Parse(#[from] ParseError),
    #[error("aucun package .dtsx trouvé")]
    NoPackage,
}

#[derive(Debug, thiserror::Error)]
#[error("{path} : {kind}")]
pub struct FileError {
    pub path: String,
    #[source]
    pub kind: FileErrorKind,
}

/// Inventorie un `.dtsx` (avec les `.conmgr` de son dossier) ou un dossier
/// (récursivement). Un fichier illisible n'arrête pas les autres.
pub fn inventory(path: &Path) -> (Inventory, Vec<FileError>) {
    let (files, walk_errors) = collect_files(path);
    let (conmgrs, packages): (Vec<_>, Vec<_>) = files
        .into_iter()
        .partition(|f| has_extension(&f.path, CONMGR_EXTENSION));
    let (project_connections, conmgr_errors) = parse_all(&conmgrs, parse_conmgr);
    // Un projet SSIS = un dossier : ses `.conmgr` sont à côté de ses `.dtsx`.
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
        },
        errors,
    )
}

/// Retrouve la connexion désignée par une référence SSIS.
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
    let id = reference.strip_suffix(":external").unwrap_or(reference);
    if !id.starts_with('{') {
        return None;
    }
    package
        .iter()
        .chain(project)
        .find(|c| c.id.eq_ignore_ascii_case(id))
}

/// Fichier SSIS trouvé, avec son chemin affiché (relatif, séparateurs `/`).
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
    // Même forme de chemin que walkdir, pour dédoublonner un `.conmgr` passé en argument.
    let package_path = dir.join(path.file_name().unwrap_or_default());
    let package = Found {
        path: package_path.clone(),
        display: display(dir, path),
    };
    let files = siblings
        .into_iter()
        .filter(|f| has_extension(&f.path, CONMGR_EXTENSION) && f.path != package_path)
        .chain(once(package))
        .collect();
    (files, errors)
}

/// Dossier d'un fichier, `.` pour un nom sans dossier.
fn dir_of(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

fn walk(root: &Path, max_depth: usize) -> (Vec<Found>, Vec<FileError>) {
    let mut found = Vec::new();
    let mut errors = Vec::new();
    for entry in WalkDir::new(root).max_depth(max_depth).sort_by_file_name() {
        match entry {
            Ok(e) if e.file_type().is_file() && is_ssis_file(e.path()) => found.push(Found {
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

/// Résout chaque référence et ajoute aux connexions du package celles du
/// projet qu'il utilise.
fn resolve_package(package: Package, project: &[ConnectionManager]) -> Package {
    let Package {
        file,
        name,
        format_version,
        connections,
        executables,
    } = package;
    let find = |r: &ConnectionRef| resolve(&r.reference, &connections, project);
    // ponytail: O(connexions projet × références), quelques centaines au plus par package.
    let used: Vec<ConnectionManager> = project
        .iter()
        .filter(|cm| all_refs(&executables).any(|r| find(r).is_some_and(|hit| hit.id == cm.id)))
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
    let executables = executables
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
        file,
        name,
        format_version,
        connections: connections.into_iter().chain(used).collect(),
        executables,
    }
}

fn all_refs(executables: &[Executable]) -> impl Iterator<Item = &ConnectionRef> {
    executables.iter().flat_map(|e| {
        e.connections
            .iter()
            .chain(e.components.iter().flat_map(|c| &c.connections))
    })
}

fn is_ssis_file(path: &Path) -> bool {
    has_extension(path, PACKAGE_EXTENSION) || has_extension(path, CONMGR_EXTENSION)
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
            vec![cm("{BBBB-2}", "Projet", Scope::Project)],
        )
    }

    fn name_of(reference: &str) -> Option<String> {
        let (package, project) = fixtures();
        resolve(reference, &package, &project).map(|c| c.name.clone())
    }

    #[test]
    fn resolves_guids_case_insensitively_with_external_suffix() {
        assert_eq!(name_of("{aaaa-1}").as_deref(), Some("Local"));
        assert_eq!(name_of("{bbbb-2}:external").as_deref(), Some("Projet"));
    }

    #[test]
    fn resolves_named_references_in_their_own_scope() {
        assert_eq!(
            name_of("Package.ConnectionManagers[Local]").as_deref(),
            Some("Local")
        );
        assert_eq!(
            name_of("Project.ConnectionManagers[Projet]").as_deref(),
            Some("Projet")
        );
        assert_eq!(name_of("Package.ConnectionManagers[Projet]"), None);
    }

    #[test]
    fn leaves_unknown_references_unresolved() {
        assert_eq!(name_of("{DEAD}"), None);
        assert_eq!(name_of("@[User::Connexion]"), None);
        assert_eq!(name_of(""), None);
    }
}
