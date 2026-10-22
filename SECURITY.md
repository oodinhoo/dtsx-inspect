# Security policy

## Reporting a vulnerability

Please do **not** open a public issue for a security problem. Report it privately instead:
**Security** tab of this repository, then **Report a vulnerability**.

Examples of what to report:

- a password, secret or full connection string shown in an output (Markdown, JSON, Mermaid,
  CSV, HTML report) instead of being masked;
- text from a package that runs as script or HTML in the report;
- a package that makes `dtsx-inspect` crash or hang.

Never attach a real customer package or a real secret. Reduce the problem to a small
anonymized `.dtsx`: rename servers, databases, tables and columns, and replace passwords with
a fake value such as `s3cr3t`.

You will get an answer within a week. Fixes go to the latest version on `main`.
