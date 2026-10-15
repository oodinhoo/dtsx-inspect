//! Rapport HTML autonome. À gauche, l'arborescence projets → packages ; à droite,
//! la vue d'ensemble, la carte du lignage ou la page d'un package (flux, inventaire,
//! lint). Mis en forme par le navigateur (ELK pour le graphe, marked pour le Markdown).

use std::collections::{BTreeMap, HashMap};

use super::graph::graph_json;
use super::markdown;
use super::orchestration::orchestration_view;
use crate::calls::calls;
use crate::inventory::{is_build_dir, FileError, Inventory};
use crate::lineage::Lineage;
use crate::lint::{rules, Finding, RuleDoc, Severity};
use crate::model::{Package, Scope};

const STYLE: &str = include_str!("report.css");
const SCRIPT: &str = include_str!("report.js");
const LINEAGE: &str = include_str!("lineage.js");
// ponytail: bibliothèques chargées depuis le CDN ; les embarquer (`include_str!`) si le
// rapport doit s'ouvrir sans internet. Sans elles, navigation et filtres marchent toujours.
// `integrity` : empreinte de la version épinglée, à recalculer si on la change.
const LIBRARIES: &str = r#"<script src="https://cdn.jsdelivr.net/npm/elkjs@0.9.3/lib/elk.bundled.js" integrity="sha384-KO35c6PFq7tFxAEx6FtATwL9vOBOmCzQYr7GM0iTr3oO22s7Jc9OkclOH4AV7jwK" crossorigin="anonymous"></script>
<script src="https://cdn.jsdelivr.net/npm/marked@12.0.2/marked.min.js" integrity="sha384-/TQbtLCAerC3jgaim+N78RZSDYV7ryeoBCVqTuzRrFec2akfBkHS7ACQ3PQhvMVi" crossorigin="anonymous"></script>"#;
const SEVERITIES: [Severity; 3] = [Severity::Erreur, Severity::Avertissement, Severity::Info];
/// Packages listés dans « Les plus signalés ».
const TOP_PACKAGES: usize = 10;
/// Graphe partagé par la carte et le flux d'un package : le script le déplace
/// dans la vue affichée.
const STAGE: &str = "<div class=\"stage\"><div class=\"canvas\"><div class=\"world\">\
<svg class=\"wires\"></svg></div>\
<p class=\"empty offline\">Graphe indisponible : la bibliothèque de mise en page n'a pas pu être \
chargée (poste sans internet ?).</p>\
<p class=\"empty no-block\" hidden>Aucun jeu de données ni étape suivie pour ce package.</p></div>\
<input type=\"search\" class=\"stage-search\" placeholder=\"Rechercher dans le graphe… (Entrée)\">\
<div class=\"zoom\"><button data-zoom=\"in\" title=\"Zoomer\">+</button>\
<button data-zoom=\"out\" title=\"Dézoomer\">−</button>\
<button data-zoom=\"fit\" title=\"Ajuster à l'écran (F)\">⤢</button></div>\
<details class=\"legend\" open><summary>Légende</summary><div class=\"legend-body\"></div></details>\
<aside class=\"inspector\" aria-label=\"Détails\"></aside></div>";

pub fn html(
    title: &str,
    inventory: &Inventory,
    lineage: &Lineage,
    lint: &[Finding],
    errors: &[FileError],
) -> String {
    let title = escape(title);
    let index: HashMap<&str, usize> = inventory
        .packages
        .iter()
        .enumerate()
        .map(|(i, p)| (p.file.as_str(), i))
        .collect();
    let templates: String = inventory
        .packages
        .iter()
        .enumerate()
        .map(|(i, p)| package_template(i, p))
        .collect();
    format!(
        "<!doctype html>\n<html lang=\"fr\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>dtsx-inspect : {title}</title><style>{STYLE}</style></head><body><div class=\"app\">\
{}<main class=\"content\">{}{}{}{}{}</main></div>{templates}\
<script type=\"application/json\" id=\"lineage-data\">{}</script>\
{LIBRARIES}<script>{LINEAGE}</script><script>{SCRIPT}</script></body></html>\n",
        sidebar(&title, inventory, lint),
        overview(inventory, lint, errors, &index),
        map_view(),
        package_view(),
        rules_view(lint),
        orchestration_view(inventory, &calls(inventory)),
        graph_json(inventory, lineage),
    )
}

/// Une fiche par règle, des erreurs aux infos, avec le nombre de constats du rapport.
fn rules_view(lint: &[Finding]) -> String {
    let mut docs: Vec<&RuleDoc> = rules().collect();
    docs.sort_by_key(|d| (SEVERITIES.iter().position(|s| *s == d.severity), d.id));
    let cards: String = docs
        .iter()
        .map(|d| {
            let count = lint.iter().filter(|f| f.rule == d.id).count();
            let found = if count == 0 {
                "<span class=\"muted\">Aucun constat dans ce rapport</span>".to_owned()
            } else {
                format!(
                    "<button class=\"action\" data-show-rule=\"{}\" data-rule-severity=\"{}\">\
Voir {}</button>",
                    escape(d.id),
                    d.severity,
                    plural(count, "constat")
                )
            };
            format!(
                "<article class=\"rule\" id=\"regle-{id}\"><header>\
<span class=\"badge {s}\">{s}</span><h3>{}</h3><code>{id}</code></header>\
<dl><dt>Pourquoi</dt><dd>{}</dd><dt>Corriger</dt><dd>{}</dd></dl>{found}</article>",
                escape(d.title),
                escape(d.why),
                escape(d.fix),
                id = escape(d.id),
                s = d.severity,
            )
        })
        .collect();
    format!(
        "<section id=\"regles\" class=\"view scroll\" hidden><h2>Règles de lint</h2>\
<p class=\"muted\">Chaque constat du rapport vient d'une de ces règles. Erreur : le package \
échouera ou fait autre chose que prévu. Avertissement : il marche, mais lentement ou de façon \
fragile. Info : à savoir, sans urgence.</p><div class=\"rules\">{cards}</div></section>"
    )
}

/// Projet d'un package : le dossier qui le contient (`FINANCE/FINANCE/x.dtsx`),
/// celui au-dessus de `obj` ou `bin` pour une copie de compilation.
pub(super) fn project_of(file: &str) -> &str {
    let mut dirs: Vec<&str> = file.split('/').collect();
    dirs.pop();
    if dirs.len() >= 2 && dirs.get(dirs.len() - 2).is_some_and(|d| is_build_dir(d)) {
        dirs.truncate(dirs.len() - 2);
    }
    dirs.last().copied().unwrap_or("")
}

/// Erreurs et avertissements de lint par fichier.
fn counts_by_file(lint: &[Finding]) -> HashMap<&str, [usize; 2]> {
    let mut counts: HashMap<&str, [usize; 2]> = HashMap::new();
    for f in lint {
        let slot = match f.severity {
            Severity::Erreur => 0,
            Severity::Avertissement => 1,
            Severity::Info => continue,
        };
        counts.entry(&f.file).or_default()[slot] += 1;
    }
    counts
}

fn sidebar(title: &str, inventory: &Inventory, lint: &[Finding]) -> String {
    let counts = counts_by_file(lint);
    let mut projects: BTreeMap<&str, Vec<(usize, &Package)>> = BTreeMap::new();
    // Sans Data Flow (orchestrateur, SQL seul), un package reste accessible par les
    // liens du lint et de la carte, sans encombrer l'arborescence.
    let with_flow = |p: &&Package| p.executables.iter().any(|e| !e.components.is_empty());
    for (i, p) in inventory
        .packages
        .iter()
        .enumerate()
        .filter(|(_, p)| with_flow(p))
    {
        projects
            .entry(project_of(&p.file))
            .or_default()
            .push((i, p));
    }
    let tree: String = projects
        .iter()
        .map(|(project, packages)| {
            let mut total = [0, 0];
            let items: String = packages
                .iter()
                .map(|(i, p)| {
                    let [e, w] = counts.get(p.file.as_str()).copied().unwrap_or_default();
                    total = [total[0] + e, total[1] + w];
                    format!(
                        "<a href=\"#p/{i}\" data-pkg=\"{i}\" title=\"{}\"><span class=\"name\">{}</span>{}</a>",
                        escape(&p.file),
                        escape(&p.name),
                        badges(e, w)
                    )
                })
                .collect();
            let label = if project.is_empty() { "Packages" } else { project };
            format!(
                "<details class=\"project\"><summary><span class=\"name\">{}</span>\
<span class=\"count\">{}</span>{}</summary>{items}</details>",
                escape(label),
                packages.len(),
                badges(total[0], total[1])
            )
        })
        .collect();
    format!(
        "<aside class=\"sidebar\"><div class=\"brand\"><h1>dtsx-inspect</h1>\
<p class=\"path\" title=\"{title}\">{title}</p></div>\
<input type=\"search\" class=\"tree-filter\" placeholder=\"Filtrer les packages… ( / )\">\
<nav class=\"tree\" aria-label=\"Projets et packages\">\
<a href=\"#vue\" class=\"nav-item\">Vue d'ensemble</a>\
<a href=\"#carte\" class=\"nav-item\">Carte du lignage</a>\
<a href=\"#appels\" class=\"nav-item\">Orchestration</a>\
<a href=\"#regles\" class=\"nav-item\">Règles de lint</a>{tree}</nav></aside>"
    )
}

fn badges(errors: usize, warnings: usize) -> String {
    badge(errors, Severity::Erreur) + &badge(warnings, Severity::Avertissement)
}

fn badge(count: usize, severity: Severity) -> String {
    if count == 0 {
        return String::new();
    }
    format!("<span class=\"badge {severity}\">{count}</span>")
}

fn overview(
    inventory: &Inventory,
    lint: &[Finding],
    errors: &[FileError],
    index: &HashMap<&str, usize>,
) -> String {
    let packages = &inventory.packages;
    let executables = || packages.iter().flat_map(|p| &p.executables);
    let local = packages.iter().flat_map(|p| &p.connections);
    let connections =
        inventory.project_connections.len() + local.filter(|c| c.scope == Scope::Package).count();
    let count = |s: Severity| lint.iter().filter(|f| f.severity == s).count();
    let cards = [
        (packages.len(), "packages", ""),
        (connections, "connexions", ""),
        (executables().count(), "tâches et conteneurs", ""),
        (
            executables().filter(|e| !e.components.is_empty()).count(),
            "Data Flows",
            "",
        ),
        (
            executables()
                .flat_map(|e| &e.components)
                .filter(|c| !c.known)
                .count(),
            "composants tiers",
            "",
        ),
        (count(Severity::Erreur), "erreurs de lint", "erreur"),
        (
            count(Severity::Avertissement),
            "avertissements",
            "avertissement",
        ),
        (errors.len(), "fichiers illisibles", "erreur"),
    ];
    let cards: String = cards
        .iter()
        .map(|(n, label, alert)| {
            let class = if *n > 0 { alert } else { &"" };
            format!("<div class=\"card {class}\"><strong>{n}</strong>{label}</div>")
        })
        .collect();
    let unreadable = if errors.is_empty() {
        String::new()
    } else {
        let items: String = errors
            .iter()
            .map(|e| format!("<li>{}</li>", escape(&e.to_string())))
            .collect();
        format!("<h3>Fichiers illisibles</h3><ul>{items}</ul>")
    };
    format!(
        "<section id=\"vue\" class=\"view scroll\"><h2>Vue d'ensemble</h2>\
<div class=\"cards\">{cards}</div>{unreadable}{}{}</section>",
        most_flagged(lint, index),
        lint_table(lint, index)
    )
}

/// Packages ayant le plus d'erreurs, puis d'avertissements.
fn most_flagged(lint: &[Finding], index: &HashMap<&str, usize>) -> String {
    let mut ranked: Vec<_> = counts_by_file(lint).into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    if ranked.is_empty() {
        return String::new();
    }
    let rows: String = ranked
        .iter()
        .take(TOP_PACKAGES)
        .map(|(file, [errors, warnings])| {
            format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                file_link(file, index, "lint"),
                badge(*errors, Severity::Erreur),
                badge(*warnings, Severity::Avertissement)
            )
        })
        .collect();
    format!(
        "<h3>Les plus signalés</h3><table><thead><tr><th>Fichier</th><th>Erreurs</th>\
<th>Avertissements</th></tr></thead><tbody>{rows}</tbody></table>"
    )
}

/// Lien vers la page d'un package ; texte seul pour `projet` (connexions de projet).
fn file_link(file: &str, index: &HashMap<&str, usize>, sub: &str) -> String {
    match index.get(file) {
        Some(i) => format!("<a href=\"#p/{i}/{sub}\">{}</a>", breakable(file)),
        None => breakable(file),
    }
}

/// Chemin échappé, coupable après chaque séparateur plutôt qu'au milieu d'un nom.
fn breakable(path: &str) -> String {
    escape(path).replace('/', "/<wbr>").replace('\\', "\\<wbr>")
}

fn lint_table(lint: &[Finding], index: &HashMap<&str, usize>) -> String {
    if lint.is_empty() {
        return "<h3>Constats de lint</h3><p class=\"empty\">Aucun constat.</p>".to_owned();
    }
    // Les infos (angles morts du lignage) sont masquées d'office : souvent nombreuses.
    let filters: String = SEVERITIES
        .iter()
        .map(|s| {
            let checked = if *s == Severity::Info { "" } else { " checked" };
            format!("<label><input type=\"checkbox\" data-severity=\"{s}\"{checked}> {s}</label>")
        })
        .collect();
    let rows: String = lint
        .iter()
        .map(|f| {
            let pkg = index
                .get(f.file.as_str())
                .map_or(String::new(), |i| format!(" data-pkg=\"{i}\""));
            format!(
                "<tr class=\"{s}\"{pkg}><td><span class=\"badge {s}\">{s}</span></td>\
<td><a href=\"#regles/{id}\" title=\"Voir la règle\">{id}</a></td><td>{}</td><td>{}</td><td>{}</td></tr>",
                file_link(&f.file, index, "flux"),
                breakable(&f.location),
                escape(&f.message),
                s = f.severity,
                id = escape(f.rule),
            )
        })
        .collect();
    format!(
        "<h3>Constats de lint</h3><div class=\"toolbar\">{filters}\
<input type=\"search\" placeholder=\"Filtrer par fichier, tâche, message…\" data-filter=\"lint-rows\">\
</div><table class=\"lint hide-info\"><thead><tr><th>Gravité</th><th>Règle</th><th>Fichier</th>\
<th>Emplacement</th><th>Message</th></tr></thead><tbody id=\"lint-rows\">{rows}</tbody></table>"
    )
}

fn map_view() -> String {
    format!(
        "<section id=\"carte\" class=\"view\" hidden><div class=\"view-head\"><h2>Carte du lignage</h2>\
<select class=\"project-select\" aria-label=\"Projet\"></select></div>\
<div class=\"stage-slot\">{STAGE}</div></section>"
    )
}

fn package_view() -> String {
    "<section id=\"package\" class=\"view\" hidden><div class=\"view-head\"><div>\
<p class=\"crumb\"></p><h2 class=\"pkg-name\"></h2><p class=\"muted pkg-meta\"></p></div>\
<nav class=\"subtabs\" aria-label=\"Vues du package\"><a data-sub=\"flux\">Flux</a>\
<a data-sub=\"inventaire\">Inventaire</a><a data-sub=\"lint\">Lint <span class=\"count\"></span></a>\
</nav></div>\
<div class=\"sub stage-slot\" data-sub=\"flux\"></div>\
<div class=\"sub scroll md-host\" data-sub=\"inventaire\"></div>\
<div class=\"sub scroll\" data-sub=\"lint\"><table class=\"lint pkg-lint\"><thead><tr><th>Gravité</th>\
<th>Règle</th><th>Fichier</th><th>Emplacement</th><th>Message</th></tr></thead><tbody></tbody></table>\
<p class=\"empty\" hidden>Aucun constat pour ce package.</p></div></section>"
        .to_owned()
}

/// Inventaire d'un package, en Markdown, et ses métadonnées : copiés dans la
/// page du package quand on l'ouvre.
fn package_template(i: usize, p: &Package) -> String {
    let tasks = p.executables.len();
    let flows = p
        .executables
        .iter()
        .filter(|e| !e.components.is_empty())
        .count();
    format!(
        "<template id=\"pkg-{i}\" data-name=\"{}\" data-file=\"{}\" data-project=\"{}\" \
data-meta=\"{}\"><pre class=\"md\">{}</pre></template>",
        escape(&p.name),
        escape(&p.file),
        escape(project_of(&p.file)),
        escape(&format!(
            "format {} · {} · {}",
            p.format_version,
            plural(tasks, "tâche"),
            plural(flows, "Data Flow")
        )),
        escape(&markdown::package(p))
    )
}

/// `1 tâche`, `2 tâches` (en français, 0 est au singulier).
fn plural(count: usize, word: &str) -> String {
    let s = if count > 1 { "s" } else { "" };
    format!("{count} {word}{s}")
}

pub(super) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cannot_inject_html() {
        let page = html(
            "x</title><script>alert(1)</script>",
            &Inventory::default(),
            &Lineage::default(),
            &[],
            &[],
        );
        assert!(!page.contains("<script>alert"), "{page}");
    }

    #[test]
    fn the_orchestration_page_shows_who_calls_whom() {
        let (inventory, _) = crate::inventory(std::path::Path::new("tests/fixtures/appels"));

        let page = html("x", &inventory, &Lineage::default(), &[], &[]);

        assert!(
            page.contains("href=\"#appels\""),
            "lien manquant dans la barre latérale"
        );
        let view = &page[page.find("id=\"appels\"").expect("vue Orchestration")..];
        // Sans Data Flow, un package s'ouvre sur son inventaire.
        assert!(view.contains("href=\"#p/2/inventaire\""), "{view}");
        let root = view.find(">Orchestrateur</a>").expect("racine");
        let child = view.find(">Charger</a>").expect("appel");
        let grandchild = view.find(">Fils</a>").expect("appel imbriqué");
        assert!(root < child && child < grandchild, "{view}");
        assert!(view.contains("02_Absent.dtsx"), "{view}");
    }

    #[test]
    fn the_rules_page_explains_every_rule() {
        let page = html("x", &Inventory::default(), &Lineage::default(), &[], &[]);

        assert!(
            page.contains("href=\"#regles\""),
            "lien manquant dans la barre latérale"
        );
        for doc in crate::lint::rules() {
            assert!(
                page.contains(&format!("id=\"regle-{}\"", doc.id)),
                "{}",
                doc.id
            );
            assert!(page.contains(&escape(doc.fix)), "{}", doc.id);
        }
    }

    #[test]
    fn each_finding_links_to_its_rule() {
        let finding = Finding {
            rule: "tri-dans-le-flux",
            severity: Severity::Avertissement,
            file: "p.dtsx".into(),
            location: "DFT\\Trier".into(),
            message: "m".into(),
        };

        let table = lint_table(&[finding], &HashMap::new());

        assert!(
            table.contains("href=\"#regles/tri-dans-le-flux\""),
            "{table}"
        );
    }

    #[test]
    fn the_tree_lists_only_packages_with_a_data_flow() {
        let fixture =
            |name: &str| crate::inventory(&std::path::Path::new("tests/fixtures").join(name)).0;
        let packages = fixture("minimal.dtsx")
            .packages
            .into_iter()
            .chain(fixture("orchestrateur.dtsx").packages)
            .collect();
        let inventory = Inventory {
            packages,
            ..Inventory::default()
        };

        let tree = sidebar("x", &inventory, &[]);

        assert!(tree.contains("Demo_Minimal"), "{tree}");
        // L'orchestrateur n'a que des Execute Package Task : sa page reste
        // accessible par les liens du lint, mais il encombrerait l'arborescence.
        assert!(!tree.contains("Orchestrateur"), "{tree}");
    }

    #[test]
    fn external_scripts_are_pinned_by_their_hash() {
        // Un fichier modifié sur le CDN lirait tout le rapport : le navigateur le refuse.
        let tags: Vec<&str> = LIBRARIES.split("<script").skip(1).collect();
        assert_eq!(tags.len(), 2);
        for tag in tags {
            assert!(tag.contains("integrity=\"sha384-"), "{tag}");
            assert!(tag.contains("crossorigin=\"anonymous\""), "{tag}");
        }
    }

    #[test]
    fn packages_are_grouped_by_project_in_the_tree() {
        assert_eq!(project_of("FINANCE/FINANCE/01_Init.dtsx"), "FINANCE");
        assert_eq!(project_of("01_Init.dtsx"), "");
        // Dossier analysé plus haut : le projet reste le dossier du package.
        assert_eq!(project_of("Packages SSIS/SALES/SALES/21_Diff.dtsx"), "SALES");
        assert_eq!(
            project_of("Packages SSIS/DW_SALES_ONLY/01.dtsx"),
            "DW_SALES_ONLY"
        );
        // Copie de compilation seule (Demo) : le projet est au-dessus de `obj`.
        assert_eq!(project_of("Demo/Demo/obj/Development/01.dtsx"), "Demo");
    }
}
