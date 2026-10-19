// Navigation du rapport : un onglet par espace, la recherche globale en haut (search.js).
// L'adresse décrit la vue (#vue, #carte/PROJET, #colonnes/c|t/INDICE[?filtres], #packages,
// #p/INDICE/flux|inventaire|lint[/BLOC], #qualite, #regles/RÈGLE) : liens
// partageables et bouton Précédent. Sans les scripts du CDN, tout marche sauf le graphe
// et la mise en forme de l'inventaire.
(() => {
  const views = [...document.querySelectorAll(".view")];
  const tabs = [...document.querySelectorAll(".tabs a")];
  const packageView = document.getElementById("package");
  const search = document.getElementById("global-search");
  const lineage = window.lineage;
  const SUBS = ["flux", "inventaire", "lint"];
  // Espace de chaque début d'adresse ; une règle s'affiche dans l'onglet Qualité.
  const TARGETS = {
    carte: "carte", colonnes: "colonnes", packages: "packages", p: "package",
    qualite: "qualite", regles: "qualite",
  };

  function route() {
    // `?q=…&base=…` : les filtres de l'onglet Colonnes, gardés dans l'adresse.
    const [path, query] = location.hash.slice(1).split("?");
    const [view, arg, sub, block] = path.split("/").map(decodeURIComponent);
    const target = TARGETS[view] ?? "vue";
    views.forEach((v) => (v.hidden = v.id !== target));
    // La page d'un package se trouve sous l'onglet Packages.
    const tab = target === "package" ? "packages" : target;
    for (const a of tabs) {
      if (a.dataset.tab === tab) a.setAttribute("aria-current", "page");
      else a.removeAttribute("aria-current");
    }
    if (target === "carte") {
      lineage?.mount(document.querySelector("#carte .stage-slot"));
      lineage?.map(arg);
    } else if (target === "colonnes") {
      window.columns?.show(arg, Number(sub), new URLSearchParams(query));
    } else if (target === "package") {
      showPackage(Number(arg), SUBS.includes(sub) ? sub : "flux", block == null ? null : Number(block));
    } else if (view === "regles") {
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

  // « Voir N constats » : le tableau de l'onglet Qualité filtré sur la règle, gravité affichée.
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
      location.hash = "#qualite";
      requestAnimationFrame(() => document.getElementById("lint-rows").scrollIntoView({ block: "start" }));
    })
  );

  // `block` : bloc du flux à ouvrir (résultat de la recherche globale).
  function showPackage(index, sub, block) {
    const template = document.getElementById(`pkg-${index}`);
    if (!template) return (location.hash = "#packages");
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
      lineage?.flow(index, Number.isInteger(block) ? block : null);
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

  addEventListener("hashchange", route);

  // Filtre de l'onglet Packages : nom ou chemin du package, nom du projet. Les projets
  // trouvés se déplient ; filtre vidé, tout se replie.
  const packageFilter = document.getElementById("package-filter");
  packageFilter?.addEventListener("input", () => {
    const query = packageFilter.value.trim().toLowerCase();
    for (const project of document.querySelectorAll("#packages .project")) {
      const projectMatch = project.querySelector("summary").textContent.toLowerCase().includes(query);
      let shown = 0;
      for (const item of project.querySelectorAll("li")) {
        const match = projectMatch || item.textContent.toLowerCase().includes(query);
        item.hidden = !match;
        shown += match;
      }
      project.hidden = shown === 0;
      project.open = query !== "" && shown > 0;
    }
  });

  // Constats de lint de l'onglet Qualité : gravités et filtre texte.
  document.querySelectorAll("[data-severity]").forEach((box) =>
    box.addEventListener("change", () =>
      document.querySelector("#qualite table.lint").classList.toggle("hide-" + box.dataset.severity, !box.checked)
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

  // Raccourcis : / cherche partout (comme Ctrl K), Échap ferme le panneau, F ajuste le graphe.
  addEventListener("keydown", (event) => {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.target.matches?.("input, select, textarea")) {
      if (event.key === "Escape") event.target.blur();
      return;
    }
    if (event.key === "/") {
      event.preventDefault();
      search?.focus();
    } else if (event.key === "Escape") {
      lineage?.close();
    } else if (event.key === "f" || event.key === "F") {
      lineage?.fit();
    }
  });

  route();
})();
