//! Page « Orchestration » du rapport : l'arbre des appels de packages.

use std::collections::HashSet;

use super::html::escape;
use crate::calls::{Call, Callee};
use crate::inventory::Inventory;

/// Les packages que personne n'appelle en tête, chaque appel en dessous : tâche,
/// package appelé, puis ses propres appels.
pub(super) fn orchestration_view(inventory: &Inventory, calls: &[Call]) -> String {
    let body = if calls.is_empty() {
        "<p class=\"empty\">Aucune tâche Execute Package dans ce dossier.</p>".to_owned()
    } else {
        let called: HashSet<usize> = calls
            .iter()
            .filter_map(|c| match c.callee {
                Callee::Package(i) => Some(i),
                Callee::Missing(_) => None,
            })
            .collect();
        // `calls` est rangé par appelant : `dedup` suffit.
        let mut roots: Vec<usize> = calls
            .iter()
            .map(|c| c.caller)
            .filter(|i| !called.contains(i))
            .collect();
        roots.dedup();
        // Que des appels en boucle : aucun package n'est en tête, on part du premier.
        if roots.is_empty() {
            roots.push(calls[0].caller);
        }
        let tree: String = roots
            .iter()
            .map(|&i| {
                let sub = call_list(inventory, calls, i, &mut vec![i]);
                format!("<li>{}{sub}</li>", link(inventory, i))
            })
            .collect();
        format!("<ul class=\"calls\">{tree}</ul>")
    };
    format!(
        "<section id=\"appels\" class=\"view scroll\" hidden><h2>Orchestration</h2>\
<p class=\"muted\">Qui lance quoi : chaque tâche Execute Package et le package qu'elle appelle. \
En tête, les packages que personne n'appelle.</p>{body}</section>"
    )
}

/// Appels d'un package ; `path` (appelants en cours) arrête un appel circulaire.
fn call_list(
    inventory: &Inventory,
    calls: &[Call],
    caller: usize,
    path: &mut Vec<usize>,
) -> String {
    let mut items = String::new();
    for call in calls.iter().filter(|c| c.caller == caller) {
        let via = format!("<span class=\"via\">{}</span>", escape(&call.task));
        let item = match &call.callee {
            Callee::Missing(name) => format!(
                "<li class=\"missing\">{via}<span>{}</span>\
<span class=\"badge avertissement\">introuvable</span></li>",
                escape(name)
            ),
            Callee::Package(i) if path.contains(i) => format!(
                "<li>{via}{}<span class=\"badge avertissement\">appel circulaire</span></li>",
                link(inventory, *i)
            ),
            Callee::Package(i) => {
                path.push(*i);
                let sub = call_list(inventory, calls, *i, path);
                path.pop();
                format!("<li>{via}{}{sub}</li>", link(inventory, *i))
            }
        };
        items.push_str(&item);
    }
    if items.is_empty() {
        items
    } else {
        format!("<ul>{items}</ul>")
    }
}

/// Lien vers la page du package : son flux s'il a un Data Flow, sinon son inventaire.
fn link(inventory: &Inventory, i: usize) -> String {
    let Some(p) = inventory.packages.get(i) else {
        return "?".to_owned();
    };
    let has_flow = p.executables.iter().any(|e| !e.components.is_empty());
    let sub = if has_flow { "flux" } else { "inventaire" };
    format!(
        "<a href=\"#p/{i}/{sub}\" title=\"{}\">{}</a>",
        escape(&p.file),
        escape(&p.name)
    )
}
