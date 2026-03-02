# Security Policy

## Supported Versions

Only the latest release receives security fixes.

| Version | Supported |
|---------|-----------|
| latest  | Yes       |
| older   | No        |

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Report vulnerabilities privately via GitHub's built-in security advisory feature:
[https://github.com/Dalot/scavenger/security/advisories/new](https://github.com/Dalot/scavenger/security/advisories/new)

Include:
- A description of the vulnerability and its potential impact
- Steps to reproduce or a proof-of-concept
- Affected versions
- Any suggested mitigations, if known

You can expect an acknowledgement within **72 hours** and a status update within **7 days**.

## Scope

Areas of particular interest:

- **Daemon socket** — the Unix domain socket accepts connections from local processes; any path traversal or injection via socket messages is in scope.
- **SQLite databases** — the per-branch `.scavenger/` databases are written based on file paths and AST content; path injection or SQL injection is in scope.
- **`scavenger init` hook registration** — writing hooks into `.claude/` or `.cursor/` directories; any file write outside the intended directories is in scope.
- **Dependency vulnerabilities** — tracked automatically via `cargo deny` and the GitHub Advisory Database.

Out of scope: issues requiring physical access, social engineering, or attacks against third-party tools (Claude Code, Cursor).

## Disclosure Policy

Once a fix is available, we will:

1. Release a patched version.
2. Publish a GitHub Security Advisory.
3. Add an entry to `CHANGELOG.md`.

We follow coordinated disclosure and ask that you give us reasonable time to ship a fix before any public disclosure.
