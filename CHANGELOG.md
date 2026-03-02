# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-02-28

### Fixed
- Kotlin language support removed from claimed supported languages; `tree-sitter-kotlin` 0.3.8 requires tree-sitter <0.23, incompatible with our tree-sitter 0.25 dependency.

### Changed
- Improved capsule scoring and token budget enforcement.
- Incremental re-indexing debounce tuning for large repos.

## [0.1.0] - 2026-02-01

### Added
- Initial release.
- AST dependency graph built with tree-sitter over 15 languages (Rust, Python, TypeScript, TSX, JavaScript, JSX, Go, Java, C#, C, C++, Ruby, Bash, PHP, Swift).
- Per-branch SQLite index (WAL mode) with full-text search via FTS5.
- 6-stage capsule pipeline: gather → score → render → budget → assemble → serve.
- Session memory: annotations anchored to symbols, behavioral signals, version history.
- MCP bridge exposing `get_capsule`, `read_annotations`, `write_annotation`, `delete_annotation`, `search_docs`.
- Claude Code integration: `PreToolUse`/`PostToolUse`/`SessionStart`/`SessionEnd` hooks.
- Cursor integration: `afterFileEdit`/`sessionStart`/`sessionEnd` hooks + MCP.
- `scavenger init`: zero-click setup for Claude Code, Cursor, and generic `.mcp.json`.
- `scavenger observe`: live TUI observability dashboard.
- `scavenger doctor`: health diagnostics with `--format=json`.
- `scavenger federate`: query symbols from linked repositories.
- Multi-platform release builds: Linux x86_64, macOS x86_64, macOS aarch64.
- `telemetry` feature flag for OpenTelemetry/OTLP export.

[Unreleased]: https://github.com/Dalot/scavenger/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/Dalot/scavenger/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Dalot/scavenger/releases/tag/v0.1.0
