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
    [
        header,
        connections(p),
        parameters(p),
        variables(p),
        tasks(p),
        expressions(p),
    ]
    .concat()
    .into_iter()
    .chain(flows)
    .collect::<Vec<_>>()
    .join("\n")
}

/// Section left out when it would be empty: most packages have neither parameter
/// nor expression.
fn optional(title: &str, headers: &[&str], rows: Vec<String>) -> Vec<String> {
    if rows.is_empty() {
        Vec::new()
    } else {
        section(title, headers, rows)
    }
}

fn parameters(p: &Package) -> Vec<String> {
    let rows = p
        .parameters
        .iter()
        .map(|x| {
            let value = match (&x.value, x.sensitive) {
                (_, true) => "(sensitive)",
                (Some(value), false) => value,
                (None, false) => "",
            };
            row(&[&x.name, value])
        })
        .collect();
    optional("## Parameters", &["Name", "Value"], rows)
}

fn variables(p: &Package) -> Vec<String> {
    let rows = p
        .variables
        .iter()
        .map(|v| {
            let scope = if v.scope.is_empty() {
                "package"
            } else {
                &v.scope
            };
            let value = v.value.as_deref().unwrap_or("");
            row(&[scope, &v.name, value, v.expression.as_deref().unwrap_or("")])
        })
        .collect();
    optional(
        "## Variables",
        &["Scope", "Name", "Value", "Expression"],
        rows,
    )
}

fn expressions(p: &Package) -> Vec<String> {
    let rows = p
        .expressions
        .iter()
        .map(|x| row(&[&x.owner, &x.property, &x.expression]))
        .collect();
    optional("## Expressions", &["Owner", "Property", "Expression"], rows)
}

fn connections(p: &Package) -> Vec<String> {
    let rows = p
        .connections
        .iter()
        .map(|c| {
            let scope = match c.scope {
                Scope::Package => "Package",
                Scope::Project => "Project",
            };
            row(&[&c.name, &c.kind, scope, c.target.as_deref().unwrap_or("")])
        })
        .collect();
    section("## Connections", &["Name", "Type", "Scope", "Target"], rows)
}

fn tasks(p: &Package) -> Vec<String> {
    // "Called package" column only for orchestrators.
    let calls = p.executables.iter().any(|e| e.called_package.is_some());
    let headers = ["Path", "Type", "Connection", "Disabled", "Called package"];
    let width = if calls {
        headers.len()
    } else {
        headers.len() - 1
    };
    let rows = p
        .executables
        .iter()
        .map(|e| {
            let disabled = if e.disabled { "yes" } else { "" };
            let connections = refs(&e.connections);
            let called = e.called_package.as_deref().unwrap_or("");
            let cells = [e.path(), &e.executable_type, &connections, disabled, called];
            row(&cells[..width])
        })
        .collect();
    section("## Tasks", &headers[..width], rows)
}

fn data_flow(e: &Executable) -> Vec<String> {
    let rows = e.components.iter().map(component).collect();
    section(
        &clean(&format!("### Data Flow {}", e.path())),
        &["Component", "Class", "Role", "Connection"],
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
        vec!["_None._".to_owned()]
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

/// `|` does not split the cell; `[` and `]` escaped, a name becomes neither link nor
/// image; a `\` before a punctuation (read as an escape) is doubled.
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
            None => format!("{} (unresolved)", r.reference),
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
            ref_id: "Package\\DFT\n## injected".into(),
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
                inputs: Vec::new(),
                outputs: Vec::new(),
            }],
            paths: Vec::new(),
            sql: None,
        };
        let inventory = Inventory {
            project_connections: Vec::new(),
            packages: vec![Package {
                file: "p.dtsx".into(),
                name: "P\r\n# injected".into(),
                format_version: 8,
                connections: Vec::new(),
                executables: vec![flow],
                ..Package::default()
            }],
            sql_scripts: Vec::new(),
        };

        let md = markdown(&inventory);

        assert!(!md.contains("\n# injected"), "{md}");
        assert!(!md.contains("\n## injected"), "{md}");
    }

    #[test]
    fn names_cannot_become_links_or_images() {
        // The HTML report hands this Markdown to `marked`, which does not filter URLs.
        let cell = row(&["[open](javascript:alert(1))", "![](http://tracker/p.png)"]);

        assert_eq!(
            cell,
            r"| \[open\](javascript:alert(1)) | !\[\](http://tracker/p.png) |"
        );
        // An existing `\` must not cancel the escape; elsewhere it stays as is.
        assert_eq!(
            row(&[r"\[x\](javascript:alert(1))", r"Package\DFT"]),
            r"| \\\[x\\\](javascript:alert(1)) | Package\DFT |"
        );
    }
}
