# Research: Scavenger v1 Core Engine

**Branch**: `v1-core-engine` | **Date**: 2026-02-28

## Research Summary

The consolidated design document (`docs/plans/2026-02-28-consolidated-design.md`) has 84 CLOSED components with all major decisions already made. Research focused on validating assumptions and resolving the one constitution conflict discovered during clarification.

## Resolved: Rust Toolchain (rmcp nightly requirement)

**Decision**: Use Rust stable toolchain (edition 2024, minimum 1.85+)
**Rationale**: The `rmcp` crate v0.17.0 (released 2026-02-27) works on stable Rust. The design doc's "nightly required" references at §9.1, §14, and §15 are outdated — they were written when `rmcp` was pre-stable. The constitution correctly specifies stable.
**Alternatives considered**: (1) Stay on nightly — rejected, unnecessary constraint and fragile CI. (2) Drop rmcp for hand-rolled JSON-RPC — rejected, rmcp is now stable with 4.3M+ downloads and 542 reverse dependencies.
**Action**: Design doc lines 1023, 1443, 1502 contain outdated "nightly" references that should be updated.

## Validated: FTS5 BM25 Parameters

**Decision**: Use FTS5 built-in BM25 (k1=1.2, b=0.75) for v1, validate empirically
**Rationale**: FTS5 BM25 parameters are tuned for natural language, not code. Code has shorter documents, repetitive vocabulary, and compound identifiers. However, the `heck` crate splitting at index time (camelCase/snake_case → component words) mitigates the compound identifier problem. Empirical validation with 20+ representative queries on a real codebase is planned for Phase 11.
**Alternatives considered**: (1) tantivy — allows custom BM25 parameters and code tokenizers, but adds complexity. Deferred to v2 unless Phase 11 validation reveals quality issues. (2) Custom BM25 parameters — FTS5 hardcodes k1/b, no tuning available.
**Risk**: Medium. Mitigation: early validation, tantivy as escape hatch.

## Validated: tree-sitter Grammar Coverage

**Decision**: Use crates.io grammar crates for all 15 languages
**Rationale**: All 15 target languages have published tree-sitter grammar crates on crates.io. The `tags.scm` query files provide standardized capture conventions. Grammar quality varies by language — TypeScript, Python, Rust, Go have mature grammars; PHP, Swift, Kotlin are newer.
**Alternatives considered**: WASM-based dynamic grammar loading — deferred to v2 for extensibility.
**Risk**: Medium-low. Some languages may have incomplete `tags.scm` coverage. Mitigation: test each language early in Phase 3, accept partial extraction for v1.

## Validated: rmcp SDK Integration

**Decision**: Use rmcp v0.17.0 with `#[tool]` proc macros and `schemars` JSON Schema generation
**Rationale**: rmcp is the official Rust MCP SDK from the Model Context Protocol organization. v0.17.0 provides stable API, tokio-based async, multiple transport options (stdio, HTTP, streaming), and proc macros for tool declarations. 4.3M+ total downloads, 542 reverse dependencies — production-ready.
**Alternatives considered**: Hand-rolled JSON-RPC over stdio — documented as fallback in design doc §9.1, only needed if rmcp introduces breaking API changes.
**Risk**: Low. Active development, large user base.

## Validated: SQLite WAL Concurrency Model

**Decision**: SQLite WAL mode with 1 writer + N readers via tokio-rusqlite
**Rationale**: WAL mode allows concurrent reads during writes. One writer connection (daemon indexer), N reader connections (UDS request handlers). Manual WAL checkpoint during idle (5s after last query) prevents unbounded WAL growth. `parking_lot::RwLock` for in-memory graph concurrency with split-phase locking.
**Alternatives considered**: (1) Multiple SQLite databases per subsystem — rejected, increases complexity without benefit. (2) In-memory-only graph without SQLite — rejected, need persistence across daemon restarts and branch switches.
**Risk**: Low. Well-understood pattern, SQLite WAL is battle-tested.

## No Further Research Needed

All remaining design decisions are closed in the consolidated design document. No NEEDS CLARIFICATION items remain in the spec after the clarification session. The five clarification items (config management, daemon logging, schema versioning, capsule format, anti-pattern thresholds) were resolved and added as FR-016 through FR-018, NFR-011, and expanded FR-008.
