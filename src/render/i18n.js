// Report language: English as written by dtsx-inspect, or French. The choice comes from
// the EN / FR buttons of the rail (kept in local storage), otherwise from the browser
// language. French is applied by translating the text nodes and a few attributes, then
// everything the other scripts render (MutationObserver): they stay written in English.
// shortcut: switching language reloads the page (the address keeps the view), the
// translation is not reversible in place.
(() => {
  const KEY = "dtsx-inspect.lang";
  let lang = null;
  try {
    lang = localStorage.getItem(KEY);
  } catch {}
  if (lang !== "en" && lang !== "fr") lang = (navigator.language || "").toLowerCase().startsWith("fr") ? "fr" : "en";
  document.documentElement.lang = lang;
  document.querySelectorAll("[data-lang]").forEach((button) => {
    button.setAttribute("aria-pressed", button.dataset.lang === lang);
    button.addEventListener("click", () => {
      try {
        localStorage.setItem(KEY, button.dataset.lang);
      } catch {}
      if (button.dataset.lang !== lang) location.reload();
    });
  });
  if (lang !== "fr") return;

  // French plural: 0 and 1 are singular.
  const n = (count, one, many) => `${count} ${Number(count) <= 1 ? one : many}`;
  const HOW = {
    read: "lecture", write: "écriture", expression: "expression", conversion: "conversion",
    "Union All": "Unir tout", lookup: "recherche", "SQL expression": "calcul SQL", "SELECT *": "SELECT *",
  };
  const ROLE = { writes: "écrit", reads: "lit", "computes from": "calcule à partir de" };
  const SUB = {
    File: "Fichier", Table: "Table", Entity: "Entité", Procedure: "Procédure", "SQL task": "Tâche SQL",
    "Not traced": "Non suivi",
  };

  const EXACT = new Map([
    // Rail and tabs
    ["Overview", "Vue d'ensemble"], ["Lineage map", "Carte du lignage"], ["Columns", "Colonnes"],
    ["Quality", "Qualité"], ["Language", "Langue"], ["Global search", "Recherche globale"],
    ["Search… (Ctrl K)", "Rechercher… (Ctrl K)"], ["Results", "Résultats"],
    ["Report sections", "Sections du rapport"], ["Lint findings", "Constats de lint"],
    // Graph
    ["Graph unavailable: the layout library could not be loaded (no internet access?).",
      "Graphe indisponible : la bibliothèque de mise en page n'a pas pu être chargée (poste sans internet ?)."],
    ["No dataset or traced step for this package.", "Aucun jeu de données ni étape suivie pour ce package."],
    ["Search the graph… (Enter)", "Rechercher dans le graphe… (Entrée)"], ["Zoom in", "Zoomer"],
    ["Zoom out", "Dézoomer"], ["Fit to screen (F)", "Ajuster à l'écran (F)"], ["Legend", "Légende"],
    ["Details", "Détails"], ["All projects", "Tous les projets"], ["All projects (slow)", "Tous les projets (lent)"],
    ["(root)", "(racine)"], ["Dynamics entity", "Entité Dynamics"], ["Stored procedure", "Procédure stockée"],
    ["Source component", "Composant source"], ["Destination component", "Composant destination"],
    ["Untraced step", "Étape non suivie"], ["not traced", "non suivi"], ["None", "Aucun"], ["Projects", "Projets"],
    ["Show the whole graph", "Afficher tout le graphe"], ["Isolate the chain", "Isoler la chaîne"],
    ["See in the inventory", "Voir dans l'inventaire"], ["Close (Esc)", "Fermer (Échap)"], ["Close", "Fermer"],
    ["Built by an expression: the value shown is the design-time one, it can differ at run time.",
      "Construit par expression : la valeur affichée est celle de la conception, elle peut différer à l'exécution."],
    ["Properties", "Propriétés"], ["Copy", "Copier"], ["Copied", "Copié"], ["Fed by", "Alimenté par"],
    ["Upstream", "En amont"], ["Consumed by", "Consommé par"], ["Downstream", "En aval"],
    ["Open the column page", "Ouvrir la page de la colonne"],
    ["Click a column: where it comes from (←), where it goes (→).", "Cliquer une colonne : d'où elle vient (←), où elle va (→)."],
    ["No linked column: neither origin nor use known.", "Aucune colonne liée : ni origine ni usage connus."],
    ["Drag", "Glisser"], ["pan", "déplacer"], ["Wheel", "Molette"], ["zoom", "zoomer"], ["Click", "Clic"],
    ["details", "détails"], ["see all", "tout voir"], ["Esc", "Échap"], ["close", "fermer"], ["Enter", "Entrée"],
    ["search", "chercher"], ["Frame grouping the components of a Data Flow", "Cadre qui regroupe les composants d'un Data Flow"],
    ["lineage interrupted (reason on click)", "lignage interrompu (raison au clic)"],
    ["query computed by an expression", "requête calculée par expression"],
    ["shared table, one copy per package", "table partagée, une copie par package"], ["data", "données"],
    ["by expression", "par expression"], ["table creation", "création de table"], ["selected chain", "chaîne sélectionnée"],
    ["Blocks", "Blocs"], ["Markers", "Repères"], ["Links", "Liens"], ["Controls", "Commandes"],
    ["Entity", "Entité"], ["Procedure", "Procédure"], ["SQL task", "Tâche SQL"], ["Not traced", "Non suivi"],
    // How a column gives the next one (`.how` of the column page)
    ["read", "lecture"], ["write", "écriture"], ["lookup", "recherche"], ["SQL expression", "calcul SQL"],
    ["Union All", "Unir tout"],
    // Blocks of the lineage (graph.rs)
    ["Path", "Chemin"], ["Database", "Base"], ["Connection", "Connexion"], ["Server", "Serveur"],
    ["Class", "Classe"], ["By expression", "Par expression"], ["Query", "Requête"], ["SQL query", "Requête SQL"],
    ["typed in directly", "saisie directe"], ["expression (design-time value)", "expression (valeur à la conception)"],
    ["file", "fichier"], ["File connection", "Connexion du fichier"],
    // Columns tab
    ["No known origin", "Sans origine connue"], ["Never read", "Jamais lue"],
    ["Written by 2+ packages", "Écrite par 2+ packages"], ["Search", "Chercher"],
    ["Column or table name", "Nom de colonne ou de table"], ["All", "Tous"], ["Project", "Projet"],
    ["Layout", "Affichage"], ["Flat", "À plat"], ["By table", "Par table"],
    ["Each written column, the column read that gives it, the package and the transformations: one file for Excel",
      "Chaque colonne écrite, la colonne lue qui la donne, le package et les transformations : un fichier pour Excel"],
    ["Export the mapping (CSV)", "Exporter le mapping (CSV)"], ["To check", "À vérifier"],
    ["Clear the filters", "Effacer les filtres"],
    ["The column lineage needs the report scripts.", "Le lignage par colonne a besoin des scripts du rapport."],
    ["Files", "Fichiers"], ["Dynamics entities", "Entités Dynamics"], ["writes", "écrit"], ["reads", "lit"],
    ["computes from", "calcule à partir de"], ["Column", "Colonne"], ["Database · server", "Base · serveur"],
    ["Written by", "Écrite par"], ["Read by", "Lue par"], ["Direct origins", "Origines directes"],
    ["Direct uses", "Usages directs"], ["Databases · server", "Bases de données · serveur"],
    ["File folders", "Dossiers de fichiers"], ["Dynamics connections", "Connexions Dynamics"],
    ["No traced column in these packages.", "Aucune colonne suivie dans ces packages."],
    ["No column matches.", "Aucune colonne ne correspond."], ["origins and uses", "origines et usages"],
    ["Table matrix", "Matrice de la table"], ["No known origin.", "Aucune origine connue."],
    ["No known use.", "Aucun usage connu."], ["unknown origin", "origine inconnue"], ["loop", "boucle"],
    ["Component", "Composant"], ["See the step in the package flow", "Voir l'étape dans le flux du package"],
    ["Copy the trace", "Copier la trace"], ["Where it comes from", "D'où elle vient"], ["Where it goes", "Où elle va"],
    ["No known package.", "Aucun package connu."], ["Other columns of the step", "Autres colonnes de l'étape"],
    ["Other columns of the table", "Autres colonnes de la table"], ["does not touch it", "ne la touche pas"],
    ["No traced column for this dataset.", "Aucune colonne suivie pour ce jeu de données."],
    ["Who writes, reads or computes each column: what breaks if it changes.",
      "Qui écrit, lit ou calcule chaque colonne : ce qui casse si on la change."],
    ["Traced columns", "Colonnes suivies"],
    // Global search
    ["Tables and files", "Tables et fichiers"], ["Components and tasks", "Composants et tâches"],
    ["Component columns", "Colonnes de composants"], ["No results.", "Aucun résultat."],
    // Packages tab, package page and inventory
    ["No package.", "Aucun package."], ["Filter the packages", "Filtrer les packages"],
    ["Filter by project, package or file…", "Filtrer par projet, package ou fichier…"],
    ["Package views", "Vues du package"], ["Flow", "Flux"], ["Inventory", "Inventaire"],
    ["No finding for this package.", "Aucun constat pour ce package."], ["Connections", "Connexions"],
    ["Parameters", "Paramètres"], ["Tasks", "Tâches"], ["Name", "Nom"], ["Scope", "Portée"], ["Target", "Cible"],
    ["Value", "Valeur"], ["Owner", "Élément"], ["Property", "Propriété"], ["Disabled", "Désactivée"],
    ["Called package", "Package appelé"], ["Role", "Rôle"], ["yes", "oui"], ["(sensitive)", "(sensible)"],
    ["None.", "Aucune."],
    // Overview
    ["connections", "connexions"], ["tasks and containers", "tâches et conteneurs"],
    ["third-party components", "composants tiers"], ["lint errors", "erreurs de lint"],
    ["warnings", "avertissements"], ["unreadable files", "fichiers illisibles"], ["Unreadable files", "Fichiers illisibles"],
    ["Lineage reliability", "Fiabilité du lignage"], ["Direct", "Directe"], ["Transformed", "Transformée"],
    ["By name (SELECT *)", "Par nom (SELECT *)"], ["Unknown origin", "Origine inconnue"],
    ["\"By name\": an", "« Par nom » : un"],
    [", right only if the columns of both tables are in the same order (", ", juste seulement si les colonnes des deux tables sont dans le même ordre ("],
    ["rule", "règle"], ["Most flagged", "Les plus signalés"], ["File", "Fichier"], ["Errors", "Erreurs"],
    ["Warnings", "Avertissements"],
    // Quality tab
    ["No finding in this report", "Aucun constat dans ce rapport"], ["Why", "Pourquoi"], ["Fix", "Corriger"],
    ["Lint rules", "Règles de lint"],
    ["Each finding of the report comes from one of these rules. Error: the package will fail or does something other than intended. Warning: it works, but slowly or in a fragile way. Info: good to know, not urgent.",
      "Chaque constat du rapport vient d'une de ces règles. Erreur : le package échouera ou fait autre chose que prévu. Avertissement : il marche, mais lentement ou de façon fragile. Info : à savoir, sans urgence."],
    ["No finding.", "Aucun constat."], ["See the rule", "Voir la règle"],
    ["Filter by file, task, message…", "Filtrer par fichier, tâche, message…"], ["Severity", "Gravité"],
    ["Rule", "Règle"], ["Location", "Emplacement"], ["error", "erreur"], ["warning", "avertissement"],
    // Rule cards (src/lint)
    ["Task or component still named as the designer named it", "Tâche ou composant resté sous le nom donné par le concepteur"],
    ["\"Data Flow Task 1\", \"Derived Column\"…: these names say nothing in the execution logs, the error messages or this report.",
      "« Data Flow Task 1 », « Derived Column »… : ces noms ne disent rien dans les journaux d'exécution, les messages d'erreur ni ce rapport."],
    ["Rename following the team convention: a type prefix (DFT_, SQL_, LKP_, DER_…) followed by what the step does.",
      "Renommer selon la convention de l'équipe : un préfixe de type (DFT_, SQL_, LKP_, DER_…) suivi de ce que fait l'étape."],
    ["Disabled task", "Tâche désactivée"],
    ["Dead code that stays deployed, or a task someone forgot to enable again before going to production.",
      "Code mort qui reste déployé, ou tâche qu'on a oublié de réactiver avant la mise en production."],
    ["Delete the task if it is no longer used; otherwise enable it again, or note in an annotation why it is disabled.",
      "Supprimer la tâche si elle ne sert plus ; sinon la réactiver, ou noter dans une annotation pourquoi elle est désactivée."],
    ["Step the lineage could not trace", "Étape que le lignage n'a pas pu suivre"],
    ["SQL that cannot be parsed, query read from a variable or a file, stored procedure without definition: the lineage documentation has a blind spot there.",
      "SQL non analysable, requête lue dans une variable ou un fichier, procédure stockée sans définition : la documentation du lignage a un angle mort à cet endroit."],
    ["Nothing mandatory. To fill the gap: add the .sql scripts of the procedures to the analyzed folder, or replace the query in a variable by a query typed in directly.",
      "Rien d'obligatoire. Pour combler le trou : ajouter au dossier analysé les scripts .sql des procédures, ou remplacer la requête en variable par une requête saisie directement."],
    ["INSERT … SELECT * in an SQL task", "INSERT … SELECT * dans une tâche SQL"],
    ["Columns are matched by their position: a column added or reordered on one side shifts the data or makes the load fail without a clear message.",
      "Les colonnes sont appariées par leur position : une colonne ajoutée ou réordonnée d'un côté décale les données ou fait échouer le chargement sans message clair."],
    ["List the columns on both sides: INSERT INTO target (a, b) SELECT a, b FROM source.",
      "Lister les colonnes des deux côtés : INSERT INTO cible (a, b) SELECT a, b FROM source."],
    ["OLE DB destination inserting row by row", "Destination OLE DB en insertion ligne à ligne"],
    ["In \"Table or view\" or \"Table or view - variable\" mode, the destination sends one INSERT per row: the load is dozens of times slower and fills the transaction log.",
      "En mode « Table ou vue » ou « Table ou vue - variable », la destination envoie un INSERT par ligne : le chargement est des dizaines de fois plus lent et charge le journal de transactions."],
    ["Choose \"Table or view - fast load\", then tune the batch sizes (Rows per batch, Maximum insert commit size).",
      "Choisir « Table ou vue - chargement rapide » (fast load), puis régler la taille des lots (Rows per batch, Maximum insert commit size)."],
    ["Discontinued OLE DB provider (SQLNCLI, SQLOLEDB)", "Fournisseur OLE DB abandonné (SQLNCLI, SQLOLEDB)"],
    ["SQL Server Native Client and SQLOLEDB are no longer maintained by Microsoft: no security fixes, and no guaranteed support for recent TLS and SQL Server versions.",
      "SQL Server Native Client et SQLOLEDB ne sont plus maintenus par Microsoft : ni correctifs de sécurité, ni prise en charge garantie des versions récentes de TLS et de SQL Server."],
    ["Switch the connection to the MSOLEDBSQL provider (Microsoft OLE DB Driver for SQL Server).",
      "Passer la connexion au fournisseur MSOLEDBSQL (Microsoft OLE DB Driver for SQL Server)."],
    ["OLE DB Command in a Data Flow", "OLE DB Command dans un Data Flow"],
    ["The component runs its query once per row: as many round trips to the server as rows, which gets very slow from a few thousand rows on.",
      "Le composant exécute sa requête une fois par ligne : autant d'allers-retours avec le serveur que de lignes, ce qui devient très lent dès quelques milliers de lignes."],
    ["Load the rows into a staging table, then apply a single set-based query (UPDATE … FROM, MERGE) in an SQL task after the Data Flow.",
      "Charger les lignes dans une table de transit, puis appliquer une seule requête ensembliste (UPDATE … FROM, MERGE) dans une tâche SQL après le Data Flow."],
    ["Sort component in a Data Flow", "Composant Tri dans un Data Flow"],
    ["Sorting is blocking: it keeps every row in memory before letting one through, and spills to disk with large volumes.",
      "Le tri est bloquant : il garde toutes les lignes en mémoire avant d'en laisser passer une, et déborde sur disque avec les gros volumes."],
    ["Sort in the source query (ORDER BY), then declare the output sorted (IsSorted, SortKeyPosition) in the advanced editor of the source.",
      "Trier dans la requête source (ORDER BY), puis déclarer la sortie triée (IsSorted, SortKeyPosition) dans l'éditeur avancé de la source."],
    ["Package still named \"Package\"", "Package resté nommé « Package »"],
    ["The internal package name shows up in the SSIS catalog and the logs: several \"Package\" packages are indistinguishable there.",
      "Le nom interne du package apparaît dans le catalogue SSIS et les journaux : plusieurs packages « Package » y sont indiscernables."],
    ["In the package properties, set Name to the name of the .dtsx file.", "Dans les propriétés du package, donner à Name le nom du fichier .dtsx."],
    ["Connection reference not found or marked invalid", "Référence de connexion introuvable ou marquée invalide"],
    ["The task or component points to a deleted or renamed connection, or one SSDT marked \":invalid\" because it could not find it on opening: the package fails at validation or at run time.",
      "La tâche ou le composant pointe vers une connexion supprimée ou renommée, ou que SSDT a marquée « :invalid » faute de la trouver à l'ouverture : le package échoue à la validation ou à l'exécution."],
    ["Open the package in SSDT and reassign the connection of the task or component, or recreate the missing connection manager.",
      "Ouvrir le package dans SSDT et réassocier la connexion de la tâche ou du composant, ou recréer le gestionnaire de connexion manquant."],
    ["Source or lookup reading every column", "Source ou recherche qui lit toutes les colonnes"],
    ["A source in table or view mode, or a SELECT * query, brings unused columns into the flow: more memory per row, fewer rows per buffer, and a package that breaks when the table changes.",
      "Une source en mode table ou vue, ou une requête SELECT *, fait passer dans le flux des colonnes inutiles : plus de mémoire par ligne, moins de lignes par tampon, et un package qui casse quand la table change."],
    ["Write a SELECT query listing only the columns used further down the flow.",
      "Écrire une requête SELECT qui ne liste que les colonnes utilisées par la suite du flux."],
    // Lint messages and lineage notes
    ["row-by-row insert: choose \"Table or view - fast load\"", "insertion ligne à ligne : choisir « Table ou vue - chargement rapide »"],
    ["source in table or view mode: write a SELECT of the useful columns only", "source en mode table ou vue : écrire un SELECT des seules colonnes utiles"],
    ["SELECT *: list the useful columns only", "SELECT * : lister les seules colonnes utiles"],
    ["one query per row: load a staging table then run a single set-based query", "une requête par ligne : charger une table de transit puis une seule requête ensembliste"],
    ["INSERT … SELECT *: list the columns on both sides", "INSERT … SELECT * : lister les colonnes des deux côtés"],
    ["designer default name: rename it following the convention (DFT_, SQL_, LKP_…)", "nom par défaut du concepteur : le renommer selon la convention (DFT_, SQL_, LKP_…)"],
    ["default name \"Package\", rename it after its file", "nom par défaut « Package », le renommer comme son fichier"],
    ["disabled task, delete it if it is no longer used", "tâche désactivée, à supprimer si elle ne sert plus"],
    ["blocking in-memory sort: sort in the source query (ORDER BY) and declare IsSorted", "tri bloquant en mémoire : trier dans la requête source (ORDER BY) et déclarer IsSorted"],
    ["source without an identified dataset", "source sans jeu de données identifié"],
    ["destination without an identified dataset", "destination sans jeu de données identifié"],
    ["SQL not analyzed", "SQL non analysé"], ["query in a file, not traced", "requête dans un fichier, non suivie"],
    ["Dynamics destination not traced", "destination Dynamics non suivie"],
    ["table or query in a variable, not traced", "table ou requête dans une variable, non suivie"],
    // Unreadable files
    ["no .dtsx package found", "aucun package .dtsx trouvé"],
  ]);

  // Texts built with names or counts: first matching pattern wins.
  const PATTERNS = [
    [/^(\d+) errors?$/, (_, c) => n(c, "erreur", "erreurs")],
    [/^(\d+) warnings?$/, (_, c) => n(c, "avertissement", "avertissements")],
    [/^See (\d+) findings?$/, (_, c) => `Voir ${n(c, "constat", "constats")}`],
    [/^(\d+) links? column to column$/, (_, c) => `${n(c, "lien", "liens")} colonne à colonne`],
    [/^format (\d+) · (\d+) tasks? · (\d+) Data Flows?$/, (_, f, t, d) => `format ${f} · ${n(t, "tâche", "tâches")} · ${n(d, "Data Flow", "Data Flows")}`],
    [/^(\d+) more: refine the search$/, (_, c) => `${n(c, "autre", "autres")} : préciser la recherche`],
    [/^(\d+) columns? of (\d+)$/, (_, c, total) => `${n(c, "colonne", "colonnes")} sur ${total}`],
    [/^(\d+) traced columns?$/, (_, c) => n(c, "colonne suivie", "colonnes suivies")],
    [/^(\d+) columns: the first (\d+) are shown\. Refine the search, the database or the package to narrow down\.$/,
      "$1 colonnes : les $2 premières sont affichées. Préciser la recherche, la base ou le package pour affiner."],
    [/^(\d+) other datasets?: refine the search\.$/, (_, c) => `${n(c, "autre jeu", "autres jeux")} de données : affiner la recherche.`],
    [/^List cut at (\d+) steps: dtsx-inspect columns --of for the rest\.$/, "Liste tronquée à $1 étapes : dtsx-inspect columns --of pour la suite."],
    [/^… chain cut at (\d+) lines \(dtsx-inspect columns --of for the rest\)$/, "… chaîne tronquée à $1 lignes (dtsx-inspect columns --of pour la suite)"],
    [/^Columns \((\d+)\)$/, "Colonnes ($1)"],
    // Sorted column header: `Column ▲`.
    [/^(.+) (▲|▼)$/, (_, label, arrow) => `${EXACT.get(label) ?? label} ${arrow}`],
    [/^(\d+) direct, (\d+) transformed, (\d+) by name \(SELECT \*\), (\d+) unknown origin$/,
      "$1 directes, $2 transformées, $3 par nom (SELECT *), $4 d'origine inconnue"],
    [/^See the flow of package (.+)$/, "Voir le flux du package $1"],
    [/^Touched by (\d+) packages: the map shows one copy per package, with its own links only\. The flow of each package details them\.$/,
      "Touché par $1 packages : la carte en montre une copie par package, avec ses seuls liens. Le flux de chaque package les détaille."],
    [/^(.+): hover to spot them$/, (_, kind) => `${EXACT.get(kind) ?? kind} : survoler pour les repérer`],
    [/^(File|Table|Entity|Procedure|SQL task|Not traced)( · .+)$/, (_, kind, rest) => SUB[kind] + rest],
    [/^produces the column · (.+)$/, "produit la colonne · $1"],
    [/^((?:writes|reads|computes from)(?:, (?:writes|reads|computes from))*) · (.+)$/,
      (_, roles, project) => `${roles.split(", ").map((r) => ROLE[r]).join(", ")} · ${project}`],
    [/^(←|→) (read|write|expression|conversion|Union All|lookup|SQL expression|SELECT \*): (.+?)(, unknown origin)?( \(loop\))?$/,
      (_, arrow, how, rest, unknown, loop) => `${arrow} ${HOW[how]} : ${rest}${unknown ? ", origine inconnue" : ""}${loop ? " (boucle)" : ""}`],
    [/^connection (.+) not found$/, "connexion $1 introuvable"],
    [/^reference to "(.+)" marked invalid: reassign it in SSDT$/, "référence vers « $1 » marquée invalide : la réassocier dans SSDT"],
    [/^provider (.+) discontinued by Microsoft, switch to MSOLEDBSQL$/, "fournisseur $1 abandonné par Microsoft, passer à MSOLEDBSQL"],
    [/^stored procedure (.+) defined in several scripts \((.+)\), not traced$/, "procédure stockée $1 définie dans plusieurs scripts ($2), non suivie"],
    [/^stored procedure (.+), not traced$/, "procédure stockée $1, non suivie"],
    [/^query in variable (.+), not traced$/, "requête dans la variable $1, non suivie"],
    [/^connection (.+)$/, "connexion $1"],
    [/^(.+) \(unresolved\)$/, "$1 (non résolue)"],
    [/^(.+): this file is not an SSIS package$/, "$1 : ce fichier n'est pas un package SSIS"],
    [/^(.+): this file is not an SSIS connection manager$/, "$1 : ce fichier n'est pas un gestionnaire de connexion SSIS"],
    [/^(.+): unsupported package format (\d+) \(SSIS 2012 or later required\)$/, "$1 : format de package $2 non supporté (SSIS 2012 ou plus récent requis)"],
    [/^(.+): invalid XML: (.+)$/, "$1 : XML invalide : $2"],
    [/^(.+): no \.dtsx package found$/, "$1 : aucun package .dtsx trouvé"],
  ];

  function one(text) {
    const trimmed = text.trim();
    if (!trimmed || !/[a-z]/i.test(trimmed)) return null;
    let out = EXACT.get(trimmed);
    if (out === undefined) {
      const hit = PATTERNS.find(([re]) => re.test(trimmed));
      if (hit) out = trimmed.replace(hit[0], hit[1]);
    }
    return out === undefined || out === trimmed ? null : text.replace(trimmed, () => out);
  }
  // A lineage note can hold several reasons, one per line.
  function translated(text) {
    if (!text.includes("\n")) return one(text);
    const out = text.split("\n").map((line) => one(line) ?? line).join("\n");
    return out === text ? null : out;
  }

  // Names, paths, SQL and rule identifiers stay as they are.
  const SKIP = "script, style, code, pre, textarea, [translate=no]";
  const ATTRS = ["title", "placeholder", "aria-label", "label"];
  function attributes(el) {
    for (const name of ATTRS) {
      const value = el.getAttribute(name);
      const out = value && translated(value);
      if (out) el.setAttribute(name, out);
    }
  }
  function text(node) {
    const out = translated(node.nodeValue);
    if (out) node.nodeValue = out;
  }
  function translate(root) {
    const el = root.nodeType === Node.TEXT_NODE ? root.parentElement : root;
    if (!el || el.nodeType !== Node.ELEMENT_NODE || el.closest(SKIP)) return;
    if (root.nodeType === Node.TEXT_NODE) return text(root);
    attributes(root);
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT, {
      acceptNode: (node) => (node.nodeType === Node.ELEMENT_NODE && node.matches(SKIP) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
    });
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      if (node.nodeType === Node.TEXT_NODE) text(node);
      else attributes(node);
    }
  }

  translate(document.body);
  // Translated texts are no keys: the changes made here end the loop by themselves.
  new MutationObserver((records) => {
    for (const record of records) {
      if (record.type === "childList") record.addedNodes.forEach(translate);
      else if (record.type === "characterData") translate(record.target);
      else if (!record.target.closest(SKIP)) attributes(record.target);
    }
  }).observe(document.body, { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ATTRS });
})();
