// Navigation du rapport : arborescence projets → packages à gauche, vue à droite.
// L'adresse décrit la vue (#vue, #carte/PROJET, #p/INDICE/flux|inventaire|lint, #regles/RÈGLE) :
// liens partageables et bouton Précédent. Sans les scripts du CDN, tout marche
// sauf le graphe et la mise en forme de l'inventaire.
(() => {
  const views = [...document.querySelectorAll(".view")];
  const treeLinks = [...document.querySelectorAll(".tree a")];
  const treeFilter = document.querySelector(".tree-filter");
  const packageView = document.getElementById("package");
  const lineage = window.lineage;
  const SUBS = ["flux", "inventaire", "lint"];

  function route() {
    const [view, arg, sub] = location.hash.slice(1).split("/").map(decodeURIComponent);
    const target = { p: "package", carte: "carte", regles: "regles" }[view] ?? "vue";
    views.forEach((v) => (v.hidden = v.id !== target));
    for (const a of treeLinks) {
      const active = a.dataset.pkg !== undefined ? view === "p" && a.dataset.pkg === arg : a.hash === `#${target}`;
      a.classList.toggle("active", active);
      if (active) reveal(a);
    }
    if (target === "carte") {
      lineage?.mount(document.querySelector("#carte .stage-slot"));
      lineage?.map(arg);
    } else if (target === "package") {
      showPackage(Number(arg), SUBS.includes(sub) ? sub : "flux");
    } else if (target === "regles") {
      showRule(arg);
    }
  }

  // Fiche d'une règle (lien depuis un constat) : amenée à l'écran et soulignée.
  function showRule(id) {
    const rule = id && document.getElementById(`regle-${id}`);
    document.querySelectorAll(".rule.focus").forEach((r) => r.classList.remove("focus"));
    if (!rule) return;
    rule.classList.add("focus");
    rule.scrollIntoView({ block: "center" });
  }

  // « Voir N constats » : la vue d'ensemble filtrée sur la règle, gravité affichée.
  document.querySelectorAll("[data-show-rule]").forEach((button) =>
    button.addEventListener("click", () => {
      const box = document.querySelector(`[data-severity="${button.dataset.ruleSeverity}"]`);
      if (box && !box.checked) {
        box.checked = true;
        box.dispatchEvent(new Event("change"));
      }
      const filter = document.querySelector("[data-filter='lint-rows']");
      filter.value = button.dataset.showRule;
      filter.dispatchEvent(new Event("input"));
      location.hash = "#vue";
      requestAnimationFrame(() => document.getElementById("lint-rows").scrollIntoView({ block: "start" }));
    })
  );

  function showPackage(index, sub) {
    const template = document.getElementById(`pkg-${index}`);
    if (!template) return (location.hash = "#vue");
    const info = template.dataset;
    packageView.querySelector(".crumb").textContent = info.project ? `${info.project} › ${info.file}` : info.file;
    packageView.querySelector(".pkg-name").textContent = info.name;
    packageView.querySelector(".pkg-meta").textContent = info.meta;
    const rows = [...document.querySelectorAll(`#lint-rows tr[data-pkg="${index}"]`)];
    packageView.querySelector(".subtabs .count").textContent = rows.length;
    for (const a of packageView.querySelectorAll(".subtabs a")) {
      a.href = `#p/${index}/${a.dataset.sub}`;
      a.setAttribute("aria-selected", a.dataset.sub === sub);
    }
    for (const panel of packageView.querySelectorAll(".sub")) panel.hidden = panel.dataset.sub !== sub;
    if (sub === "flux") {
      lineage?.mount(packageView.querySelector(".sub[data-sub='flux']"));
      lineage?.flow(index);
    } else if (sub === "inventaire") {
      packageView.querySelector(".md-host").replaceChildren(rendered(template));
    } else {
      packageView.querySelector(".pkg-lint tbody").replaceChildren(...rows.map((r) => r.cloneNode(true)));
      packageView.querySelector(".sub[data-sub='lint'] .empty").hidden = rows.length > 0;
    }
  }

  // `<` neutralisé : un nom de package ne doit pas pouvoir injecter de HTML.
  function rendered(template) {
    const markdown = template.content.querySelector("pre").textContent;
    const div = document.createElement("div");
    div.className = "md";
    if (window.marked) div.innerHTML = marked.parse(markdown.replace(/</g, "&lt;"));
    else div.append(Object.assign(document.createElement("pre"), { textContent: markdown }));
    return div;
  }

  // Package courant visible dans l'arborescence, son projet déplié.
  function reveal(link) {
    const project = link.closest("details");
    if (project) project.open = true;
    link.scrollIntoView({ block: "nearest" });
  }

  addEventListener("hashchange", route);

  // Filtre de l'arborescence : nom ou chemin du package, nom du projet.
  treeFilter.addEventListener("input", () => {
    const query = treeFilter.value.trim().toLowerCase();
    for (const project of document.querySelectorAll(".tree details")) {
      const projectMatch = project.querySelector("summary").textContent.toLowerCase().includes(query);
      let shown = 0;
      for (const a of project.querySelectorAll("a")) {
        const match = projectMatch || `${a.textContent} ${a.title}`.toLowerCase().includes(query);
        a.hidden = !match;
        shown += match;
      }
      project.hidden = shown === 0;
      project.open = query !== "" && shown > 0;
    }
  });

  // Constats de lint de la vue d'ensemble : gravités et filtre texte.
  document.querySelectorAll("[data-severity]").forEach((box) =>
    box.addEventListener("change", () =>
      document.querySelector("#vue table.lint").classList.toggle("hide-" + box.dataset.severity, !box.checked)
    )
  );
  document.querySelectorAll("[data-filter]").forEach((input) =>
    input.addEventListener("input", () => {
      const query = input.value.trim().toLowerCase();
      for (const row of document.getElementById(input.dataset.filter).children) {
        row.hidden = !row.textContent.toLowerCase().includes(query);
      }
    })
  );

  // Raccourcis : / filtre les packages, Échap ferme le panneau, F ajuste le graphe.
  addEventListener("keydown", (event) => {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.target.matches?.("input, select, textarea")) {
      if (event.key === "Escape") event.target.blur();
      return;
    }
    if (event.key === "/") {
      event.preventDefault();
      treeFilter.focus();
    } else if (event.key === "Escape") {
      lineage?.close();
    } else if (event.key === "f" || event.key === "F") {
      lineage?.fit();
    }
  });

  route();
})();
