# Changelog

Notable changes of each version. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) (before 1.0, a minor version may change outputs).

## [Unreleased]

## [0.2.0] - 2026-10-22

### Added

- `report <path> [--out file.html]`: writes the standalone HTML report to a file, to share it or
  publish it from a CI. Without `--out`, it opens in the browser like the no-argument mode.

### Tests

- Canary test: a secret planted in every form a package can hold it (project and package
  connection strings, connection expression, parameters, sensitive parameter, variables, SQL
  tasks, property expressions, Data Flow SQL, UTF-16 script) must appear in no command output
  and no written file.

## [0.1.0] - 2026-10-22

First public release.

### Added

- Commands: `inventory` (Markdown or JSON), `lineage` (Mermaid, `--focus`), `columns`, `mapping`
  (CSV for Excel), `lint` (11 rules), `calls`, `order`, `diff`.
- Without arguments: folder picker, then the standalone HTML report (overview, lineage map,
  column pages, package pages, lint findings) in English or French.
- Passwords and connection strings masked in every output.
- Windows executable built by GitHub Actions, with SHA-256 checksum and build provenance
  attestation.

### Known limitations

- Not traced: queries read from files, KingswaySoft Dynamics destinations. Derived tables
  (`FROM (SELECT …) x`) end the column lineage; `MERGE` / `UPDATE` are traced at table level
  only. The `incomplete-lineage` lint rule lists every step the lineage could not follow.
- The graph of the HTML report loads its layout library from a CDN: offline, everything works
  except the graph.
- The Windows executable is not code-signed: Windows SmartScreen warns on first launch.

[Unreleased]: https://github.com/oodinhoo/dtsx-inspect/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/oodinhoo/dtsx-inspect/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/oodinhoo/dtsx-inspect/releases/tag/v0.1.0
