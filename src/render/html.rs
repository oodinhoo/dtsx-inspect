//! Rapport HTML autonome. En haut, la recherche globale et un onglet par espace (vue
//! d'ensemble, carte du lignage, colonnes, packages, qualité) ; la page
//! d'un package a ses propres onglets (flux, inventaire, lint). Mis en forme par le
//! navigateur (ELK pour le graphe, marked pour le Markdown).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::graph::graph_json;
use super::mapping::{mapping_rows, to_csv, Row, STATUSES};
use super::{markdown, plural};
use crate::inventory::{is_build_dir, FileError, Inventory};
use crate::lineage::columns::column_lineage;
use crate::lineage::Lineage;
use crate::lint::{rules, Finding, RuleDoc, Severity};
use crate::model::{Package, Scope};

const STYLE: &str = include_str!("report.css");
const SCRIPT: &str = include_str!("report.js");
const LINEAGE: &str = include_str!("lineage.js");
const COLUMNS: &str = include_str!("columns.js");
const SEARCH: &str = include_str!("search.js");
// ponytail: bibliothèques chargées depuis le CDN ; les embarquer (`include_str!`) si le
// rapport doit s'ouvrir sans internet. Sans elles, navigation et filtres marchent toujours.
// `integrity` : empreinte de la version épinglée, à recalculer si on la change.
const LIBRARIES: &str = r#"<script src="https://cdn.jsdelivr.net/npm/elkjs@0.9.3/lib/elk.bundled.js" integrity="sha384-KO35c6PFq7tFxAEx6FtATwL9vOBOmCzQYr7GM0iTr3oO22s7Jc9OkclOH4AV7jwK" crossorigin="anonymous"></script>
<script src="https://cdn.jsdelivr.net/npm/marked@12.0.2/marked.min.js" integrity="sha384-/TQbtLCAerC3jgaim+N78RZSDYV7ryeoBCVqTuzRrFec2akfBkHS7ACQ3PQhvMVi" crossorigin="anonymous"></script>"#;
// IBM Plex depuis Google Fonts ; sans internet, les polices système de `report.css`.
const FONTS: &str = "<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\
<link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500\
&amp;family=IBM+Plex+Sans:wght@400;500;600;700&amp;display=swap\">";
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
    let columns = column_lineage(inventory);
    let rows = mapping_rows(inventory, &columns);
    // Même CSV que `dtsx-inspect mapping`, téléchargé par les boutons d'export.
    // Le navigateur ramènerait `\r\n` à `\n` et perdrait le BOM : références de caractères.
    let mapping = escape(&to_csv(&rows))
        .replace('\r', "&#13;")
        .replace('\u{feff}', "&#xFEFF;");
    format!(
        "<!doctype html>\n<html lang=\"fr\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>dtsx-inspect : {title}</title>{FONTS}<style>{STYLE}</style></head><body><div class=\"app\">\
{}<main class=\"content\">{}{}{}{}{}{}</main></div>{templates}\
<template id=\"mapping-csv\">{mapping}</template>\
<script type=\"application/json\" id=\"lineage-data\">{}</script>\
{LIBRARIES}<script>{LINEAGE}</script><script>{COLUMNS}</script><script>{SEARCH}</script>\
<script>{SCRIPT}</script></body></html>\n",
        rail(&title, inventory, lint),
        overview(inventory, lint, errors, &index, &rows),
        map_view(),
        columns_view(),
        packages_view(inventory, lint),
        package_view(),
        quality_view(lint, &index),
        graph_json(inventory, lineage, &columns),
    )
}

/// Rail de navigation : dossier analysé, recherche globale (Ctrl K), un lien par espace
/// (report.js marque le courant, `aria-current`) et totaux du lint.
fn rail(title: &str, inventory: &Inventory, lint: &[Finding]) -> String {
    let count = |s: Severity| lint.iter().filter(|f| f.severity == s).count();
    let flagged = count(Severity::Erreur) + count(Severity::Avertissement);
    let number = |n: usize| format!("<span class=\"count\">{n}</span>");
    let tabs = [
        ("vue", "Vue d'ensemble", "<rect x=\"3\" y=\"3\" width=\"7\" height=\"9\"/><rect x=\"14\" y=\"3\" width=\"7\" height=\"5\"/><rect x=\"14\" y=\"12\" width=\"7\" height=\"9\"/><rect x=\"3\" y=\"16\" width=\"7\" height=\"5\"/>", String::new()),
        ("carte", "Carte du lignage", "<circle cx=\"5\" cy=\"12\" r=\"2\"/><circle cx=\"19\" cy=\"6\" r=\"2\"/><circle cx=\"19\" cy=\"18\" r=\"2\"/><path d=\"M7 12h5l5-6M12 12l5 6\"/>", String::new()),
        // Rempli par columns.js, qui lit le graphe des colonnes.
        ("colonnes", "Colonnes", "<rect x=\"3\" y=\"4\" width=\"18\" height=\"16\" rx=\"1\"/><path d=\"M9 4v16M15 4v16\"/>", "<span class=\"count\" data-column-count></span>".to_owned()),
        ("packages", "Packages", "<path d=\"M3 7l9-4 9 4-9 4-9-4z\"/><path d=\"M3 7v10l9 4 9-4V7\"/>", number(inventory.packages.len())),
        ("qualite", "Qualité", "<path d=\"M12 3l8 4v5c0 5-3.5 8-8 9-4.5-1-8-4-8-9V7l8-4z\"/>", number(flagged)),
    ];
    let links: String = tabs
        .iter()
        .map(|(id, label, icon, extra)| {
            format!(
                "<a href=\"#{id}\" data-tab=\"{id}\"><svg aria-hidden=\"true\" viewBox=\"0 0 24 24\">\
{icon}</svg><span>{label}</span>{extra}</a>"
            )
        })
        .collect();
    format!(
        "<aside class=\"rail\"><div class=\"brand\"><h1>dtsx-inspect</h1>\
<p class=\"path\" title=\"{title}\">{title}</p></div>\
<div class=\"search\" role=\"search\">\
<label for=\"global-search\" class=\"sr-only\">Recherche globale</label>\
<input type=\"search\" id=\"global-search\" role=\"combobox\" aria-expanded=\"false\" \
aria-controls=\"search-results\" aria-autocomplete=\"list\" autocomplete=\"off\" \
placeholder=\"Rechercher… (Ctrl K)\">\
<div id=\"search-results\" class=\"search-results\" role=\"listbox\" aria-label=\"Résultats\" hidden>\
</div></div><nav class=\"tabs\" aria-label=\"Sections du rapport\">{links}</nav>\
<a href=\"#qualite\" class=\"totals\" title=\"Constats de lint\">\
<span class=\"badge erreur\">{}</span><span class=\"badge avertissement\">{}</span></a></aside>",
        plural(count(Severity::Erreur), "erreur"),
        plural(count(Severity::Avertissement), "avertissement"),
    )
}

/// Onglet Colonnes : un champ texte, une base, un projet, un package et les colonnes à vérifier ;
/// les colonnes trouvées à plat ou par table, et à droite la page d'une colonne ou la
/// matrice d'une table. Listes et résultats remplis par columns.js.
fn columns_view() -> String {
    let signals: String = [
        ("sans-origine", "Sans origine connue"),
        ("jamais-lue", "Jamais lue"),
        ("multi-ecrivains", "Écrite par 2+ packages"),
    ]
    .iter()
    .map(|(id, label)| {
        format!(
            "<button type=\"button\" data-signal=\"{id}\" aria-pressed=\"false\">{label} \
             <span class=\"count\"></span></button>"
        )
    })
    .collect();
    format!(
        "<section id=\"colonnes\" class=\"view columns-view\" hidden>\
<div class=\"col-query\">\
<label class=\"col-field col-text\">Chercher<input type=\"search\" id=\"col-filter\" \
autocomplete=\"off\" spellcheck=\"false\" placeholder=\"Nom de colonne ou de table\"></label>\
<label class=\"col-field\">Base<select id=\"col-base\"><option value=\"\">Toutes</option></select></label>\
<label class=\"col-field\">Projet<select id=\"col-project\"><option value=\"\">Tous</option></select></label>\
<label class=\"col-field\">Package<input type=\"search\" id=\"col-package\" list=\"col-packages\" \
autocomplete=\"off\" placeholder=\"Tous\"><datalist id=\"col-packages\"></datalist></label>\
<div class=\"segmented\" role=\"group\" aria-label=\"Affichage\">\
<button type=\"button\" data-layout=\"flat\" aria-pressed=\"true\">À plat</button>\
<button type=\"button\" data-layout=\"tables\" aria-pressed=\"false\">Par table</button></div>\
<button type=\"button\" class=\"col-export\" data-export-mapping title=\"Chaque colonne écrite, \
la colonne lue qui la donne, le package et les transformations : un fichier pour Excel\">\
Exporter le mapping (CSV)</button></div>\
<div class=\"col-filters\"><span class=\"muted\">À vérifier</span>{signals}\
<button type=\"button\" class=\"col-reset\" data-reset hidden>Effacer les filtres</button>\
<span class=\"col-count muted\" aria-live=\"polite\"></span></div>\
<div class=\"col-body\"><div class=\"col-results\"><p class=\"empty\">Le lignage par colonne a \
besoin des scripts du rapport.</p></div><div class=\"col-main\" hidden></div></div></section>"
    )
}

/// Onglet Qualité : les constats de lint, puis une fiche par règle, des erreurs aux
/// infos, avec le nombre de constats du rapport.
fn quality_view(lint: &[Finding], index: &HashMap<&str, usize>) -> String {
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
        "<section id=\"qualite\" class=\"view scroll\" hidden><h2>Qualité</h2>{}\
<h3>Règles de lint</h3><p class=\"muted\">Chaque constat du rapport vient d'une de ces règles. \
Erreur : le package échouera ou fait autre chose que prévu. Avertissement : il marche, mais \
lentement ou de façon fragile. Info : à savoir, sans urgence.</p>\
<div class=\"rules\">{cards}</div></section>",
        lint_table(lint, index)
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

/// Onglet Packages : tous les packages, rangés par projet (pliés d'office), avec leurs
/// constats.
fn packages_view(inventory: &Inventory, lint: &[Finding]) -> String {
    let counts = counts_by_file(lint);
    let mut projects: BTreeMap<&str, Vec<(usize, &Package)>> = BTreeMap::new();
    for (i, p) in inventory.packages.iter().enumerate() {
        projects
            .entry(project_of(&p.file))
            .or_default()
            .push((i, p));
    }
    let list: String = projects
        .iter()
        .map(|(project, packages)| {
            let mut total = [0, 0];
            let items: String = packages
                .iter()
                .map(|(i, p)| {
                    let [e, w] = counts.get(p.file.as_str()).copied().unwrap_or_default();
                    total = [total[0] + e, total[1] + w];
                    format!(
                        "<li><a href=\"{}\" data-pkg=\"{i}\" title=\"{file}\"><span class=\"name\">{}</span>\
<span class=\"file\">{file}</span>{}</a></li>",
                        package_href(*i, p),
                        escape(&p.name),
                        badges(e, w),
                        file = escape(&p.file),
                    )
                })
                .collect();
            let label = if project.is_empty() { "Packages" } else { project };
            format!(
                "<details class=\"project\"><summary><span class=\"name\">{}</span> \
<span class=\"count\">{}</span>{}</summary><ul>{items}</ul></details>",
                escape(label),
                packages.len(),
                badges(total[0], total[1])
            )
        })
        .collect();
    let body = if list.is_empty() {
        "<p class=\"empty\">Aucun package.</p>".to_owned()
    } else {
        list
    };
    format!(
        "<section id=\"packages\" class=\"view scroll\" hidden><h2>Packages</h2>\
<div class=\"toolbar\"><label for=\"package-filter\" class=\"sr-only\">Filtrer les packages</label>\
<input type=\"search\" id=\"package-filter\" placeholder=\"Filtrer par projet, package ou fichier…\">\
</div><div class=\"package-list\">{body}</div></section>"
    )
}

/// Page d'un package : son flux s'il a un Data Flow, sinon son inventaire.
fn package_href(i: usize, p: &Package) -> String {
    let has_flow = p.executables.iter().any(|e| !e.components.is_empty());
    let sub = if has_flow { "flux" } else { "inventaire" };
    format!("#p/{i}/{sub}")
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
    mapping: &BTreeSet<Row>,
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
        format!("<section class=\"panel\"><h3>Fichiers illisibles</h3><ul>{items}</ul></section>")
    };
    format!(
        "<section id=\"vue\" class=\"view scroll\"><div class=\"view-head\"><h2>Vue d'ensemble</h2>\
<button type=\"button\" class=\"primary\" data-export-mapping>Exporter le mapping (CSV)</button></div>\
<div class=\"cards\">{cards}</div><div class=\"panels\">{}{}</div>{}{unreadable}</section>",
        reliability(mapping),
        most_flagged(lint, index),
        projects(inventory),
    )
}

/// Part de chaque statut dans le mapping colonne à colonne : ce que vaut le lignage.
fn reliability(mapping: &BTreeSet<Row>) -> String {
    let total = mapping.len();
    if total == 0 {
        return String::new();
    }
    let counts = STATUSES.map(|s| mapping.iter().filter(|row| row[8] == s).count());
    let share = |n: usize| n as f64 * 100.0 / total as f64;
    let bar: String = counts
        .iter()
        .enumerate()
        .map(|(i, n)| {
            format!(
                "<span class=\"s{i}\" style=\"width: {:.1}%\"></span>",
                share(*n)
            )
        })
        .collect();
    let legend: String = STATUSES
        .iter()
        .zip(counts)
        .enumerate()
        .map(|(i, (status, n))| {
            format!(
                "<li class=\"s{i}\">{}<b>{n}</b><span>{:.0} %</span></li>",
                escape(&capitalized(status)),
                share(n)
            )
        })
        .collect();
    let summary: Vec<String> = STATUSES
        .iter()
        .zip(counts)
        .map(|(status, n)| format!("{n} {status}"))
        .collect();
    let links = if total > 1 { "liens" } else { "lien" };
    format!(
        "<section class=\"panel reliability\"><div class=\"panel-head\"><h3>Fiabilité du lignage</h3>\
<span class=\"muted\">{total} {links} colonne à colonne</span></div>\
<div class=\"meter\" role=\"img\" aria-label=\"{}\">{bar}</div><ul class=\"legend-status\">{legend}</ul>\
<p class=\"muted\">« Par nom » : un <code>INSERT … SELECT *</code>, juste seulement si les colonnes \
des deux tables sont dans le même ordre (<a href=\"#regles/insert-select-etoile\">règle</a>).</p></section>",
        escape(&summary.join(", ")),
    )
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// Projets et leur nombre de packages, du plus gros au plus petit ; chacun ouvre sa carte.
fn projects(inventory: &Inventory) -> String {
    let mut sizes: BTreeMap<&str, usize> = BTreeMap::new();
    for p in &inventory.packages {
        *sizes.entry(project_of(&p.file)).or_default() += 1;
    }
    let mut sizes: Vec<(&str, usize)> = sizes.into_iter().collect();
    sizes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let Some(&(_, largest)) = sizes.first() else {
        return String::new();
    };
    let items: String = sizes
        .iter()
        .map(|(project, n)| {
            let label = if project.is_empty() { "(racine)" } else { project };
            format!(
                "<li><a href=\"#carte/{}\"><span class=\"name\">{}</span>\
<span class=\"bar\"><span style=\"width: {}%\"></span></span><span class=\"muted\">{}</span></a></li>",
                escape(project),
                escape(label),
                n * 100 / largest,
                plural(*n, "package")
            )
        })
        .collect();
    format!("<section class=\"panel\"><h3>Projets</h3><ul class=\"project-sizes\">{items}</ul></section>")
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
        "<section class=\"panel\"><h3>Les plus signalés</h3><table><thead><tr><th>Fichier</th>\
<th>Erreurs</th><th>Avertissements</th></tr></thead><tbody>{rows}</tbody></table></section>"
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
    fn the_columns_tab_downloads_the_same_mapping_as_the_cli() {
        let (inventory, _) = crate::inventory(std::path::Path::new("tests/fixtures/colonnes_sql"));
        let csv = crate::render::mapping_csv(&inventory, &column_lineage(&inventory));

        let page = html("x", &inventory, &Lineage::default(), &[], &[]);

        assert!(page.contains("data-export-mapping"), "bouton d'export");
        let open = "<template id=\"mapping-csv\">";
        let start = page.find(open).expect("CSV embarqué") + open.len();
        let embedded = &page[start..start + page[start..].find("</template>").expect("fin")];
        // Le navigateur ramène `\r\n` à `\n` et peut perdre le BOM : seules des
        // références de caractères les gardent intacts.
        assert!(!embedded.contains(['\r', '\u{feff}']), "{embedded}");
        let decoded = embedded
            .replace("&#13;", "\r")
            .replace("&#xFEFF;", "\u{feff}")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&amp;", "&");
        assert_eq!(decoded, csv);
    }

    #[test]
    fn the_header_offers_a_global_search_and_a_tab_per_space() {
        let page = html("x", &Inventory::default(), &Lineage::default(), &[], &[]);

        assert!(page.contains("id=\"global-search\""), "recherche globale");
        for tab in ["#vue", "#carte", "#colonnes", "#packages", "#qualite"] {
            assert!(
                page.contains(&format!("<a href=\"{tab}\" data-tab=")),
                "onglet {tab}"
            );
        }
        assert!(page.contains("id=\"colonnes\""), "espace Colonnes");
        assert!(!page.contains("#appels"), "plus d'onglet Orchestration");
    }

    #[test]
    fn a_side_rail_holds_the_search_the_sections_and_the_lint_totals() {
        let page = html("x", &Inventory::default(), &Lineage::default(), &[], &[]);

        let start = page
            .find("<aside class=\"rail\"")
            .expect("rail de navigation");
        let rail = &page[start..start + page[start..].find("</aside>").expect("fin du rail")];
        for part in [
            "id=\"global-search\"",
            "<nav class=\"tabs\"",
            "class=\"totals\"",
        ] {
            assert!(rail.contains(part), "{part} dans le rail\n{rail}");
        }
        // Sans internet, les polices système prennent le relais.
        assert!(page.contains("family=IBM+Plex+Sans"), "police du rapport");
    }

    #[test]
    fn the_overview_shows_how_reliable_the_column_lineage_is() {
        let (inventory, _) = crate::inventory(std::path::Path::new("tests/fixtures/colonnes_sql"));

        let page = html("x", &inventory, &Lineage::default(), &[], &[]);

        let view = &page[page.find("id=\"vue\"").expect("vue d'ensemble")..];
        assert!(view.contains("Fiabilité du lignage"), "{view}");
        // 8 liens : 3 directs, 1 transformé, 4 par nom, aucun sans origine.
        for item in [
            "Directe<b>3</b>",
            "Transformée<b>1</b>",
            "Par nom (SELECT *)<b>4</b>",
            "Origine inconnue<b>0</b>",
        ] {
            assert!(view.contains(item), "{item}\n{view}");
        }
    }

    #[test]
    fn the_overview_lists_projects_with_a_link_to_their_map() {
        let (inventory, _) = crate::inventory(std::path::Path::new("tests/fixtures"));

        let page = html("x", &inventory, &Lineage::default(), &[], &[]);

        let view = &page[page.find("id=\"vue\"").expect("vue d'ensemble")..];
        assert!(view.contains("href=\"#carte/colonnes_sql\""), "{view}");
    }

    #[test]
    fn the_columns_tab_has_a_search_field_simple_filters_and_two_layouts() {
        let page = html("x", &Inventory::default(), &Lineage::default(), &[], &[]);

        let view = &page[page.find("id=\"colonnes\"").expect("onglet Colonnes")..];
        for field in [
            "col-filter",
            "col-base",
            "col-project",
            "col-package",
            "col-packages",
        ] {
            assert!(view.contains(&format!("id=\"{field}\"")), "{field}");
        }
        assert!(
            !view.contains("<code>pkg:</code>"),
            "pas de syntaxe à apprendre"
        );
        for signal in ["sans-origine", "jamais-lue", "multi-ecrivains"] {
            assert!(
                view.contains(&format!("data-signal=\"{signal}\"")),
                "{signal}"
            );
        }
        for layout in ["flat", "tables"] {
            assert!(
                view.contains(&format!("data-layout=\"{layout}\"")),
                "{layout}"
            );
        }
    }

    #[test]
    fn the_quality_tab_explains_every_rule() {
        let page = html("x", &Inventory::default(), &Lineage::default(), &[], &[]);

        let view = &page[page.find("id=\"qualite\"").expect("onglet Qualité")..];
        for doc in crate::lint::rules() {
            assert!(
                view.contains(&format!("id=\"regle-{}\"", doc.id)),
                "{}",
                doc.id
            );
            assert!(view.contains(&escape(doc.fix)), "{}", doc.id);
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
    fn the_packages_tab_lists_every_package_by_project() {
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

        let view = packages_view(&inventory, &[]);

        assert!(view.contains(">Demo_Minimal<"), "{view}");
        assert!(view.contains("href=\"#p/0/flux\""), "{view}");
        // Sans Data Flow, l'orchestrateur est listé et s'ouvre sur son inventaire.
        assert!(view.contains(">Orchestrateur<"), "{view}");
        assert!(view.contains("href=\"#p/1/inventaire\""), "{view}");
        // Un projet par bloc pliable, plié d'office pour alléger la page.
        assert!(
            view.contains("<details class=\"project\"><summary>"),
            "{view}"
        );
        assert!(!view.contains("<details class=\"project\" open"), "{view}");
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
