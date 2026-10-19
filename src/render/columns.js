// Onglet Colonnes : un champ texte, une base, un projet, un package et les colonnes à vérifier ;
// les colonnes trouvées à plat (tableau triable) ou par table, et à droite la page d'une
// colonne (d'où elle vient, où elle va, packages) ou la matrice d'une table (quel package
// écrit, lit ou calcule chaque colonne). Adresses : #colonnes, #colonnes/c/COLONNE,
// #colonnes/t/BLOC, suivies des filtres (?q=…&base=…&projet=…&pkg=…&signal=…).
// Données et échappement : lineage.js.
// Les textes viennent des packages : jamais insérés en HTML sans `esc`.
(() => {
  const view = document.getElementById("colonnes");
  const lineage = window.lineage;
  if (!view || !lineage) return;
  const { data, esc, tile, columnsIn, columnsOut } = lineage;
  const results = view.querySelector(".col-results");
  const main = view.querySelector(".col-main");
  const filter = view.querySelector("#col-filter");
  const baseSelect = view.querySelector("#col-base");
  const projectSelect = view.querySelector("#col-project");
  const packageInput = view.querySelector("#col-package");
  const packageList = view.querySelector("#col-packages");
  const reset = view.querySelector(".col-reset");
  const counter = view.querySelector(".col-count");
  const DATASETS = ["table", "file", "entity", "proc"];
  const GROUPS = [["table", "Tables"], ["file", "Fichiers"], ["entity", "Entités Dynamics"]];
  // Au-delà, la liste demande d'affiner : des milliers de lignes ou de tables rendraient
  // chaque frappe lente.
  const ROW_LIMIT = 200;
  const TREE_LIMIT = 200;
  // Comme la trace du panneau de détails : une colonne très partagée a des milliers
  // de descendants.
  const STEP_LIMIT = 300;
  const MARKS = { W: ["É", "écrit", "w"], R: ["L", "lit", "r"], C: ["C", "calcule à partir de", "c"] };
  const NUMERIC = ["writers", "readers", "sources", "targets"];
  const HEADS = [
    ["name", "Colonne"], ["table", "Table"], ["base", "Base · serveur"], ["writers", "Écrite par"],
    ["readers", "Lue par"], ["sources", "←", "Origines directes"], ["targets", "→", "Usages directs"],
  ];

  const collator = new Intl.Collator("fr", { numeric: true });
  const name = (c) => data.columns[c][0];
  const full = (c) => data.columns[c][1];
  const isStep = (c) => data.columns[c][2];
  const byName = (a, b) => name(a).localeCompare(name(b));
  const plural = (n, word, many = `${word}s`) => `${n} ${n > 1 ? many : word}`;
  // Sans casse ni accents : `entité` trouve `entite`.
  const fold = (text) => text.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
  // Bloc qui porte chaque colonne : table ou fichier, sinon le composant qui la produit.
  const owner = new Map();
  data.nodes.forEach((n) => (n.columns ?? []).forEach((c) => owner.set(c, n)));
  const datasets = data.nodes
    .filter((n) => DATASETS.includes(n.kind) && n.columns?.length)
    .sort((a, b) => `${a.sub} ${a.title}`.localeCompare(`${b.sub} ${b.title}`));
  const listedSets = datasets.filter((d) => GROUPS.some(([kind]) => kind === d.kind));
  // `Table · Entrepot · srv` : la base et le serveur, le dossier ou la connexion.
  const where = (d) => d.sub.split(" · ").slice(1).join(" · ");

  const packageLink = (p) => {
    // Même page que l'onglet Packages : le flux, ou l'inventaire sans Data Flow.
    const href = document.querySelector(`#packages a[data-pkg="${p}"]`)?.getAttribute("href") ?? `#p/${p}`;
    return `<a href="${esc(href)}">${esc(data.packages[p] ?? "?")}</a>`;
  };
  const project = (p) => data.projects[p] || "(racine)";
  const uses = (c) => data.uses?.[c] ?? [];
  const packageKeys = data.packages.map(fold);

  // Une ligne par colonne de table, fichier ou entité, avec ce que lisent les filtres.
  const rows = listedSets.flatMap((d) => d.columns.map((c) => {
    const marks = uses(c);
    const writers = [...new Set(marks.filter(([, m]) => m === "W").map(([p]) => p))];
    const readers = [...new Set(marks.filter(([, m]) => m !== "W").map(([p]) => p))];
    const sources = columnsIn.get(c)?.length ?? 0;
    const targets = columnsOut.get(c)?.length ?? 0;
    const signals = [sources === 0 && "sans-origine", targets === 0 && "jamais-lue", writers.length > 1 && "multi-ecrivains"];
    return {
      c, d, marks, writers, readers, sources, targets, where: where(d),
      signals: signals.filter(Boolean),
      text: fold(`${name(c)} ${d.title} ${d.title}.${name(c)}`),
    };
  })).sort((a, b) => collator.compare(name(a.c), name(b.c)) || collator.compare(a.d.title, b.d.title));
  const tabCount = document.querySelector("[data-column-count]");
  if (tabCount) tabCount.textContent = rows.length;
  view.querySelectorAll("[data-signal]").forEach((button) => {
    button.querySelector(".count").textContent = rows.filter((r) => r.signals.includes(button.dataset.signal)).length;
  });

  // Listes des bases (par genre de jeu de données), des projets et des packages.
  const sortedSet = (values) => [...new Set(values)].sort(collator.compare);
  // Ce que la liste propose pour chaque genre : la base et son serveur, le dossier, la connexion.
  const PLACES = [["table", "Bases de données · serveur"], ["file", "Dossiers de fichiers"], ["entity", "Connexions Dynamics"]];
  baseSelect.insertAdjacentHTML("beforeend", PLACES.map(([kind, title]) => {
    const values = sortedSet(listedSets.filter((d) => d.kind === kind).map(where).filter(Boolean));
    return values.length ? `<optgroup label="${title}">${values.map((v) => `<option>${esc(v)}</option>`).join("")}</optgroup>` : "";
  }).join(""));
  // Un seul projet : la liste ne filtrerait rien.
  projectSelect.closest("label").hidden = sortedSet(data.packages.map((_, p) => project(p))).length < 2;

  // Packages qui touchent chaque base : écrivent, lisent ou calculent une de ses colonnes.
  const packagesOf = new Map();
  for (const r of rows) {
    for (const [p] of r.marks) (packagesOf.get(r.where) ?? packagesOf.set(r.where, new Set()).get(r.where)).add(p);
  }
  const inBase = (p) => !baseSelect.value || (packagesOf.get(baseSelect.value)?.has(p) ?? false);
  const inProject = (p) => !projectSelect.value || project(p) === projectSelect.value;

  // Projets proposés : ceux qui ont un package sur la base choisie, sinon tous.
  function fillProjects() {
    const chosen = projectSelect.value;
    const projects = sortedSet(data.packages.map((_, p) => p).filter(inBase).map(project));
    projectSelect.replaceChildren(new Option("Tous", ""), ...projects.map((p) => new Option(p, p)));
    projectSelect.value = projects.includes(chosen) ? chosen : "";
  }
  // Packages proposés : ceux de la base et du projet choisis, sinon tous.
  function fillPackages() {
    const names = sortedSet(data.packages.filter((_, p) => inBase(p) && inProject(p)));
    packageList.innerHTML = names.map((n) => `<option value="${esc(n)}"></option>`).join("");
  }
  // Autre base ou autre projet : listes resserrées, et ce qui n'y est plus revient à « Tous ».
  function narrow() {
    fillProjects();
    fillPackages();
    const pkg = fold(packageInput.value.trim());
    const offered = (p) => inBase(p) && inProject(p) && packageKeys[p].includes(pkg);
    if (pkg && !data.packages.some((_, p) => offered(p))) packageInput.value = "";
  }
  fillProjects();
  fillPackages();

  // ---------- Filtres ----------
  const signals = new Set();

  // Chaque mot du champ texte dans le nom de la colonne ou de sa table, la base choisie,
  // un package (du projet choisi) qui touche la colonne, et chaque signal coché.
  function test(row, words, base, pkg) {
    const scoped = pkg || projectSelect.value;
    return words.every((w) => row.text.includes(w))
      && (!base || row.where === base)
      && (!scoped || row.marks.some(([p]) => inProject(p) && packageKeys[p].includes(pkg)))
      && [...signals].every((s) => row.signals.includes(s));
  }

  // ---------- Résultats ----------
  let layout = "flat";
  let sort = { key: "name", dir: 1 };
  let found = [];
  let current = { kind: null, id: null };

  function render() {
    const words = fold(filter.value).split(/\s+/).filter(Boolean);
    const pkg = fold(packageInput.value.trim());
    found = rows.filter((r) => test(r, words, baseSelect.value, pkg));
    const filtered = words.length || pkg || baseSelect.value || projectSelect.value || signals.size;
    reset.hidden = !filtered;
    counter.textContent = filtered ? `${plural(found.length, "colonne")} sur ${rows.length}`
      : plural(rows.length, "colonne suivie", "colonnes suivies");
    view.querySelectorAll("[data-signal]").forEach((b) => b.setAttribute("aria-pressed", signals.has(b.dataset.signal)));
    if (!found.length) results.innerHTML = nothing();
    else if (layout === "flat") renderFlat(words);
    else renderTree(filtered);
    markActive();
  }

  function sorted() {
    if (sort.key === "name" && sort.dir > 0) return found; // `rows` est déjà dans cet ordre
    const key = {
      name: (r) => name(r.c), table: (r) => r.d.title, base: (r) => r.where, writers: (r) => r.writers.length,
      readers: (r) => r.readers.length, sources: (r) => r.sources, targets: (r) => r.targets,
    }[sort.key];
    const compare = NUMERIC.includes(sort.key) ? (a, b) => key(a) - key(b) : (a, b) => collator.compare(key(a), key(b));
    // `sort` est stable : à égalité, l'ordre des noms reste.
    return found.slice().sort((a, b) => sort.dir * compare(a, b));
  }

  function renderFlat(words) {
    const head = HEADS.map(([key, label, title]) => {
      const on = sort.key === key;
      return `<th scope="col"${on ? ` aria-sort="${sort.dir > 0 ? "ascending" : "descending"}"` : ""}>`
        + `<button type="button" data-sort="${key}"${title ? ` title="${title}"` : ""}>${label}${on ? (sort.dir > 0 ? " ▲" : " ▼") : ""}</button></th>`;
    }).join("");
    const body = sorted().slice(0, ROW_LIMIT).map((r) => `<tr data-col="${r.c}">`
      + `<td><a href="#colonnes/c/${r.c}">${highlight(name(r.c), words)}</a></td>`
      + `<td><a href="#colonnes/t/${r.d.id}">${highlight(r.d.title, words)}</a></td><td>${esc(r.where)}</td>`
      + `<td>${packages(r.writers, "w")}</td><td>${packages(r.readers, "")}</td>`
      + `<td class="num">${r.sources}</td><td class="num">${r.targets}</td></tr>`).join("");
    const cut = found.length > ROW_LIMIT
      ? `<p class="col-note">${found.length} colonnes : les ${ROW_LIMIT} premières sont affichées. Préciser la recherche, la base ou le package pour affiner.</p>`
      : "";
    results.innerHTML = `${cut}<table class="col-table"><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table>`;
  }

  // Premier mot cherché trouvé dans le texte, surligné.
  function highlight(text, words) {
    const lower = text.toLowerCase();
    for (const word of words) {
      const at = lower.indexOf(word);
      if (at >= 0) return `${esc(text.slice(0, at))}<mark>${esc(text.slice(at, at + word.length))}</mark>${esc(text.slice(at + word.length))}`;
    }
    return esc(text);
  }

  function packages(list, cls) {
    if (!list.length) return '<span class="muted">—</span>';
    // Deux projets peuvent avoir un package du même nom : le projet au survol.
    const names = list.map((p) => [data.packages[p] ?? "?", project(p)]).sort(([a], [b]) => collator.compare(a, b));
    const more = names.length > 3 ? `<span class="pk" title="${esc(names.slice(3).map(([n]) => n).join(", "))}">+${names.length - 3}</span>` : "";
    return names.slice(0, 3).map(([n, where]) => `<span class="pk ${cls}" title="${esc(where)}">${esc(n)}</span>`).join("") + more;
  }

  function nothing() {
    if (!rows.length) return '<p class="empty">Aucune colonne suivie dans ces packages.</p>';
    return '<div class="col-none"><p><strong>Aucune colonne ne correspond.</strong></p>'
      + '<button type="button" data-reset>Effacer les filtres</button></div>';
  }

  // ---------- Par table ----------
  let listed = new Map();

  function renderTree(filtered) {
    const by = new Map();
    for (const r of found) (by.get(r.d) ?? by.set(r.d, []).get(r.d)).push(r.c);
    listed = new Map();
    let hidden = 0;
    const parts = [];
    for (const [kind, title] of GROUPS) {
      const items = [];
      for (const d of listedSets) {
        const cols = d.kind === kind && by.get(d);
        if (!cols) continue;
        if (listed.size >= TREE_LIMIT) {
          hidden++;
          continue;
        }
        listed.set(d.id, cols);
        items.push(datasetItem(d, cols, filtered && cols.length < d.columns.length));
      }
      if (items.length) parts.push(`<h3>${title}</h3>${items.join("")}`);
    }
    if (hidden) parts.push(`<p class="muted">${plural(hidden, "autre jeu", "autres jeux")} de données : affiner la recherche.</p>`);
    results.innerHTML = `<div class="col-tree">${parts.join("")}</div>`;
  }

  function datasetItem(d, cols, open) {
    return `<details data-block="${d.id}"${open ? " open" : ""}><summary><span class="name">${esc(d.title)}</span>`
      + `<span class="count">${cols.length}</span><span class="sub">${esc(d.sub)}</span></summary>`
      + `<div class="cols">${open ? columnLinks(d, cols) : ""}</div></details>`;
  }

  function columnLinks(d, cols) {
    const links = cols.map((c) => `<a href="#colonnes/c/${c}" data-col="${c}"><span>${esc(name(c))}</span>`
      + `<span class="flow" title="origines et usages">←${columnsIn.get(c)?.length ?? 0} →${columnsOut.get(c)?.length ?? 0}</span></a>`);
    return `<a href="#colonnes/t/${d.id}" class="matrix-link" data-table="${d.id}">Matrice de la table</a>${links.join("")}`;
  }

  // Liste d'une table remplie à l'ouverture seulement (`toggle` ne remonte pas : capture).
  results.addEventListener("toggle", (event) => {
    const details = event.target;
    const cols = details.open && listed.get(Number(details.dataset.block));
    const box = details.querySelector(".cols");
    if (cols && !box.childElementCount) {
      box.innerHTML = columnLinks(data.nodes[Number(details.dataset.block)], cols);
      markActive();
    }
  }, true);

  // Colonne ou table affichée : soulignée (et dépliée, par table) dans les résultats.
  function markActive() {
    results.querySelectorAll(".active").forEach((el) => el.classList.remove("active"));
    const row = current.kind === "c" && results.querySelector(`tr[data-col="${current.id}"]`);
    if (row) {
      row.classList.add("active");
      row.scrollIntoView({ block: "nearest" });
      return;
    }
    const table = current.kind === "t" ? current.id : current.kind === "c" && !isStep(current.id) ? owner.get(current.id)?.id : null;
    const details = table == null ? null : results.querySelector(`details[data-block="${table}"]`);
    if (details && !details.open) details.open = true;
    const link = current.kind === "c" ? results.querySelector(`a[data-col="${current.id}"]`)
      : current.kind === "t" ? results.querySelector(`a[data-table="${current.id}"]`) : null;
    link?.classList.add("active");
    (link ?? details)?.scrollIntoView({ block: "nearest" });
  }

  // ---------- Filtres dans l'adresse : lien partageable, Précédent les retrouve ----------
  function params() {
    const p = new URLSearchParams();
    const set = (key, value) => value && p.set(key, value);
    set("q", filter.value.trim());
    set("base", baseSelect.value);
    set("projet", projectSelect.value);
    set("pkg", packageInput.value.trim());
    set("signal", [...signals].join(","));
    return p;
  }

  function remember() {
    const path = current.kind ? `#colonnes/${current.kind}/${current.id}` : "#colonnes";
    const query = params().toString();
    history.replaceState(null, "", query ? `${path}?${query}` : path);
  }

  function apply(p) {
    filter.value = p.get("q") ?? "";
    // Base ou projet absent de ce rapport (ou de cette base) : tous.
    const choose = (select, key) => {
      select.value = p.get(key) ?? "";
      if (select.selectedIndex < 0) select.value = "";
    };
    choose(baseSelect, "base");
    fillProjects();
    choose(projectSelect, "projet");
    fillPackages();
    packageInput.value = p.get("pkg") ?? "";
    signals.clear();
    (p.get("signal") ?? "").split(",").filter((s) => view.querySelector(`[data-signal="${s}"]`)).forEach((s) => signals.add(s));
  }

  function update() {
    remember();
    render();
  }

  let typing;
  const later = () => {
    clearTimeout(typing);
    typing = setTimeout(update, 120);
  };
  filter.addEventListener("input", later);
  packageInput.addEventListener("input", later);
  for (const select of [baseSelect, projectSelect]) {
    select.addEventListener("change", () => {
      narrow();
      update();
    });
  }

  view.addEventListener("click", (event) => {
    const target = event.target.closest("[data-reset], [data-sort], [data-layout], [data-signal]");
    if (!target) return;
    const { sort: key, layout: next, signal } = target.dataset;
    if (target.hasAttribute("data-reset")) {
      apply(new URLSearchParams());
      update();
    } else if (key) {
      sort = { key, dir: sort.key === key ? -sort.dir : NUMERIC.includes(key) ? -1 : 1 };
      render();
    } else if (next) {
      layout = next;
      view.querySelectorAll("[data-layout]").forEach((b) => b.setAttribute("aria-pressed", b === target));
      render();
    } else {
      if (!signals.delete(signal)) signals.add(signal);
      update();
    }
  });

  // Fiche de mapping : le CSV de `dtsx-inspect mapping`, embarqué tel quel par le rapport
  // (boutons de la vue d'ensemble et de l'onglet Colonnes).
  document.querySelectorAll("[data-export-mapping]").forEach((button) => button.addEventListener("click", () => {
    const csv = document.getElementById("mapping-csv")?.content.textContent ?? "";
    const href = URL.createObjectURL(new Blob([csv], { type: "text/csv;charset=utf-8" }));
    Object.assign(document.createElement("a"), { href, download: "mapping-colonnes.csv" }).click();
    // Le téléchargement lit l'adresse après le clic : libérée un peu plus tard.
    setTimeout(() => URL.revokeObjectURL(href), 10000);
  }));

  // ---------- Page d'une colonne ----------
  // Étapes amont ou aval, en profondeur : une ligne par colonne liée.
  function walk(start, upstream) {
    const rows = [];
    const visit = (node, path) => {
      const next = [...((upstream ? columnsIn : columnsOut).get(node) ?? [])].sort(([a], [b]) => full(a).localeCompare(full(b)));
      for (const [col, how] of next) {
        if (rows.length >= STEP_LIMIT) return;
        const loop = path.includes(col);
        rows.push({ depth: path.length - 1, how, col, loop });
        if (!loop) visit(col, [...path, col]);
      }
    };
    visit(start, [start]);
    return rows;
  }

  // Libellé et contexte : `Composant.colonne` et `Package › Data Flow`, ou la colonne
  // de table et son jeu de données.
  function label(col) {
    const text = full(col);
    if (!isStep(col)) return [text, owner.get(col)?.sub ?? ""];
    const at = text.lastIndexOf(" (");
    return at < 0 ? [text, ""] : [text.slice(0, at), text.slice(at + 2, -1)];
  }

  function steps(start, upstream) {
    const rows = walk(start, upstream);
    if (!rows.length) return `<p class="empty">${upstream ? "Aucune origine connue." : "Aucun usage connu."}</p>`;
    const items = rows.map(({ depth, how, col, loop }) => {
      const [text, context] = label(col);
      const unknown = upstream && isStep(col) && !columnsIn.has(col);
      const flags = (unknown ? '<span class="flag-sample">origine inconnue</span>' : "")
        + (loop ? '<span class="flag-sample">boucle</span>' : "");
      return `<li style="--depth:${depth}"><span class="how">${esc(how)}</span><span class="step">`
        + `<a href="#colonnes/c/${col}">${esc(text)}</a><span class="ctx">${esc(context)}</span></span>${flags}</li>`;
    });
    const cut = rows.length >= STEP_LIMIT ? `<p class="muted">Liste tronquée à ${STEP_LIMIT} étapes : dtsx-inspect columns --of pour la suite.</p>` : "";
    return `<ol class="steps">${items.join("")}</ol>${cut}`;
  }

  function facts(items) {
    return `<ul class="facts">${items.filter(([, v]) => v).map(([k, v]) => `<li><span>${esc(k)}</span> <strong>${esc(v)}</strong></li>`).join("")}</ul>`;
  }

  const close = '<a class="action" href="#colonnes">Fermer</a>';

  function columnPage(c) {
    const own = owner.get(c);
    const roles = new Map();
    for (const [p, mark] of uses(c)) roles.set(p, [...(roles.get(p) ?? []), MARKS[mark][1]]);
    const writers = uses(c).filter(([, m]) => m === "W").length;
    const readers = new Set(uses(c).filter(([, m]) => m !== "W").map(([p]) => p)).size;
    const about = isStep(c)
      ? [["Package", data.packages[own?.package] ?? "?"], ["Data Flow", data.groups[own?.group]?.title], [own?.kind === "sql" ? "Tâche SQL" : "Composant", own?.title]]
      : (own?.props ?? []).concat([["Écrite par", plural(writers, "package")], ["Lue par", plural(readers, "package")]]);
    const packages = isStep(c)
      ? (own?.package != null ? `<li>${packageLink(own.package)} <span class="muted">produit la colonne · ${esc(project(own.package))}</span></li>` : "")
      : [...roles].map(([p, r]) => `<li>${packageLink(p)} <span class="muted">${esc(r.join(", "))} · ${esc(project(p))}</span></li>`).join("");
    const siblings = (own?.columns ?? []).filter((o) => o !== c).sort(byName)
      .map((o) => `<a href="#colonnes/c/${o}">${esc(name(o))}</a>`).join("");
    const table = isStep(c)
      ? (own?.package != null ? `<a class="action" href="#p/${own.package}/flux/${own.id}">Voir l'étape dans le flux du package</a>` : "")
      : `<a class="action" href="#colonnes/t/${own?.id}">Matrice de la table</a>`;
    main.innerHTML = `<header class="col-head">
        <p class="crumb">${esc(own ? `${own.sub} › ${own.title}` : "")}</p>
        <div class="col-title">${own ? tile(own.kind) : ""}<div><h2>${esc(name(c))}</h2><p class="muted">${esc(full(c))}</p></div>
          <div class="col-actions">${table}<button type="button" class="action" data-copy-trace>Copier la trace</button>${close}</div></div>
        ${facts(about)}</header>
      <div class="chains">
        <section><h3>D'où elle vient</h3>${steps(c, true)}</section>
        <section><h3>Où elle va</h3>${steps(c, false)}</section>
      </div>
      <section><h3>Packages</h3>${packages ? `<ul class="col-packages">${packages}</ul>` : '<p class="empty">Aucun package connu.</p>'}</section>
      ${siblings ? `<section><h3>Autres colonnes ${isStep(c) ? "de l'étape" : "de la table"}</h3><div class="chips">${siblings}</div></section>` : ""}`;
    main.querySelector("[data-copy-trace]").addEventListener("click", (event) => {
      const text = lineage.columnTrace(c).map(([depth, line]) => "  ".repeat(depth) + line).join("\n");
      navigator.clipboard?.writeText(text);
      event.target.textContent = "Copiée";
    });
  }

  // ---------- Matrice d'une table ----------
  function tablePage(id) {
    const d = data.nodes[id];
    if (!d || !DATASETS.includes(d.kind)) {
      main.hidden = true;
      return;
    }
    const cols = [...(d.columns ?? [])].sort(byName);
    const packages = [...new Set(cols.flatMap((c) => uses(c).map(([p]) => p)))]
      .sort((a, b) => (data.packages[a] ?? "").localeCompare(data.packages[b] ?? ""));
    const legend = Object.values(MARKS).map(([mark, text, cls]) => `<li><span class="mark ${cls}">${mark}</span>${text}</li>`).join("");
    const cell = (c, p) => {
      const marks = uses(c).filter(([q]) => q === p).map(([, m]) => MARKS[m]);
      return marks.length
        ? marks.map(([mark, text, cls]) => `<span class="mark ${cls}" title="${text}">${mark}</span>`).join("")
        : '<span class="mark none" title="ne la touche pas">·</span>';
    };
    const matrix = !cols.length
      ? '<p class="empty">Aucune colonne suivie pour ce jeu de données.</p>'
      : `<ul class="legend-marks" aria-label="Légende">${legend}</ul><div class="matrix-box"><table class="matrix">
          <thead><tr><th scope="col">Colonne</th>${packages.map((p) => `<th scope="col">${packageLink(p)}<span>${esc(project(p))}</span></th>`).join("")}</tr></thead>
          <tbody>${cols.map((c) => `<tr><th scope="row"><a href="#colonnes/c/${c}">${esc(name(c))}</a></th>${packages.map((p) => `<td>${cell(c, p)}</td>`).join("")}</tr>`).join("")}</tbody>
        </table></div>`;
    main.innerHTML = `<header class="col-head">
        <p class="crumb">${esc(d.sub)}</p>
        <div class="col-title">${tile(d.kind)}<div><h2>${esc(d.title)}</h2>
          <p class="muted">Qui écrit, lit ou calcule chaque colonne : ce qui casse si on la change.</p></div>
          <div class="col-actions">${close}</div></div>
        ${facts([...d.props, ["Colonnes suivies", String(cols.length)], ["Packages", String(packages.length)]])}</header>${matrix}`;
  }

  let rendered = false;
  window.columns = {
    // `kind` : `c` (colonne), `t` (table) ou rien (les résultats seuls) ; `filters` :
    // les filtres de l'adresse (URLSearchParams), vides pour garder ceux de l'écran.
    show(kind, id, filters) {
      const changed = filters?.toString() && filters.toString() !== params().toString();
      if (changed) apply(filters);
      const valid = Number.isInteger(id) && (kind === "c" ? id < data.columns.length : kind === "t" && id < data.nodes.length);
      current = valid ? { kind, id } : { kind: null, id: null };
      main.hidden = !current.kind;
      if (current.kind === "c") columnPage(id);
      else if (current.kind === "t") tablePage(id);
      main.scrollTop = 0;
      if (!rendered || changed) {
        render();
        rendered = true;
      } else {
        markActive();
      }
      remember();
    },
  };
})();
