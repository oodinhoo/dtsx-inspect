use super::clean;
use crate::inventory::Inventory;
use crate::model::{Component, ConnectionRef, Executable, Package, Role, Scope};

pub fn markdown(inventory: &Inventory) -> String {
    inventory
        .packages
        .iter()
        .map(package)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn package(p: &Package) -> String {
    let header = vec![
        clean(&format!(
            "# {} ({}, format {})",
            p.name, p.file, p.format_version
        )),
        String::new(),
    ];
    let flows = p
        .executables
        .iter()
        .filter(|e| !e.components.is_empty())
        .flat_map(data_flow);
    [header, connections(p), tasks(p)]
        .concat()
        .into_iter()
        .chain(flows)
        .collect::<Vec<_>>()
        .join("\n")
}

fn connections(p: &Package) -> Vec<String> {
    let rows = p
        .connections
        .iter()
        .map(|c| {
            let scope = match c.scope {
                Scope::Package => "Package",
                Scope::Project => "Projet",
            };
            row(&[&c.name, &c.kind, scope, c.target.as_deref().unwrap_or("")])
        })
        .collect();
    section("## Connexions", &["Nom", "Type", "Portée", "Cible"], rows)
}

fn tasks(p: &Package) -> Vec<String> {
    // Colonne « Package appelé » seulement pour les orchestrateurs.
    let calls = p.executables.iter().any(|e| e.called_package.is_some());
    let headers = [
        "Chemin",
        "Type",
        "Connexion",
        "Désactivée",
        "Package appelé",
    ];
    let width = if calls {
        headers.len()
    } else {
        headers.len() - 1
    };
    let rows = p
        .executables
        .iter()
        .map(|e| {
            let disabled = if e.disabled { "oui" } else { "" };
            let connections = refs(&e.connections);
            let called = e.called_package.as_deref().unwrap_or("");
            let cells = [e.path(), &e.executable_type, &connections, disabled, called];
            row(&cells[..width])
        })
        .collect();
    section("## Tâches", &headers[..width], rows)
}

fn data_flow(e: &Executable) -> Vec<String> {
    let rows = e.components.iter().map(component).collect();
    section(
        &clean(&format!("### Data Flow {}", e.path())),
        &["Composant", "Classe", "Rôle", "Connexion"],
        rows,
    )
}

fn component(c: &Component) -> String {
    let class = if c.known {
        c.class_id.clone()
    } else {
        format!("Unknown ({})", c.class_id)
    };
    let role = match c.role {
        Role::Source => "Source",
        Role::Transform => "Transformation",
        Role::Destination => "Destination",
    };
    row(&[&c.name, &class, role, &refs(&c.connections)])
}

fn section(title: &str, headers: &[&str], rows: Vec<String>) -> Vec<String> {
    let table = if rows.is_empty() {
        vec!["_Aucune._".to_owned()]
    } else {
        let separator = row(&vec!["---"; headers.len()]);
        [vec![row(headers), separator], rows].concat()
    };
    [
        vec![title.to_owned(), String::new()],
        table,
        vec![String::new()],
    ]
    .concat()
}

fn row(cells: &[&str]) -> String {
    let cells: Vec<String> = cells.iter().map(|c| clean(&escape_cell(c))).collect();
    format!("| {} |", cells.join(" | "))
}

/// `|` ne coupe pas la cellule ; `[` et `]` échappés, un nom ne devient ni lien ni
/// image ; un `\` devant une ponctuation (lu comme échappement) est doublé.
fn escape_cell(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek().is_some_and(char::is_ascii_punctuation) => out.push_str("\\\\"),
            '|' | '[' | ']' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

fn refs(connections: &[ConnectionRef]) -> String {
    connections
        .iter()
        .map(|r| match &r.resolved {
            Some(name) => name.clone(),
            None => format!("{} (non résolue)", r.reference),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cannot_inject_markdown_headings() {
        let flow = Executable {
            ref_id: "Package\\DFT\n## injecté".into(),
            name: "DFT".into(),
            executable_type: "Microsoft.Pipeline".into(),
            disabled: false,
            connections: Vec::new(),
            called_package: None,
            components: vec![Component {
                ref_id: "c".into(),
                name: "c".into(),
                class_id: "Microsoft.RowCount".into(),
                known: true,
                role: Role::Transform,
                connections: Vec::new(),
                properties: Default::default(),
                expressions: Vec::new(),
            }],
            paths: Vec::new(),
            sql: None,
        };
        let inventory = Inventory {
            project_connections: Vec::new(),
            packages: vec![Package {
                file: "p.dtsx".into(),
                name: "P\r\n# injecté".into(),
                format_version: 8,
                connections: Vec::new(),
                executables: vec![flow],
            }],
            sql_scripts: Vec::new(),
        };

        let md = markdown(&inventory);

        assert!(!md.contains("\n# injecté"), "{md}");
        assert!(!md.contains("\n## injecté"), "{md}");
    }

    #[test]
    fn names_cannot_become_links_or_images() {
        // Le rapport HTML passe ce Markdown à `marked`, qui ne filtre pas les URL.
        let cell = row(&["[ouvrir](javascript:alert(1))", "![](http://pisteur/p.png)"]);

        assert_eq!(
            cell,
            r"| \[ouvrir\](javascript:alert(1)) | !\[\](http://pisteur/p.png) |"
        );
        // Un `\` déjà présent ne doit pas annuler l'échappement ; ailleurs il reste tel quel.
        assert_eq!(
            row(&[r"\[x\](javascript:alert(1))", r"Package\DFT"]),
            r"| \\\[x\\\](javascript:alert(1)) | Package\DFT |"
        );
    }
}
