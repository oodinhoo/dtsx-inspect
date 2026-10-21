# dtsx-inspect

**English** · [Français](README.fr.md)

A fast command-line analyzer for SQL Server Integration Services (SSIS) packages, written in Rust.
Point it at a `.dtsx` file or a whole folder of SSIS projects and it tells you what the packages
contain, where the data comes from and where it goes, and which anti-patterns they carry, without
SQL Server, Visual Studio or a database connection.

- **Inventory**: connections, tasks and containers, Data Flow components, variables, parameters
  and property expressions.
- **Table lineage**: files, tables and Dynamics entities → Data Flow components and SQL tasks →
  tables, across packages (Mermaid output, or an interactive map in the HTML report).
- **Column lineage**: where a column comes from and where it goes, through Data Flows and
  `INSERT … SELECT` statements, with a source → target mapping sheet for Excel.
- **Lint**: 11 rules for common SSIS anti-patterns, each with why it matters and how to fix it.
- **Execution order** inferred from the data, **package call graph**, and **lineage diff** between
  two versions of a folder (for example before and after a server migration).
- **HTML report**: a single standalone page with search, lineage map, column pages, package
  pages and lint findings, in **English or French** (switch in the sidebar).

Secrets never leave the package: passwords are masked everywhere, connection strings are reduced
to server, database and provider, and SQL text is only shown in the HTML report's details panel,
with passwords masked.

## Installation

You need a recent stable [Rust toolchain](https://rustup.rs/) (1.88 or later).

```sh
git clone <repository-url>
cd dtsx-inspect
cargo install --path .
```

Or build without installing: `cargo build --release`, the binary is
`target/release/dtsx-inspect` (`.exe` on Windows).

## Quick start

On Windows, **double-click** `dtsx-inspect.exe`: it asks for a folder, analyzes it and opens the
HTML report in your browser. From a terminal, run it without arguments for the same result.

```sh
dtsx-inspect inventory MyProject/            # Markdown inventory
dtsx-inspect lint MyProject/                 # anti-patterns, exit code 1 on errors
dtsx-inspect lineage MyProject/ --focus dbo.Customers > lineage.mmd
```

Every command takes a `.dtsx` file or a folder, walked recursively. `.conmgr` files (project
connections) and `.sql` files (database project scripts) found next to the packages are read too.
Visual Studio build copies (`obj/`, `bin/`) are skipped when the sources are present.

## Commands

| Command | Output |
|---|---|
| `inventory <path> [--format md\|json]` | Connections, tasks, Data Flow components, parameters, variables, expressions |
| `lineage <path> [--focus <text>]` | Mermaid `flowchart` of the table lineage; `--focus` keeps only what feeds or reads the matching tables, files or entities |
| `columns <path> --of <text>` | Upstream (←) and downstream (→) chain of the matching table or file columns, for instance `--of dbo.Customers.email` |
| `mapping <path> [--out file.csv]` | Source → target mapping as CSV (`;`, UTF-8 with BOM, opens directly in Excel), one row per column link with its status |
| `lint <path>` | One line per finding, `severity[rule] file: location: message`, then the totals |
| `calls <path>` | Mermaid graph of the Execute Package tasks and the packages they run |
| `order <path>` | Markdown execution order: waves of packages, cycles, shared tables set aside, sources to load first |
| `diff <before> <after>` | Markdown comparison of the column lineage of two folders, servers aside |

Exit codes: `0` on success, `1` when a file cannot be read, a search finds nothing, `lint` finds
an error, or `diff` detects a regression (a link or a package removed, or a link traced less
well). That makes `lint` and `diff` usable in CI.

Example of `columns`:

```text
Warehouse : dbo.Customers.email (srv-demo)
  ← write: DER Email.email_norm (LoadCustomers › DFT Customers)
    ← expression: FF Customers.email (LoadCustomers › DFT Customers)
      ← read: D:\demo\customers.csv.email
  → read: SRC Customers.email (Export › DFT Export)
```

## The HTML report

The report is a single HTML file. Its sections:

- **Overview**: figures, lineage reliability (share of direct, transformed, by-name and unknown
  column links), most flagged packages, projects.
- **Lineage map**: interactive graph of one project or all of them. Click a block for its
  upstream and downstream chain, its properties, its SQL query and its columns.
- **Columns**: every table and file column with who writes and reads it, filters (database,
  project, package, no origin, never read, several writers), a page per column and a matrix per
  table. Export the mapping as CSV.
- **Packages**: one page per package with its flow, inventory and findings.
- **Quality**: lint findings and one card per rule.

The language follows the browser (French browsers get French) and can be switched with the
**EN / FR** buttons in the sidebar. The choice is remembered.

The graph layout ([ELK](https://github.com/kieler/elkjs)), the Markdown rendering
([marked](https://github.com/markedjs/marked)) and the IBM Plex fonts load from CDNs, pinned with
Subresource Integrity hashes. Offline, everything works except the graph and the inventory
formatting.

## Lint rules

| Rule | Severity | Reports |
|---|---|---|
| `unresolved-connection` | error | Connection reference not found, or marked `:invalid` by SSDT |
| `no-fast-load` | warning | OLE DB destination inserting row by row instead of fast load |
| `obsolete-provider` | warning | Discontinued OLE DB provider (SQLNCLI, SQLOLEDB) |
| `insert-select-star` | warning | `INSERT … SELECT *` in an SQL task (columns matched by position) |
| `row-by-row-command` | warning | OLE DB Command in a Data Flow (one query per row) |
| `sort-in-data-flow` | warning | Blocking Sort component |
| `unrenamed-package` | warning | Package still named `Package` |
| `unused-columns` | warning | Source in table mode or `SELECT *` bringing unused columns into the flow |
| `default-name` | info | Task or component still named by the designer (`Data Flow Task 1`…) |
| `disabled-task` | info | Disabled task |
| `incomplete-lineage` | info | Step the lineage could not trace (SQL not parsed, query in a variable…) |

## What is supported

- SSIS 2012 and later (package format 6+), including SSIS 2012/2014 components identified by GUID.
- Data Flow sources, transformations and destinations from Microsoft; third-party components
  (KingswaySoft, Script components…) are kept as `Unknown` with their class ID, never a crash.
  KingswaySoft Dynamics 365 sources are traced to their entities, FetchXML included.
- SQL from Execute SQL tasks, OLE DB / ADO.NET sources, Lookups and OLE DB Commands, typed in
  directly, built by an expression (dashed in the lineage: design-time value) or read from a
  variable. T-SQL is parsed with [sqlparser](https://github.com/apache/datafusion-sqlparser-rs),
  with a MySQL fallback for ADO.NET sources.
- Stored procedures and views defined in `.sql` scripts (UTF-8, UTF-8 with BOM, UTF-16) are
  expanded in the lineage.

Known limits: queries read from files are not traced, derived tables (`FROM (SELECT …) x`) end
the column lineage, and `MERGE` / `UPDATE` are traced at table level only. The `incomplete-lineage`
rule lists every step the lineage could not follow.

## Development

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

The code is organized as `parser/` (XML → model, no business logic), `lineage/` (table and column
graphs, execution order), `lint/` (one rule per file, `Rule` trait) and `render/` (Markdown,
JSON, Mermaid, CSV, HTML). Tests use small anonymized `.dtsx` fixtures in `tests/fixtures` and
[insta](https://insta.rs) snapshots. Never add real customer packages as fixtures.

Contributions are welcome: open an issue with an anonymized package that shows the problem, or a
pull request with a failing test first.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
