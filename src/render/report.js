// Report navigation: one tab per space, the global search at the top (search.js).
// The address describes the view (#overview, #map/PROJECT, #columns/c|t/INDEX[?filters],
// #packages, #p/INDEX/flow|inventory|lint[/BLOCK], #quality, #rules/RULE): shareable
// links and Back button. Without the CDN scripts, everything works except the graph and
// the inventory formatting.
(() => {
  const views = [...document.querySelectorAll(".view")];
  const tabs = [...document.querySelectorAll(".tabs a")];
  const packageView = document.getElementById("package");
  const search = document.getElementById("global-search");
  const lineage = window.lineage;
  const SUBS = ["flow", "inventory", "lint"];
  // Space of each address start; a rule shows in the Quality tab.
  const TARGETS = {
    map: "map", columns: "columns", packages: "packages", p: "package",
    quality: "quality", rules: "quality",
  };

  function route() {
    // `?q=…&base=…`: the Columns tab filters, kept in the address.
    const [path, query] = location.hash.slice(1).split("?");
    const [view, arg, sub, block] = path.split("/").map(decodeURIComponent);
    const target = TARGETS[view] ?? "overview";
    views.forEach((v) => (v.hidden = v.id !== target));
    // A package page sits under the Packages tab.
    const tab = target === "package" ? "packages" : target;
    for (const a of tabs) {
      if (a.dataset.tab === tab) a.setAttribute("aria-current", "page");
      else a.removeAttribute("aria-current");
    }
    if (target === "map") {
      lineage?.mount(document.querySelector("#map .stage-slot"));
      lineage?.map(arg);
    } else if (target === "columns") {
      window.columns?.show(arg, Number(sub), new URLSearchParams(query));
    } else if (target === "package") {
      showPackage(Number(arg), SUBS.includes(sub) ? sub : "flow", block == null ? null : Number(block));
    } else if (view === "rules") {
      showRule(arg);
    }
  }

  // Card of a rule (link from a finding): brought on screen and highlighted.
  function showRule(id) {
    const rule = id && document.getElementById(`rule-${id}`);
    document.querySelectorAll(".rule.focus").forEach((r) => r.classList.remove("focus"));
    if (!rule) return;
    rule.classList.add("focus");
    rule.scrollIntoView({ block: "center" });
  }

  // "See N findings": the Quality tab table filtered on the rule, severity shown.
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
      location.hash = "#quality";
      requestAnimationFrame(() => document.getElementById("lint-rows").scrollIntoView({ block: "start" }));
    })
  );

  // `block`: block of the flow to open (global search result).
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
    if (sub === "flow") {
      lineage?.mount(packageView.querySelector(".sub[data-sub='flow']"));
      lineage?.flow(index, Number.isInteger(block) ? block : null);
    } else if (sub === "inventory") {
      packageView.querySelector(".md-host").replaceChildren(rendered(template));
    } else {
      packageView.querySelector(".pkg-lint tbody").replaceChildren(...rows.map((r) => r.cloneNode(true)));
      packageView.querySelector(".sub[data-sub='lint'] .empty").hidden = rows.length > 0;
    }
  }

  // `<` neutralized: a package name must not be able to inject HTML.
  function rendered(template) {
    const markdown = template.content.querySelector("pre").textContent;
    const div = document.createElement("div");
    div.className = "md";
    if (window.marked) div.innerHTML = marked.parse(markdown.replace(/</g, "&lt;"));
    else div.append(Object.assign(document.createElement("pre"), { textContent: markdown }));
    return div;
  }

  addEventListener("hashchange", route);

  // Packages tab filter: package name or path, project name. Matching projects unfold;
  // filter cleared, everything folds back.
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

  // Lint findings of the Quality tab: severities and text filter.
  document.querySelectorAll("[data-severity]").forEach((box) =>
    box.addEventListener("change", () =>
      document.querySelector("#quality table.lint").classList.toggle("hide-" + box.dataset.severity, !box.checked)
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

  // Shortcuts: / searches everywhere (like Ctrl K), Escape closes the panel, F fits the graph.
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
