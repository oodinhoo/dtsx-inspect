// Global search of the header (Ctrl K or /): columns, tables and files, packages,
// components and tasks. Each result is a link to its page; arrows to choose, Enter to
// open, Escape to close. Every typed word must appear, case-insensitive.
(() => {
  const input = document.getElementById("global-search");
  const box = document.getElementById("search-results");
  if (!input || !box) return;
  const data = window.lineage?.data;
  const esc = (text) =>
    String(text).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const GROUPS = ["Columns", "Tables and files", "Packages", "Components and tasks", "Component columns"];
  const PER_GROUP = 6;
  const DATASETS = ["table", "file", "entity", "proc"];

  // Built on the first search: the report opens without waiting.
  let index = null;
  function build() {
    const items = [];
    for (const a of document.querySelectorAll("#packages a[data-pkg]")) {
      items.push({ group: 2, text: a.querySelector(".name").textContent, where: a.title, href: a.getAttribute("href") });
    }
    data?.nodes.forEach((n, i) => {
      if (DATASETS.includes(n.kind)) items.push({ group: 1, text: n.title, where: n.sub, href: `#columns/t/${i}` });
      else if (n.package != null) {
        items.push({ group: 3, text: n.title, where: `${n.sub} · ${data.packages[n.package] ?? "?"}`, href: `#p/${n.package}/flow/${i}` });
      }
    });
    data?.columns.forEach(([, full, step], i) => items.push({ group: step ? 4 : 0, text: full, where: "", href: `#columns/c/${i}` }));
    for (const item of items) item.hay = `${item.text} ${item.where}`.toLowerCase();
    return items;
  }

  // First typed word highlighted, the rest escaped.
  function highlight(text, word) {
    const at = text.toLowerCase().indexOf(word);
    if (at < 0) return esc(text);
    return esc(text.slice(0, at)) + `<mark>${esc(text.slice(at, at + word.length))}</mark>` + esc(text.slice(at + word.length));
  }

  let options = [];
  let active = -1;

  function render() {
    const words = input.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return close();
    index ??= build();
    const groups = GROUPS.map(() => []);
    for (const item of index) {
      if (words.every((w) => item.hay.includes(w))) groups[item.group].push(item);
    }
    // The name starting with the typed word first, then the shortest.
    const rank = (item) => [item.text.toLowerCase().startsWith(words[0]) ? 0 : 1, item.text.length];
    const parts = groups.map((hits, g) => {
      if (!hits.length) return "";
      hits.sort((a, b) => {
        const [ra, rb] = [rank(a), rank(b)];
        return ra[0] - rb[0] || ra[1] - rb[1] || a.text.localeCompare(b.text);
      });
      const links = hits.slice(0, PER_GROUP).map((item) => `<a role="option" href="${esc(item.href)}">`
        + `<span class="text">${highlight(item.text, words[0])}</span>`
        + (item.where ? `<span class="where">${esc(item.where)}</span>` : "") + "</a>");
      const more = hits.length > PER_GROUP ? `<p class="more">${hits.length - PER_GROUP} more: refine the search</p>` : "";
      return `<div role="group" aria-label="${GROUPS[g]}"><p class="group-title">${GROUPS[g]} <span class="count">${hits.length}</span></p>${links.join("")}${more}</div>`;
    });
    box.innerHTML = parts.join("") || '<p class="empty">No results.</p>';
    options = [...box.querySelectorAll("[role=option]")];
    options.forEach((o, i) => (o.id = `search-option-${i}`));
    move(options.length ? 0 : -1);
    box.hidden = false;
    input.setAttribute("aria-expanded", "true");
  }

  function move(to) {
    options[active]?.classList.remove("active");
    active = to;
    const option = options[active];
    option?.classList.add("active");
    option?.scrollIntoView({ block: "nearest" });
    if (option) input.setAttribute("aria-activedescendant", option.id);
    else input.removeAttribute("aria-activedescendant");
  }

  function close() {
    box.hidden = true;
    options = [];
    active = -1;
    input.setAttribute("aria-expanded", "false");
    input.removeAttribute("aria-activedescendant");
  }

  let typing;
  input.addEventListener("input", () => {
    clearTimeout(typing);
    typing = setTimeout(render, 80);
  });
  input.addEventListener("focus", () => input.value.trim() && render());
  input.addEventListener("blur", close);
  // Click on a result: the field keeps the focus until the navigation.
  box.addEventListener("pointerdown", (event) => event.preventDefault());
  box.addEventListener("click", (event) => {
    if (event.target.closest("[role=option]")) {
      close();
      input.blur();
    }
  });
  input.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (box.hidden) return render();
      if (options.length) move((active + (event.key === "ArrowDown" ? 1 : -1) + options.length) % options.length);
    } else if (event.key === "Enter") {
      const option = options[active];
      if (!option) return;
      event.preventDefault();
      location.hash = option.getAttribute("href");
      close();
      input.blur();
    } else if (event.key === "Escape") {
      close();
      input.blur();
    }
  });
  addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      input.focus();
      input.select();
    }
  });
})();
