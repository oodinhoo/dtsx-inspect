# Contributing

Thanks for helping. This page says how to get a change into dtsx-inspect.

## Before you start

- Bugs and ideas go through the [issue forms](https://github.com/oodinhoo/dtsx-inspect/issues/new/choose).
  For a sizeable change, open an issue first so we agree on the approach before you write code.
- **Never share a real customer package or a real secret**, in an issue, a pull request or a test
  fixture. Reduce the problem to a small anonymized `.dtsx`: rename servers, databases, tables and
  columns, and replace passwords with a fake value such as `s3cr3t`.
- Security problems never go in a public issue: see [SECURITY.md](SECURITY.md).

## Build and test

Requires **Rust 1.88 or later** ([prerequisites](README.md#prerequisites)).

```sh
cargo fmt                                     # format
cargo clippy --all-targets -- -D warnings     # lint, warnings are errors
cargo test                                    # every test
```

The CI runs the same three commands on Windows, with `cargo fmt --check`. More commands are in
[Development](README.md#development).

Snapshot tests use [insta](https://insta.rs). After a deliberate change of an output, review and
accept the new snapshots with `cargo insta review`, and commit them with the change.

## Code

- `parser/` turns XML into the model with no business logic, `lineage/` builds the table and
  column graphs, `lint/` holds one rule per file behind the `Rule` trait, `render/` writes
  Markdown, JSON, Mermaid, CSV and HTML.
- A bug fix comes with a failing test first, on a small anonymized fixture in `tests/fixtures`.
- Every text shown in the HTML report is written in English and gets its French translation in
  `src/render/i18n.js`.
- A user-visible change gets a line under `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md), and the
  README (and `README.fr.md`) are updated when behavior changes.

## Commits and pull requests

- Branch from `main`, one topic per pull request: `main` only accepts pull requests whose CI passes.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/): `feat:`,
  `fix:`, `refactor:`, `perf:`, `test:`, `docs:`, `ci:`, `build:`, `chore:`, then a short summary
  in the imperative, in English.
- Pull requests are squash-merged: the title becomes the commit on `main`.

## License

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as in
[the README](README.md#license), without any additional terms or conditions.

By contributing, you also agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
