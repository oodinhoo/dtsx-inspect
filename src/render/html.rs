//! Standalone HTML report. At the top, the global search and one tab per space
//! (overview, lineage map, columns, packages, quality); the page of a package has its own
//! tabs (flow, inventory, lint). Formatted by the browser (ELK for the graph, marked for
//! the Markdown). Written in English; `i18n.js` switches it to French.

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
const I18N: &str = include_str!("i18n.js");
const SCRIPT: &str = include_str!("report.js");
const LINEAGE: &str = include_str!("lineage.js");
const COLUMNS: &str = include_str!("columns.js");
const SEARCH: &str = include_str!("search.js");
// shortcut: libraries loaded from the CDN; embed them (`include_str!`) if the report must
// open without internet. Without them, navigation and filters still work.
// `integrity`: hash of the pinned version, to recompute when it changes.
const LIBRARIES: &str = r#"<script src="https://cdn.jsdelivr.net/npm/elkjs@0.9.3/lib/elk.bundled.js" integrity="sha384-KO35c6PFq7tFxAEx6FtATwL9vOBOmCzQYr7GM0iTr3oO22s7Jc9OkclOH4AV7jwK" crossorigin="anonymous"></script>
<script src="https://cdn.jsdelivr.net/npm/marked@12.0.2/marked.min.js" integrity="sha384-/TQbtLCAerC3jgaim+N78RZSDYV7ryeoBCVqTuzRrFec2akfBkHS7ACQ3PQhvMVi" crossorigin="anonymous"></script>"#;
// IBM Plex from Google Fonts; without internet, the system fonts of `report.css`.
const FONTS: &str = "<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\
<link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500\
&amp;family=IBM+Plex+Sans:wght@400;500;600;700&amp;display=swap\">";
const SEVERITIES: [Severity; 3] = [Severity::Error, Severity::Warning, Severity::Info];
/// Packages listed in "Most flagged".
const TOP_PACKAGES: usize = 10;
/// Graph shared by the map and the flow of a package: the script moves it into the
/// shown view.
const STAGE: &str = "<div class=\"stage\"><div class=\"canvas\"><div class=\"world\">\
<svg class=\"wires\"></svg></div>\
<p class=\"empty offline\">Graph unavailable: the layout library could not be loaded \
(no internet access?).</p>\
<p class=\"empty no-block\" hidden>No dataset or traced step for this package.</p></div>\
<input type=\"search\" class=\"stage-search\" placeholder=\"Search the graph… (Enter)\">\
<div class=\"zoom\"><button data-zoom=\"in\" title=\"Zoom in\">+</button>\
<button data-zoom=\"out\" title=\"Zoom out\">−</button>\
<button data-zoom=\"fit\" title=\"Fit to screen (F)\">⤢</button></div>\
<details class=\"legend\" open><summary>Legend</summary><div class=\"legend-body\"></div></details>\
<aside class=\"inspector\" aria-label=\"Details\"></aside></div>";

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
    // Same CSV as `dtsx-inspect mapping`, downloaded by the export buttons.
    // The browser would turn `\r\n` into `\n` and drop the BOM: character references.
    let mapping = escape(&to_csv(&rows))
        .replace('\r', "&#13;")
        .replace('\u{feff}', "&#xFEFF;");
    // i18n.js first: it translates what the other scripts render.
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>dtsx-inspect: {title}</title>{FONTS}<style>{STYLE}</style></head><body><div class=\"app\">\
{}<main class=\"content\">{}{}{}{}{}{}</main></div>{templates}\
<template id=\"mapping-csv\">{mapping}</template>\
<script type=\"application/json\" id=\"lineage-data\">{}</script>\
<script>{I18N}</script>{LIBRARIES}<script>{LINEAGE}</script><script>{COLUMNS}</script>\
<script>{SEARCH}</script><script>{SCRIPT}</script></body></html>\n",
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

/// Navigation rail: analyzed folder, language, global search (Ctrl K), one link per
/// space (report.js marks the current one, `aria-current`) and the lint totals.
fn rail(title: &str, inventory: &Inventory, lint: &[Finding]) -> String {
    let count = |s: Severity| lint.iter().filter(|f| f.severity == s).count();
    let flagged = count(Severity::Error) + count(Severity::Warning);
    let number = |n: usize| format!("<span class=\"count\">{n}</span>");
    let tabs = [
        ("overview", "Overview", "<rect x=\"3\" y=\"3\" width=\"7\" height=\"9\"/><rect x=\"14\" y=\"3\" width=\"7\" height=\"5\"/><rect x=\"14\" y=\"12\" width=\"7\" height=\"9\"/><rect x=\"3\" y=\"16\" width=\"7\" height=\"5\"/>", String::new()),
        ("map", "Lineage map", "<circle cx=\"5\" cy=\"12\" r=\"2\"/><circle cx=\"19\" cy=\"6\" r=\"2\"/><circle cx=\"19\" cy=\"18\" r=\"2\"/><path d=\"M7 12h5l5-6M12 12l5 6\"/>", String::new()),
        // Filled by columns.js, which reads the column graph.
        ("columns", "Columns", "<rect x=\"3\" y=\"4\" width=\"18\" height=\"16\" rx=\"1\"/><path d=\"M9 4v16M15 4v16\"/>", "<span class=\"count\" data-column-count></span>".to_owned()),
        ("packages", "Packages", "<path d=\"M3 7l9-4 9 4-9 4-9-4z\"/><path d=\"M3 7v10l9 4 9-4V7\"/>", number(inventory.packages.len())),
        ("quality", "Quality", "<path d=\"M12 3l8 4v5c0 5-3.5 8-8 9-4.5-1-8-4-8-9V7l8-4z\"/>", number(flagged)),
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
<p class=\"path\" title=\"{title}\" translate=\"no\">{title}</p>\
<div class=\"lang\" role=\"group\" aria-label=\"Language\">\
<button type=\"button\" data-lang=\"en\" lang=\"en\" aria-pressed=\"true\">EN</button>\
<button type=\"button\" data-lang=\"fr\" lang=\"fr\" aria-pressed=\"false\">FR</button></div></div>\
<div class=\"search\" role=\"search\">\
<label for=\"global-search\" class=\"sr-only\">Global search</label>\
<input type=\"search\" id=\"global-search\" role=\"combobox\" aria-expanded=\"false\" \
aria-controls=\"search-results\" aria-autocomplete=\"list\" autocomplete=\"off\" \
placeholder=\"Search… (Ctrl K)\">\
<div id=\"search-results\" class=\"search-results\" role=\"listbox\" aria-label=\"Results\" hidden>\
</div></div><nav class=\"tabs\" aria-label=\"Report sections\">{links}</nav>\
<a href=\"#quality\" class=\"totals\" title=\"Lint findings\">\
<span class=\"badge error\">{}</span><span class=\"badge warning\">{}</span></a></aside>",
        plural(count(Severity::Error), "error"),
        plural(count(Severity::Warning), "warning"),
    )
}

/// Columns tab: a text field, a database, a project, a package and the columns to check;
/// the columns found flat or by table, and on the right the page of a column or the
/// matrix of a table. Lists and results filled by columns.js.
fn columns_view() -> String {
    let signals: String = [
        ("no-origin", "No known origin"),
        ("never-read", "Never read"),
        ("many-writers", "Written by 2+ packages"),
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
        "<section id=\"columns\" class=\"view columns-view\" hidden>\
<div class=\"col-query\">\
<label class=\"col-field col-text\">Search<input type=\"search\" id=\"col-filter\" \
autocomplete=\"off\" spellcheck=\"false\" placeholder=\"Column or table name\"></label>\
<label class=\"col-field\">Database<select id=\"col-base\"><option value=\"\">All</option></select></label>\
<label class=\"col-field\">Project<select id=\"col-project\"><option value=\"\">All</option></select></label>\
<label class=\"col-field\">Package<input type=\"search\" id=\"col-package\" list=\"col-packages\" \
autocomplete=\"off\" placeholder=\"All\"><datalist id=\"col-packages\"></datalist></label>\
<div class=\"segmented\" role=\"group\" aria-label=\"Layout\">\
<button type=\"button\" data-layout=\"flat\" aria-pressed=\"true\">Flat</button>\
<button type=\"button\" data-layout=\"tables\" aria-pressed=\"false\">By table</button></div>\
<button type=\"button\" class=\"col-export\" data-export-mapping title=\"Each written column, \
the column read that gives it, the package and the transformations: one file for Excel\">\
Export the mapping (CSV)</button></div>\
<div class=\"col-filters\"><span class=\"muted\">To check</span>{signals}\
<button type=\"button\" class=\"col-reset\" data-reset hidden>Clear the filters</button>\
<span class=\"col-count muted\" aria-live=\"polite\"></span></div>\
<div class=\"col-body\"><div class=\"col-results\"><p class=\"empty\">The column lineage \
needs the report scripts.</p></div><div class=\"col-main\" hidden></div></div></section>"
    )
}

/// Quality tab: the lint findings, then one card per rule, from errors to infos, with
/// the number of findings of the report.
fn quality_view(lint: &[Finding], index: &HashMap<&str, usize>) -> String {
    let mut docs: Vec<&RuleDoc> = rules().collect();
    docs.sort_by_key(|d| (SEVERITIES.iter().position(|s| *s == d.severity), d.id));
    let cards: String = docs
        .iter()
        .map(|d| {
            let count = lint.iter().filter(|f| f.rule == d.id).count();
            let found = if count == 0 {
                "<span class=\"muted\">No finding in this report</span>".to_owned()
            } else {
                format!(
                    "<button class=\"action\" data-show-rule=\"{}\" data-rule-severity=\"{}\">\
See {}</button>",
                    escape(d.id),
                    d.severity,
                    plural(count, "finding")
                )
            };
            format!(
                "<article class=\"rule\" id=\"rule-{id}\"><header>\
<span class=\"badge {s}\">{s}</span><h3>{}</h3><code>{id}</code></header>\
<dl><dt>Why</dt><dd>{}</dd><dt>Fix</dt><dd>{}</dd></dl>{found}</article>",
                escape(d.title),
                escape(d.why),
                escape(d.fix),
                id = escape(d.id),
                s = d.severity,
            )
        })
        .collect();
    format!(
        "<section id=\"quality\" class=\"view scroll\" hidden><h2>Quality</h2>{}\
<h3>Lint rules</h3><p class=\"muted\">Each finding of the report comes from one of these rules. \
Error: the package will fail or does something other than intended. Warning: it works, but \
slowly or in a fragile way. Info: good to know, not urgent.</p>\
<div class=\"rules\">{cards}</div></section>",
        lint_table(lint, index)
    )
}

/// Project of a package: the folder containing it (`FINANCE/FINANCE/x.dtsx`), the one
/// above `obj` or `bin` for a build copy.
pub(super) fn project_of(file: &str) -> &str {
    let mut dirs: Vec<&str> = file.split('/').collect();
    dirs.pop();
    if dirs.len() >= 2 && dirs.get(dirs.len() - 2).is_some_and(|d| is_build_dir(d)) {
        dirs.truncate(dirs.len() - 2);
    }
    dirs.last().copied().unwrap_or("")
}

/// Lint errors and warnings per file.
fn counts_by_file(lint: &[Finding]) -> HashMap<&str, [usize; 2]> {
    let mut counts: HashMap<&str, [usize; 2]> = HashMap::new();
    for f in lint {
        let slot = match f.severity {
            Severity::Error => 0,
            Severity::Warning => 1,
            Severity::Info => continue,
        };
        counts.entry(&f.file).or_default()[slot] += 1;
    }
    counts
}

/// Packages tab: every package, grouped by project (folded by default), with their
/// findings.
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
        "<p class=\"empty\">No package.</p>".to_owned()
    } else {
        list
    };
    format!(
        "<section id=\"packages\" class=\"view scroll\" hidden><h2>Packages</h2>\
<div class=\"toolbar\"><label for=\"package-filter\" class=\"sr-only\">Filter the packages</label>\
<input type=\"search\" id=\"package-filter\" placeholder=\"Filter by project, package or file…\">\
</div><div class=\"package-list\" translate=\"no\">{body}</div></section>"
    )
}

/// Page of a package: its flow when it has a Data Flow, otherwise its inventory.
fn package_href(i: usize, p: &Package) -> String {
    let has_flow = p.executables.iter().any(|e| !e.components.is_empty());
    let sub = if has_flow { "flow" } else { "inventory" };
    format!("#p/{i}/{sub}")
}

fn badges(errors: usize, warnings: usize) -> String {
    badge(errors, Severity::Error) + &badge(warnings, Severity::Warning)
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
        (connections, "connections", ""),
        (executables().count(), "tasks and containers", ""),
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
            "third-party components",
            "",
        ),
        (count(Severity::Error), "lint errors", "error"),
        (count(Severity::Warning), "warnings", "warning"),
        (errors.len(), "unreadable files", "error"),
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
        format!("<section class=\"panel\"><h3>Unreadable files</h3><ul>{items}</ul></section>")
    };
    format!(
        "<section id=\"overview\" class=\"view scroll\"><div class=\"view-head\"><h2>Overview</h2>\
<button type=\"button\" class=\"primary\" data-export-mapping>Export the mapping (CSV)</button></div>\
<div class=\"cards\">{cards}</div><div class=\"panels\">{}{}</div>{}{unreadable}</section>",
        reliability(mapping),
        most_flagged(lint, index),
        projects(inventory),
    )
}

/// Share of each status in the column-to-column mapping: what the lineage is worth.
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
    format!(
        "<section class=\"panel reliability\"><div class=\"panel-head\"><h3>Lineage reliability</h3>\
<span class=\"muted\">{} column to column</span></div>\
<div class=\"meter\" role=\"img\" aria-label=\"{}\">{bar}</div><ul class=\"legend-status\">{legend}</ul>\
<p class=\"muted\">\"By name\": an <code>INSERT … SELECT *</code>, right only if the columns \
of both tables are in the same order (<a href=\"#rules/insert-select-star\">rule</a>).</p></section>",
        plural(total, "link"),
        escape(&summary.join(", ")),
    )
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// Projects and their number of packages, from the largest to the smallest; each opens
/// its map.
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
            let label = if project.is_empty() { "(root)" } else { project };
            format!(
                "<li><a href=\"#map/{}\"><span class=\"name\">{}</span>\
<span class=\"bar\"><span style=\"width: {}%\"></span></span><span class=\"muted\">{}</span></a></li>",
                escape(project),
                escape(label),
                n * 100 / largest,
                plural(*n, "package")
            )
        })
        .collect();
    format!("<section class=\"panel\"><h3>Projects</h3><ul class=\"project-sizes\">{items}</ul></section>")
}

/// Packages having the most errors, then warnings.
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
                "<tr><td translate=\"no\">{}</td><td>{}</td><td>{}</td></tr>",
                file_link(file, index, "lint"),
                badge(*errors, Severity::Error),
                badge(*warnings, Severity::Warning)
            )
        })
        .collect();
    format!(
        "<section class=\"panel\"><h3>Most flagged</h3><table><thead><tr><th>File</th>\
<th>Errors</th><th>Warnings</th></tr></thead><tbody>{rows}</tbody></table></section>"
    )
}

/// Link to the page of a package; plain text for `project` (project connections).
fn file_link(file: &str, index: &HashMap<&str, usize>, sub: &str) -> String {
    match index.get(file) {
        Some(i) => format!("<a href=\"#p/{i}/{sub}\">{}</a>", breakable(file)),
        None => breakable(file),
    }
}

/// Escaped path, breakable after each separator rather than in the middle of a name.
fn breakable(path: &str) -> String {
    escape(path).replace('/', "/<wbr>").replace('\\', "\\<wbr>")
}

fn lint_table(lint: &[Finding], index: &HashMap<&str, usize>) -> String {
    if lint.is_empty() {
        return "<h3>Lint findings</h3><p class=\"empty\">No finding.</p>".to_owned();
    }
    // Infos (lineage blind spots) are hidden by default: often numerous.
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
<td><a href=\"#rules/{id}\" title=\"See the rule\">{id}</a></td><td translate=\"no\">{}</td>\
<td translate=\"no\">{}</td><td>{}</td></tr>",
                file_link(&f.file, index, "flow"),
                breakable(&f.location),
                escape(&f.message),
                s = f.severity,
                id = escape(f.rule),
            )
        })
        .collect();
    format!(
        "<h3>Lint findings</h3><div class=\"toolbar\">{filters}\
<input type=\"search\" placeholder=\"Filter by file, task, message…\" data-filter=\"lint-rows\">\
</div><table class=\"lint hide-info\"><thead><tr><th>Severity</th><th>Rule</th><th>File</th>\
<th>Location</th><th>Message</th></tr></thead><tbody id=\"lint-rows\">{rows}</tbody></table>"
    )
}

fn map_view() -> String {
    format!(
        "<section id=\"map\" class=\"view\" hidden><div class=\"view-head\"><h2>Lineage map</h2>\
<select class=\"project-select\" aria-label=\"Project\"></select></div>\
<div class=\"stage-slot\">{STAGE}</div></section>"
    )
}

fn package_view() -> String {
    "<section id=\"package\" class=\"view\" hidden><div class=\"view-head\"><div>\
<p class=\"crumb\" translate=\"no\"></p><h2 class=\"pkg-name\" translate=\"no\"></h2>\
<p class=\"muted pkg-meta\"></p></div>\
<nav class=\"subtabs\" aria-label=\"Package views\"><a data-sub=\"flow\">Flow</a>\
<a data-sub=\"inventory\">Inventory</a><a data-sub=\"lint\">Lint <span class=\"count\"></span></a>\
</nav></div>\
<div class=\"sub stage-slot\" data-sub=\"flow\"></div>\
<div class=\"sub scroll md-host\" data-sub=\"inventory\"></div>\
<div class=\"sub scroll\" data-sub=\"lint\"><table class=\"lint pkg-lint\"><thead><tr><th>Severity</th>\
<th>Rule</th><th>File</th><th>Location</th><th>Message</th></tr></thead><tbody></tbody></table>\
<p class=\"empty\" hidden>No finding for this package.</p></div></section>"
        .to_owned()
}

/// Inventory of a package, as Markdown, and its metadata: copied into the package page
/// when it opens.
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
            plural(tasks, "task"),
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
        let (inventory, _) = crate::inventory(std::path::Path::new("tests/fixtures/columns_sql"));
        let csv = crate::render::mapping_csv(&inventory, &column_lineage(&inventory));

        let page = html("x", &inventory, &Lineage::default(), &[], &[]);

        assert!(page.contains("data-export-mapping"), "export button");
        let open = "<template id=\"mapping-csv\">";
        let start = page.find(open).expect("embedded CSV") + open.len();
        let embedded = &page[start..start + page[start..].find("</template>").expect("end")];
        // The browser turns `\r\n` into `\n` and can drop the BOM: only character
        // references keep them intact.
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

        assert!(page.contains("id=\"global-search\""), "global search");
        for tab in ["#overview", "#map", "#columns", "#packages", "#quality"] {
            assert!(
                page.contains(&format!("<a href=\"{tab}\" data-tab=")),
                "tab {tab}"
            );
        }
        assert!(page.contains("id=\"columns\""), "Columns space");
        assert!(!page.contains("#calls"), "no Orchestration tab anymore");
    }

    #[test]
    fn a_side_rail_holds_the_search_the_sections_and_the_lint_totals() {
        let page = html("x", &Inventory::default(), &Lineage::default(), &[], &[]);

        let start = page.find("<aside class=\"rail\"").expect("navigation rail");
        let rail = &page[start..start + page[start..].find("</aside>").expect("end of rail")];
        for part in [
            "id=\"global-search\"",
            "<nav class=\"tabs\"",
            "class=\"totals\"",
        ] {
            assert!(rail.contains(part), "{part} in the rail\n{rail}");
        }
        // Without internet, the system fonts take over.
        assert!(page.contains("family=IBM+Plex+Sans"), "report font");
    }

    #[test]
    fn the_report_is_in_english_and_offers_french() {
        let page = html("x", &Inventory::default(), &Lineage::default(), &[], &[]);

        assert!(page.contains("<html lang=\"en\">"), "English by default");
        for lang in ["en", "fr"] {
            assert!(
                page.contains(&format!("data-lang=\"{lang}\"")),
                "{lang} button"
            );
        }
        // The translation runs before the scripts that render text.
        let i18n = page.find(I18N).expect("i18n script");
        assert!(i18n < page.find(LINEAGE).expect("lineage script"));
    }

    #[test]
    fn the_overview_shows_how_reliable_the_column_lineage_is() {
        let (inventory, _) = crate::inventory(std::path::Path::new("tests/fixtures/columns_sql"));

        let page = html("x", &inventory, &Lineage::default(), &[], &[]);

        let view = &page[page.find("id=\"overview\"").expect("overview")..];
        assert!(view.contains("Lineage reliability"), "{view}");
        // 8 links: 3 direct, 1 transformed, 4 by name, none without origin.
        for item in [
            "Direct<b>3</b>",
            "Transformed<b>1</b>",
            "By name (SELECT *)<b>4</b>",
            "Unknown origin<b>0</b>",
        ] {
            assert!(view.contains(item), "{item}\n{view}");
        }
    }

    #[test]
    fn the_overview_lists_projects_with_a_link_to_their_map() {
        let (inventory, _) = crate::inventory(std::path::Path::new("tests/fixtures"));

        let page = html("x", &inventory, &Lineage::default(), &[], &[]);

        let view = &page[page.find("id=\"overview\"").expect("overview")..];
        assert!(view.contains("href=\"#map/columns_sql\""), "{view}");
    }

    #[test]
    fn the_columns_tab_has_a_search_field_simple_filters_and_two_layouts() {
        let page = html("x", &Inventory::default(), &Lineage::default(), &[], &[]);

        let view = &page[page.find("id=\"columns\"").expect("Columns tab")..];
        for field in [
            "col-filter",
            "col-base",
            "col-project",
            "col-package",
            "col-packages",
        ] {
            assert!(view.contains(&format!("id=\"{field}\"")), "{field}");
        }
        assert!(!view.contains("<code>pkg:</code>"), "no syntax to learn");
        for signal in ["no-origin", "never-read", "many-writers"] {
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

        let view = &page[page.find("id=\"quality\"").expect("Quality tab")..];
        for doc in crate::lint::rules() {
            assert!(
                view.contains(&format!("id=\"rule-{}\"", doc.id)),
                "{}",
                doc.id
            );
            assert!(view.contains(&escape(doc.fix)), "{}", doc.id);
        }
    }

    #[test]
    fn each_finding_links_to_its_rule() {
        let finding = Finding {
            rule: "sort-in-data-flow",
            severity: Severity::Warning,
            file: "p.dtsx".into(),
            location: "DFT\\Sort".into(),
            message: "m".into(),
        };

        let table = lint_table(&[finding], &HashMap::new());

        assert!(
            table.contains("href=\"#rules/sort-in-data-flow\""),
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
        assert!(view.contains("href=\"#p/0/flow\""), "{view}");
        // Without a Data Flow, the orchestrator is listed and opens on its inventory.
        assert!(view.contains(">Orchestrateur<"), "{view}");
        assert!(view.contains("href=\"#p/1/inventory\""), "{view}");
        // One foldable block per project, folded by default to lighten the page.
        assert!(
            view.contains("<details class=\"project\"><summary>"),
            "{view}"
        );
        assert!(!view.contains("<details class=\"project\" open"), "{view}");
    }

    #[test]
    fn external_scripts_are_pinned_by_their_hash() {
        // A file modified on the CDN would read the whole report: the browser refuses it.
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
        // Folder analyzed higher up: the project stays the package folder.
        assert_eq!(
            project_of("Packages SSIS/SALES/SALES/21_Diff.dtsx"),
            "SALES"
        );
        assert_eq!(
            project_of("Packages SSIS/DW_SALES_ONLY/01.dtsx"),
            "DW_SALES_ONLY"
        );
        // Build copy only (Demo): the project is above `obj`.
        assert_eq!(project_of("Demo/Demo/obj/Development/01.dtsx"), "Demo");
    }
}
