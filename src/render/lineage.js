// Lignage interactif : carte d'un projet (gauche → droite) et flux d'un package
// (haut → bas). Clic maintenu pour se déplacer, molette pour zoomer, clic sur un
// bloc pour sa chaîne amont / aval et ses détails. Placement : ELK (CDN).
// Piloté par report.js via `window.lineage` (le graphe est déplacé dans la vue affichée).
// Les textes viennent des packages : jamais insérés en HTML sans `esc`.
(() => {
  const source = document.getElementById("lineage-data");
  const stage = document.querySelector(".stage");
  if (!source || !stage) return;
  const data = JSON.parse(source.textContent);
  if (!data) return;
  data.nodes.forEach((n, i) => (n.id = i));

  const canvas = stage.querySelector(".canvas");
  const world = stage.querySelector(".world");
  const wires = stage.querySelector(".wires");
  const inspector = stage.querySelector(".inspector");
  const SIZE = { map: [220, 60], flow: [260, 60] };
  const KINDS = {
    table: "Table", file: "Fichier", entity: "Entité Dynamics", proc: "Procédure stockée", source: "Composant source",
    transform: "Transformation", dest: "Composant destination", sql: "Tâche SQL",
    untracked: "Étape non suivie",
  };
  const ICONS = {
    table: '<svg viewBox="0 0 16 16"><ellipse cx="8" cy="3.8" rx="5" ry="2"/><path d="M3 3.8v8.4c0 1.1 2.2 2 5 2s5-.9 5-2V3.8"/><path d="M3 8c0 1.1 2.2 2 5 2s5-.9 5-2"/></svg>',
    file: '<svg viewBox="0 0 16 16"><path d="M4 1.5h5l3 3v10H4z"/><path d="M9 1.5v3h3M6 8.5h4M6 11h4"/></svg>',
    entity: '<svg viewBox="0 0 16 16"><rect x="1.5" y="3" width="13" height="10" rx="1.5"/><circle cx="5.5" cy="7" r="1.5"/><path d="M3.5 11c.4-1 1.1-1.5 2-1.5s1.6.5 2 1.5M9.5 6.5h3M9.5 9.5h3"/></svg>',
    source: '<svg viewBox="0 0 16 16"><path d="M1.5 8h8M6.5 5l3 3-3 3M12.5 2.5v11"/></svg>',
    transform: '<svg viewBox="0 0 16 16"><path d="M1.5 4.5h3l7 7h3M1.5 11.5h3l7-7h3M12.5 2.5l2 2-2 2M12.5 9.5l2 2-2 2"/></svg>',
    dest: '<svg viewBox="0 0 16 16"><path d="M3.5 2.5v11M5.5 8h8M10.5 5l3 3-3 3"/></svg>',
    sql: "SQL",
    proc: "PROC",
    untracked: '<svg viewBox="0 0 16 16"><path d="M8 1.8 14.8 13.7H1.2z"/><path d="M8 6.2v3.6M8 11.6v.2"/></svg>',
    flow: '<svg viewBox="0 0 16 16"><rect x="1.5" y="4" width="4" height="8" rx="1"/><rect x="10.5" y="4" width="4" height="8" rx="1"/><path d="M5.5 8h5"/></svg>',
  };
  const esc = (text) =>
    String(text).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const tile = (kind) => `<span class="tile k-${kind}">${ICONS[kind]}</span>`;
  const packageName = (n) => (n.package == null ? "" : data.packages[n.package] ?? "?");

  let mode = "map";
  let flowPackage = null;
  // Bloc dont on n'affiche que la chaîne amont / aval (« Isoler la chaîne »).
  let isolated = null;
  let nodes = [];
  let edges = [];
  let byId = new Map();
  let pos = new Map();
  let routes = new Map();
  let groups = [];
  let cam = { x: 0, y: 0, k: 1 };
  const elk = window.ELK ? new window.ELK() : null;

  // Packages qui ont des blocs de lignage.
  const withBlocks = [...new Set(data.nodes.map((n) => n.package).filter((p) => p != null))];

  // Projet du package, calculé comme pour l'arborescence. Au-delà de MAP_LIMIT blocs, la
  // carte complète bloque le navigateur (22 s pour 5 000 blocs) : elle s'ouvre sur un projet.
  const MAP_LIMIT = 1500;
  const ALL = "*";
  const projectOf = (p) => data.projects[p] ?? "";
  const projects = [...new Set(withBlocks.map(projectOf))].sort((a, b) => a.localeCompare(b));
  const projectSelect = document.querySelector("#carte .project-select");
  const tooBig = data.nodes.length > MAP_LIMIT;
  projectSelect.add(new Option(`Tous les projets${tooBig ? " (lent)" : ""}`, ALL));
  for (const project of projects) projectSelect.add(new Option(project || "(racine)", project));
  // Ouverture sur le projet qui a le plus de packages.
  const largest = projects.reduce((best, project) => {
    const size = (q) => withBlocks.filter((p) => projectOf(p) === q).length;
    return size(project) > size(best) ? project : best;
  }, projects[0]);
  const defaultProject = tooBig && projects.length > 1 ? largest : ALL;
  projectSelect.addEventListener("change", () => (location.hash = `#carte/${encodeURIComponent(projectSelect.value)}`));

  // Jeu de données touché par au moins HUB_PACKAGES packages (journal d'exécution…) :
  // sur la carte, une copie par package, sinon tout converge vers lui en étoile.
  // ponytail: le seuil est fixe ; le rendre réglable si une vraie table de référence
  // partagée gagne à rester en un seul bloc.
  const HUB_PACKAGES = 5;
  function splitHubs(allNodes, allEdges) {
    const touching = new Map();
    for (const [a, b] of allEdges) {
      for (const [dataset, step] of [[a, b], [b, a]]) {
        const p = data.nodes[step].package;
        if (data.nodes[dataset].package == null && p != null) {
          touching.set(dataset, (touching.get(dataset) ?? new Set()).add(p));
        }
      }
    }
    const hubs = new Map([...touching].filter(([, pkgs]) => pkgs.size >= HUB_PACKAGES));
    if (!hubs.size) return [allNodes, allEdges];
    const copies = new Map();
    const out = allNodes.filter((n) => !hubs.has(n.id));
    const copyOf = (hub, pkg) => {
      const key = `${hub}:${pkg}`;
      if (!copies.has(key)) {
        const copy = { ...data.nodes[hub], id: data.nodes.length + copies.size, shared: hubs.get(hub).size, origin: hub };
        copies.set(key, copy);
        out.push(copy);
      }
      return copies.get(key).id;
    };
    const edgesOut = allEdges.map(([a, b, ...style]) => [
      hubs.has(a) ? copyOf(a, data.nodes[b].package) : a,
      hubs.has(b) ? copyOf(b, data.nodes[a].package) : b,
      ...style,
    ]);
    return [out, edgesOut];
  }

  // Les blocs des packages retenus et les jeux de données qu'ils touchent.
  function scoped(keepPackage) {
    const own = new Set(data.nodes.filter((n) => n.package != null && keepPackage(n.package)).map((n) => n.id));
    const keep = new Set(own);
    for (const [a, b] of data.edges) {
      if (own.has(a)) keep.add(b);
      if (own.has(b)) keep.add(a);
    }
    return [data.nodes.filter((n) => keep.has(n.id)), data.edges.filter(([a, b]) => keep.has(a) && keep.has(b))];
  }

  function visible() {
    let [n, e] = mode === "flow"
      ? scoped((p) => p === flowPackage)
      : projectSelect.value === ALL
        ? splitHubs(data.nodes, data.edges)
        : splitHubs(...scoped((p) => projectOf(p) === projectSelect.value));
    if (isolated != null && n.some((x) => x.id === isolated)) {
      const keep = chain(isolated, e);
      [n, e] = [n.filter((x) => keep.has(x.id)), e.filter(([a, b]) => keep.has(a) && keep.has(b))];
    }
    return [n, e];
  }

  // Projets dont des packages touchent un jeu de données, d'après le graphe complet.
  function projectsUsing(datasetId) {
    const count = new Map();
    for (const [a, b] of data.edges) {
      const other = a === datasetId ? b : b === datasetId ? a : null;
      const p = other == null ? null : data.nodes[other].package;
      if (p != null) count.set(projectOf(p), (count.get(projectOf(p)) ?? new Set()).add(p));
    }
    return [...count].sort(([a], [b]) => a.localeCompare(b)).map(([project, pkgs]) => `${project || "(racine)"} (${pkgs.size})`);
  }

  // Mise en page ELK (algorithme « layered ») : les Data Flows sont de vrais
  // conteneurs, les liens sont orthogonaux et ceux qui convergent fusionnent.
  async function layout(nodes, edges) {
    const [w, h] = SIZE[mode];
    // Une entrée et une sortie par bloc : les liens qui y arrivent fusionnent en un tronc.
    const [inSide, outSide] = mode === "map" ? ["WEST", "EAST"] : ["NORTH", "SOUTH"];
    const block = (n) => ({
      id: String(n.id),
      width: w,
      height: h,
      layoutOptions: { "elk.portConstraints": "FIXED_SIDE" },
      ports: [
        { id: `${n.id}:in`, layoutOptions: { "elk.port.side": inSide } },
        { id: `${n.id}:out`, layoutOptions: { "elk.port.side": outSide } },
      ],
    });
    const members = new Map();
    const top = [];
    for (const n of nodes) {
      if (n.group == null) top.push(block(n));
      else members.set(n.group, [...(members.get(n.group) ?? []), block(n)]);
    }
    // Espacements répétés sur les cadres : ELK ne les hérite pas du graphe.
    const spacing = {
      "elk.spacing.nodeNode": "24",
      "elk.layered.spacing.nodeNodeBetweenLayers": "48",
      "elk.layered.spacing.edgeNodeBetweenLayers": "16",
    };
    const out = await elk.layout({
      id: "root",
      layoutOptions: {
        "elk.algorithm": "layered",
        "elk.direction": mode === "map" ? "RIGHT" : "DOWN",
        "elk.hierarchyHandling": "INCLUDE_CHILDREN",
        "elk.edgeRouting": "ORTHOGONAL",
        "elk.layered.mergeEdges": "true",
        ...spacing,
      },
      children: top.concat([...members].map(([id, children]) => ({
        id: "g" + id,
        children,
        // Haut : place du bandeau de titre.
        layoutOptions: { "elk.padding": "[top=50,left=16,bottom=16,right=16]", ...spacing },
      }))),
      edges: edges.map(([a, b], i) => ({ id: "e" + i, sources: [`${a}:out`], targets: [`${b}:in`] })),
    });
    // Coordonnées ELK relatives au conteneur : on les rend absolues.
    const origin = new Map([["root", { x: 0, y: 0 }]]);
    const pos = new Map();
    const groups = [];
    for (const c of out.children) {
      if (c.id.startsWith("g")) {
        origin.set(c.id, { x: c.x, y: c.y });
        groups.push({ id: Number(c.id.slice(1)), x: c.x, y: c.y, w: c.width, h: c.height });
        for (const m of c.children) pos.set(Number(m.id), { x: c.x + m.x, y: c.y + m.y, w, h });
      } else {
        pos.set(Number(c.id), { x: c.x, y: c.y, w, h });
      }
    }
    const routes = new Map(out.edges.map((e, i) => {
      const o = origin.get(e.container ?? "root") ?? { x: 0, y: 0 };
      const s = e.sections?.[0];
      const points = s ? [s.startPoint, ...(s.bendPoints ?? []), s.endPoint] : [];
      return [`${edges[i][0]}>${edges[i][1]}`, points.map((p) => ({ x: p.x + o.x, y: p.y + o.y }))];
    }));
    return { pos, groups, routes };
  }

  const place = (el, b) =>
    Object.assign(el.style, { left: b.x + "px", top: b.y + "px", width: b.w + "px", height: b.h + "px" });

  // La mise en page est asynchrone : seul le dernier dessin demandé s'affiche.
  let drawing = 0;
  async function draw(select = null) {
    const ticket = ++drawing;
    // Pendant le calcul, l'ancien graphe est estompé : il ne doit pas passer pour le nouveau.
    canvas.classList.add("busy");
    const [nextNodes, nextEdges] = visible();
    const placed = await layout(nextNodes, nextEdges);
    if (ticket !== drawing) return;
    canvas.classList.remove("busy");
    [nodes, edges] = [nextNodes, nextEdges];
    ({ pos, groups, routes } = placed);
    byId = new Map(nodes.map((n) => [n.id, n]));
    world.querySelectorAll(".node, .group").forEach((el) => el.remove());
    for (const b of groups) {
      const group = data.groups[b.id];
      const el = document.createElement("div");
      el.className = "group";
      place(el, b);
      const pkg = mode === "map" ? ` <span>· ${esc(data.packages[group.package] ?? "?")}</span>` : "";
      el.innerHTML = `<header>${ICONS.flow}<b>Data Flow · ${esc(group.title)}</b>${pkg}</header>`;
      world.append(el);
    }
    for (const n of nodes) {
      const el = document.createElement("div");
      el.className = "node" + (n.note ? " untracked" : "");
      el.dataset.id = n.id;
      el.title = n.title;
      place(el, pos.get(n.id));
      const sub = mode === "map" && n.package != null ? `${n.sub} · ${packageName(n)}` : n.sub;
      const flag = n.note ? "non suivi" : n.expression ? "expression" : n.shared ? `${n.shared} packages` : "";
      el.innerHTML = `${tile(n.kind)}<span class="text"><span class="title">${esc(n.title)}</span>`
        + `<span class="sub">${esc(sub)}</span></span>`
        + (flag ? `<span class="flag">${flag}</span>` : "");
      world.append(el);
    }
    drawWires();
    drawLegend();
    canvas.querySelector(".no-block").hidden = nodes.length > 0;
    choose(select);
    fit(true);
    if (select != null) center(select);
  }

  function drawWires() {
    const arrow = (id, color) =>
      `<marker id="${id}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 1 9 5 0 9z" fill="${color}"/></marker>`;
    wires.innerHTML = `<defs>${arrow("lineage-arrow", "var(--wire)")}${arrow("lineage-arrow-hot", "var(--accent)")}</defs>`;
    for (const [from, to, dashed, creates] of edges) {
      const points = routes.get(`${from}>${to}`) ?? [];
      if (points.length < 2) continue;
      // Tracé orthogonal, coins arrondis.
      let d = `M${points[0].x} ${points[0].y}`;
      for (let i = 1; i < points.length - 1; i++) {
        const [a, p, b] = [points[i - 1], points[i], points[i + 1]];
        const r = Math.min(8, Math.hypot(p.x - a.x, p.y - a.y) / 2, Math.hypot(b.x - p.x, b.y - p.y) / 2);
        const towards = (q, k) => ({ x: p.x + Math.sign(q.x - p.x) * k, y: p.y + Math.sign(q.y - p.y) * k });
        const [in_, out] = [towards(a, r), towards(b, r)];
        d += ` L${in_.x} ${in_.y} Q${p.x} ${p.y} ${out.x} ${out.y}`;
      }
      const last = points[points.length - 1];
      d += ` L${last.x} ${last.y}`;
      const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
      path.setAttribute("d", d);
      path.dataset.from = from;
      path.dataset.to = to;
      path.classList.toggle("dashed", dashed);
      path.classList.toggle("creates", creates);
      path.setAttribute("marker-end", "url(#lineage-arrow)");
      wires.append(path);
    }
  }

  // Amont et aval complets d'un bloc.
  function chain(id, among = edges) {
    const keep = new Set([id]);
    for (const forward of [true, false]) {
      const todo = [id];
      while (todo.length) {
        const current = todo.pop();
        for (const [a, b] of among) {
          const [here, next] = forward ? [a, b] : [b, a];
          if (here === current && !keep.has(next)) {
            keep.add(next);
            todo.push(next);
          }
        }
      }
    }
    return keep;
  }

  let selected = null;
  // Met en avant les blocs de `keep` et estompe les autres (`null` : rien d'estompé).
  // `hot` : liens internes en couleur, pour la chaîne du bloc sélectionné.
  function emphasize(keep, hot = true) {
    world.querySelectorAll(".node").forEach((el) => {
      el.classList.toggle("selected", Number(el.dataset.id) === selected);
      el.classList.toggle("dim", !!keep && !keep.has(Number(el.dataset.id)));
    });
    world.querySelectorAll(".group").forEach((el) => el.classList.toggle("dim", !!keep));
    wires.querySelectorAll("path").forEach((p) => {
      const inside = !!keep && keep.has(Number(p.dataset.from)) && keep.has(Number(p.dataset.to));
      p.classList.toggle("hot", hot && inside);
      p.classList.toggle("dim", !!keep && !inside);
      p.setAttribute("marker-end", hot && inside ? "url(#lineage-arrow-hot)" : "url(#lineage-arrow)");
    });
  }
  const emphasizeSelection = () => emphasize(selected == null ? null : chain(selected));

  function choose(id) {
    // Bloc absent de la vue (lien vers un autre package, bloc filtré) : rien de choisi.
    selected = byId.has(id) ? id : null;
    emphasizeSelection();
    if (selected == null) {
      inspector.classList.remove("open");
      return;
    }
    const n = byId.get(id);
    const links = (ids) => ids.length
      ? `<div class="links">${ids.map((i) => {
          const other = byId.get(i);
          return `<button data-goto="${i}">${tile(other.kind)}<span>${esc(other.title)}</span><span class="go">›</span></button>`;
        }).join("")}</div>`
      : '<p class="empty">Aucun</p>';
    const up = edges.filter(([, b]) => b === id).map(([a]) => a);
    const down = edges.filter(([a]) => a === id).map(([, b]) => b);
    const isData = ["table", "file", "entity", "proc"].includes(n.kind);
    const usedBy = isData && projects.length > 1 ? [["Projets", projectsUsing(n.origin ?? n.id).join(", ")]] : [];
    const props = (n.package != null ? [["Package", packageName(n)], ["Fichier", data.files[n.package]]] : [])
      .concat(n.props, usedBy);
    const isolate = `<button class="action" data-isolate>${isolated === id ? "Afficher tout le graphe" : "Isoler la chaîne"}</button>`;
    const pages = n.package == null ? "" : (mode === "map"
      ? `<a class="action" href="#p/${n.package}/flux">Voir le flux du package ${esc(packageName(n))}</a>`
      : "") + `<a class="action" href="#p/${n.package}/inventaire">Voir dans l'inventaire</a>`;
    const actions = `<section class="actions">${isolate}${pages}</section>`;
    inspector.innerHTML = `
      <div class="inspector-head">${tile(n.kind)}<div><h3>${esc(n.title)}</h3><p>${esc(KINDS[n.kind] ?? "")}</p></div>
        <button class="close" title="Fermer (Échap)" aria-label="Fermer">×</button></div>
      ${actions}
      ${n.note ? `<section><p class="note">${esc(n.note)}</p></section>` : ""}
      ${n.shared ? `<section><p class="note">Touché par ${n.shared} packages : la carte en montre une copie par package, avec ses seuls liens. Le flux de chaque package les détaille.</p></section>` : ""}
      ${n.expression ? '<section><p class="note">Construit par expression : la valeur affichée est celle de la conception, elle peut différer à l\'exécution.</p></section>' : ""}
      <section><h4>Propriétés</h4><dl class="props">${props.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join("")}</dl></section>
      ${n.query ? `<section><h4>${esc(n.query.title)}</h4><pre class="query">${esc(n.query.text)}</pre><button class="action" data-copy>Copier</button></section>` : ""}
      ${columnsSection(n)}
      <section><h4>${isData ? "Alimenté par" : "En amont"}</h4>${links(up)}</section>
      <section><h4>${isData ? "Consommé par" : "En aval"}</h4>${links(down)}</section>`;
    inspector.classList.add("open");
  }

  inspector.addEventListener("click", (event) => {
    const target = event.target.closest("button");
    if (!target) return;
    if (target.classList.contains("close")) return choose(null);
    if (target.dataset.goto !== undefined) {
      const id = Number(target.dataset.goto);
      choose(id);
      center(id);
    } else if (target.dataset.isolate !== undefined) {
      isolated = isolated === selected ? null : selected;
      draw(selected);
    } else if (target.dataset.copy !== undefined) {
      navigator.clipboard?.writeText(inspector.querySelector(".query").textContent);
      target.textContent = "Copié";
    } else if (target.dataset.column !== undefined) {
      inspector.querySelectorAll(".column-list button").forEach((b) => b.classList.toggle("active", b === target));
      const trace = inspector.querySelector(".trace");
      // Une étape par ligne, en retrait suspendu : une ligne longue revient à la ligne
      // sous son propre texte. `textContent` : les noms viennent des packages.
      trace.replaceChildren(...columnTrace(Number(target.dataset.column)).map(([depth, text]) => {
        const line = document.createElement("div");
        line.style.setProperty("--depth", depth);
        line.textContent = text;
        return line;
      }));
      trace.hidden = false;
      inspector.querySelector(".column-page")?.remove();
      trace.after(Object.assign(document.createElement("a"), {
        className: "action column-page",
        href: `#colonnes/c/${target.dataset.column}`,
        textContent: "Ouvrir la page de la colonne",
      }));
    }
  });

  // ---------- Lignage par colonne : liste d'un bloc et chaîne d'une colonne ----------
  // `data.columns[i]` = [colonne, description complète, produite par un composant].
  const columnsIn = new Map();
  const columnsOut = new Map();
  for (const [from, to, how] of data.column_edges ?? []) {
    columnsIn.set(to, [...(columnsIn.get(to) ?? []), [from, how]]);
    columnsOut.set(from, [...(columnsOut.get(from) ?? []), [to, how]]);
  }
  // Une colonne très partagée peut avoir des milliers de descendants : la chaîne
  // affichée s'arrête là, le reste reste consultable en ligne de commande.
  const TRACE_LINES = 300;

  function columnsSection(n) {
    const ids = [...(n.columns ?? [])].sort((a, b) => data.columns[a][0].localeCompare(data.columns[b][0]));
    if (!ids.length) return "";
    const buttons = ids.map((c) => `<button data-column="${c}">${esc(data.columns[c][0])}</button>`).join("");
    return `<section><h4>Colonnes (${ids.length})</h4><p class="muted">Cliquer une colonne : d'où elle vient (←), où elle va (→).</p>
      <div class="column-list">${buttons}</div><div class="trace" hidden></div></section>`;
  }

  // Même contenu que `dtsx-inspect columns --of` : [profondeur, texte] par étape.
  function columnTrace(start) {
    const lines = [[0, data.columns[start][1]]];
    const walk = (node, upstream, path) => {
      const next = [...((upstream ? columnsIn : columnsOut).get(node) ?? [])]
        .sort(([a], [b]) => data.columns[a][1].localeCompare(data.columns[b][1]));
      for (const [other, how] of next) {
        if (lines.length > TRACE_LINES) return;
        const unknown = upstream && data.columns[other][2] && !columnsIn.has(other);
        const loop = path.includes(other);
        lines.push([path.length, `${upstream ? "←" : "→"} ${how} : ${data.columns[other][1]}`
          + `${unknown ? ", origine inconnue" : ""}${loop ? " (boucle)" : ""}`]);
        if (!loop) walk(other, upstream, [...path, other]);
      }
    };
    walk(start, true, [start]);
    walk(start, false, [start]);
    if (lines.length > TRACE_LINES) lines.push([0, `… chaîne tronquée à ${TRACE_LINES} lignes (dtsx-inspect columns --of pour la suite)`]);
    else if (lines.length === 1) lines.push([1, "Aucune colonne liée : ni origine ni usage connus."]);
    return lines;
  }

  // ---------- Légende : seulement ce que la vue affichée contient ----------
  const legend = stage.querySelector(".legend");
  const legendBody = legend.querySelector(".legend-body");
  // Repliée ou dépliée, d'un rapport à l'autre (stockage local indisponible : ouverte).
  const LEGEND_KEY = "dtsx-inspect.legende";
  try {
    if (localStorage.getItem(LEGEND_KEY) === "repliee") legend.open = false;
  } catch {}
  legend.addEventListener("toggle", () => {
    try {
      localStorage.setItem(LEGEND_KEY, legend.open ? "depliee" : "repliee");
    } catch {}
  });
  // Libellés courts : la légende tient sur deux colonnes à côté du graphe.
  const SHORT = {
    table: "Table", file: "Fichier", entity: "Entité", proc: "Procédure", source: "Source",
    transform: "Transformation", dest: "Destination", sql: "Tâche SQL", untracked: "Non suivie",
  };
  const KEYS = [
    ["Glisser", "déplacer"],
    ["Molette", "zoomer"],
    ["Clic", "détails"],
    ["F", "tout voir"],
    ["Échap", "fermer"],
    ["Entrée", "chercher"],
  ];
  const wire = (cls, marker) =>
    `<svg class="wire-sample" viewBox="0 0 40 10" aria-hidden="true"><path class="${cls}" d="M2 5H32" marker-end="url(#${marker})"/></svg>`;

  function drawLegend() {
    const kinds = Object.keys(KINDS).filter((k) => nodes.some((n) => n.kind === k));
    const blocks = kinds.map((k) =>
      `<li data-kind="${k}" title="${esc(KINDS[k])} : survoler pour les repérer">${tile(k)}<span>${SHORT[k]}</span></li>`);
    if (groups.length) blocks.push('<li title="Cadre qui regroupe les composants d\'un Data Flow"><span class="frame-sample"></span><span>Data Flow</span></li>');
    const marks = [
      [nodes.some((n) => n.note), "non suivi", "lignage interrompu (raison au clic)"],
      [nodes.some((n) => n.expression), "expression", "requête calculée par expression"],
      [nodes.some((n) => n.shared), "N packages", "table partagée, une copie par package"],
    ].filter(([shown]) => shown)
      .map(([, flag, text]) => `<li><span class="flag-sample">${flag}</span><span>${text}</span></li>`);
    const links = [
      [true, wire("", "lineage-arrow"), "données"],
      [edges.some((e) => e[2]), wire("dashed", "lineage-arrow"), "par expression"],
      [edges.some((e) => e[3]), wire("creates", "lineage-arrow"), "création de table"],
      [edges.length > 0, wire("hot", "lineage-arrow-hot"), "chaîne sélectionnée"],
    ].filter(([shown]) => shown)
      .map(([, sample, text]) => `<li>${sample}<span>${text}</span></li>`);
    const section = (title, items, cls = "") =>
      (items.length ? `<h4>${title}</h4><ul class="${cls}">${items.join("")}</ul>` : "");
    legendBody.innerHTML = section("Blocs", blocks, "two") + section("Repères", marks)
      + section("Liens", links)
      + `<h4>Commandes</h4><p class="keys">${KEYS.map(([k, v]) => `<span><kbd>${k}</kbd> ${v}</span>`).join("")}</p>`;
    legend.hidden = nodes.length === 0;
  }

  // Survol d'un genre de bloc : ses blocs restent nets, le reste s'estompe.
  legendBody.addEventListener("pointerover", (event) => {
    const kind = event.target.closest("[data-kind]")?.dataset.kind;
    if (!kind) return;
    emphasize(new Set(nodes.filter((n) => n.kind === kind).map((n) => n.id)), false);
  });
  legendBody.addEventListener("pointerout", (event) => {
    const from = event.target.closest("[data-kind]");
    if (from && !from.contains(event.relatedTarget)) emphasizeSelection();
  });

  // ---------- Déplacement (clic maintenu) et zoom ----------
  function apply() {
    world.style.transform = `translate(${cam.x}px, ${cam.y}px) scale(${cam.k})`;
    canvas.style.backgroundSize = `${20 * cam.k}px ${20 * cam.k}px`;
    canvas.style.backgroundPosition = `${cam.x}px ${cam.y}px`;
  }
  // `readable` : si tout faire tenir rendrait les blocs illisibles, on part du
  // coin haut gauche à un zoom lisible ; le bouton ⤢ montre toujours l'ensemble.
  const READABLE_ZOOM = 0.4;
  function fit(readable = false) {
    const items = [...pos.values(), ...groups];
    if (!items.length) return;
    const x0 = Math.min(...items.map((i) => i.x)), y0 = Math.min(...items.map((i) => i.y));
    const x1 = Math.max(...items.map((i) => i.x + i.w)), y1 = Math.max(...items.map((i) => i.y + i.h));
    // Légende dépliée : le graphe se cadre à sa droite pour ne rien cacher dessous.
    const left = legend.open && !legend.hidden ? legend.offsetLeft + legend.offsetWidth : 0;
    const W = canvas.clientWidth - left, H = canvas.clientHeight;
    const k = Math.max(0.1, Math.min(1, (W - 80) / (x1 - x0), (H - 80) / (y1 - y0)));
    cam = readable && k < READABLE_ZOOM
      ? { k: 0.7, x: left + 20 - x0 * 0.7, y: 20 - y0 * 0.7 }
      : { k, x: left + (W - (x1 - x0) * k) / 2 - x0 * k, y: Math.max(20, (H - (y1 - y0) * k) / 2) - y0 * k };
    apply();
  }
  function zoomAt(factor, px, py) {
    const k = Math.min(2.5, Math.max(0.1, cam.k * factor));
    cam.x = px - (px - cam.x) * (k / cam.k);
    cam.y = py - (py - cam.y) * (k / cam.k);
    cam.k = k;
    apply();
  }
  function center(id) {
    const p = pos.get(id);
    if (!p) return;
    const W = canvas.clientWidth - (inspector.classList.contains("open") ? inspector.offsetWidth : 0);
    cam.k = Math.max(cam.k, 0.8);
    cam.x = W / 2 - (p.x + p.w / 2) * cam.k;
    cam.y = canvas.clientHeight / 2 - (p.y + p.h / 2) * cam.k;
    world.style.transition = "transform .3s ease";
    apply();
    setTimeout(() => (world.style.transition = ""), 320);
  }

  let drag = null;
  let moved = false;
  canvas.addEventListener("pointerdown", (event) => {
    drag = { x: event.clientX, y: event.clientY, cx: cam.x, cy: cam.y };
    moved = false;
  });
  addEventListener("pointermove", (event) => {
    if (!drag) return;
    const dx = event.clientX - drag.x, dy = event.clientY - drag.y;
    if (!moved && Math.hypot(dx, dy) < 4) return;
    moved = true;
    canvas.classList.add("dragging");
    cam.x = drag.cx + dx;
    cam.y = drag.cy + dy;
    apply();
  });
  addEventListener("pointerup", () => {
    drag = null;
    canvas.classList.remove("dragging");
    setTimeout(() => (moved = false));
  });
  canvas.addEventListener("click", (event) => {
    if (moved) return;
    const el = event.target.closest(".node");
    choose(el ? Number(el.dataset.id) : null);
  });
  canvas.addEventListener("wheel", (event) => {
    event.preventDefault();
    const r = canvas.getBoundingClientRect();
    zoomAt(event.deltaY < 0 ? 1.12 : 1 / 1.12, event.clientX - r.left, event.clientY - r.top);
  }, { passive: false });
  stage.querySelectorAll("[data-zoom]").forEach((button) => button.addEventListener("click", () => {
    const how = button.dataset.zoom;
    if (how === "fit") return fit(false);
    zoomAt(how === "in" ? 1.2 : 1 / 1.2, canvas.clientWidth / 2, canvas.clientHeight / 2);
  }));

  // ---------- Recherche et interface pour report.js ----------
  let lastHit = -1;
  stage.querySelector(".stage-search").addEventListener("keydown", (event) => {
    if (event.key !== "Enter") return;
    const query = event.target.value.trim().toLowerCase();
    const hits = nodes.filter((n) => `${n.title} ${n.sub}`.toLowerCase().includes(query));
    if (!hits.length) return;
    // Entrée de nouveau : résultat suivant.
    const hit = hits[(hits.findIndex((n) => n.id === lastHit) + 1) % hits.length];
    lastHit = hit.id;
    choose(hit.id);
    center(hit.id);
  });

  // Le graphe n'est redessiné que si la vue demandée change : revenir sur le flux
  // d'un package garde le cadrage et la sélection. `select` : bloc à ouvrir (recherche).
  let shown = null;
  function show(next, pkg = null, select = null) {
    const key = next === "flow" ? `flow:${pkg}` : `map:${projectSelect.value}`;
    if (key === shown) {
      if (select != null && byId.has(select)) {
        choose(select);
        center(select);
      }
      return;
    }
    [shown, mode, flowPackage, isolated] = [key, next, pkg, null];
    draw(select);
  }

  if (elk) canvas.querySelector(".offline").hidden = true;
  // Appelé par report.js une fois la vue affichée (le placement se mesure).
  window.lineage = {
    mount: (slot) => slot.append(stage),
    map(project) {
      const known = project === ALL || projects.includes(project);
      projectSelect.value = known ? project : defaultProject;
      if (elk) show("map");
    },
    flow(pkg, select = null) {
      if (elk) show("flow", pkg, select);
    },
    close: () => choose(null),
    fit: () => fit(false),
    // Pour columns.js et search.js : mêmes données, mêmes échappement et icônes.
    data,
    esc,
    tile,
    columnsIn,
    columnsOut,
    columnTrace,
  };
})();
