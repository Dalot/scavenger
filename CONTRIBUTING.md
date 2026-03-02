# Contributing to Scavenger

Thanks for taking the time to contribute. This document covers everything you need to get started.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Reporting Bugs](#reporting-bugs)
- [Requesting Features](#requesting-features)
- [Development Setup](#development-setup)
- [Making Changes](#making-changes)
- [Tests](#tests)
- [Submitting a Pull Request](#submitting-a-pull-request)
- [Commit Style](#commit-style)

---

## Code of Conduct

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md). By participating you agree to abide by its terms.

## Reporting Bugs

Use the [Bug Report issue template](https://github.com/Dalot/scavenger/issues/new?template=bug_report.md). Include:

- Your OS and architecture
- Scavenger version (`scavenger --version`)
- The agent you're using (Claude Code, Cursor, other)
- Steps to reproduce
- Expected vs actual behaviour
- Output of `scavenger doctor --format=json` if relevant

## Requesting Features

Use the [Feature Request issue template](https://github.com/Dalot/scavenger/issues/new?template=feature_request.md). Describe the problem you want solved, not just the solution.

## Development Setup

**Requirements:**

- Rust 1.85+ (`rustup update stable`)
- A C compiler for tree-sitter grammars (gcc/clang)
- SQLite dev headers (usually bundled via rusqlite's `bundled` feature — no extra install needed)

**Clone and build:**

```bash
git clone https://github.com/Dalot/scavenger.git
cd scavenger
cargo build
```

**Run the full check suite:**

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
cargo deny check
```

Or via Make:

```bash
make check   # fmt + clippy
make test    # cargo test
```

## Making Changes

1. Fork the repo and create a branch from `master`:
   ```bash
   git checkout -b feat/my-feature
   ```
2. Keep changes focused. One logical change per PR.
3. If you're adding a new language, add corresponding test fixtures under `tests/fixtures/sample_project/`.
4. If you're touching the MCP bridge or capsule pipeline, add or update integration tests in `tests/integration/`.
5. Run `cargo fmt` before committing.

## Tests

```bash
cargo test                        # all tests
cargo test --test capsule_test    # single integration test file
cargo test graph                  # filter by name
```

Tests live in two places:
- **`src/`** — unit tests in `#[cfg(test)] mod tests` blocks
- **`tests/`** — integration tests as standalone binaries

All new features and bug fixes should be accompanied by a test.

## Submitting a Pull Request

1. Ensure `cargo fmt --check`, `cargo clippy -- -D warnings`, and `cargo test` all pass locally.
2. Update `CHANGELOG.md` under `[Unreleased]` with a brief description of your change.
3. Open a PR against `master`. Fill in the PR template.
4. A maintainer will review and may request changes.
5. Once approved, the maintainer will merge.

## Commit Style

Use concise, imperative-mood subject lines:

```
feat: add Kotlin language support
fix: correct BM25 score overflow on large corpora
docs: expand configuration reference
refactor: extract capsule scoring into its own module
test: add concurrency stress test for daemon socket
```

No ticket numbers required. Keep the subject under 72 characters.

---

Licensed under MIT OR Apache-2.0. Contributions you submit are understood to be licensed under the same terms unless you explicitly state otherwise.
