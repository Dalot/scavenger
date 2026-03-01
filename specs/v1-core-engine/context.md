# Feature Context

**Feature**: Scavenger v1 Core Engine — AST dependency graph, capsule assembly, session memory, MCP server, daemon lifecycle, CLI, hooks integration
**Mission**: Build the complete v1 Scavenger system that reduces Claude Code's token consumption by serving focused capsules instead of full files and persists session memory anchored to code symbols across sessions. Ship as a single Rust binary with zero runtime dependencies.
**Code Paths**: `src/main.rs` (CLI entry), `src/daemon/` (daemon loop, handlers, coordinator, watcher, socket, federation), `src/bridge/` (MCP stdio↔UDS translation), `src/graph/` (StableGraph, indexer, doc_indexer, traversal, similarity, estimator), `src/capsule/` (6-stage pipeline: gather, score, render), `src/query/` (intent detection, FTS5 search), `src/memory/` (versions, annotations, signals, session, antipattern), `src/hooks/` (pre/post tool use, registration), `src/db/` (schema, migrations, queries)
**Directives**: Constitution principles: Single Binary/Local Only, Terminal-First CLI, Token Efficiency is the Product, Symbol-Anchored Not File-Anchored, Fail Open Never Block. Design document: `docs/plans/2026-02-28-consolidated-design.md`. Component tracker: `docs/architecture-components.md` (84 CLOSED, 2 OPEN/deferred).
**Research**: BM25 parameter validation needed early in implementation — FTS5 defaults (k1=1.2, b=0.75) tuned for natural language, not code. Run empirical tests on real codebase with 20+ queries. If ranking quality is poor, evaluate tantivy for v1 scope. rmcp v0.17.0 confirmed stable Rust compatible (4.3M+ downloads). Hand-rolled JSON-RPC fallback documented in design.
**Gateway**: `scavenger init` — single entry point that creates `.scavenger/`, indexes the project, registers hooks, registers MCP bridge, and starts the daemon.

## Plan Artifacts

| Artifact | Path | Description |
|----------|------|-------------|
| Spec | `specs/v1-core-engine/spec.md` | 18 FRs, 11 NFRs, 8 user stories, 8 success criteria |
| Plan | `specs/v1-core-engine/plan.md` | 11-phase implementation plan with SYNC/ASYNC triage |
| Architecture | `specs/v1-core-engine/architecture.md` | Feature-level architecture: views, ADRs, constraints |
| Data Model | `specs/v1-core-engine/data-model.md` | Full SQLite schema, in-memory structures, entity relationships |
| Research | `specs/v1-core-engine/research.md` | Phase 0 findings: rmcp stable, BM25 validated, grammar coverage |

## Next Step

`/cx-spec.tasks` — Break the plan into implementable task units.
