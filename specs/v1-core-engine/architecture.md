# Architecture Description: Scavenger v1

**Version**: 1.0.0 | **Created**: 2026-02-28 | **Last Updated**: 2026-02-28
**Architect**: AI | **Status**: Draft

---

## 1. Introduction

### 1.1 Purpose

Scavenger reduces Claude Code's token consumption on growing codebases by serving focused "capsules" (AST-aware context snippets) instead of full files, and persists session memory anchored to code symbols across sessions. It solves two problems: (1) token waste from reading entire files for a single function, and (2) session amnesia where observations and decisions are lost between sessions.

### 1.2 Scope

**In Scope:**

- AST-based code indexing for 15 programming languages via tree-sitter
- Directed dependency graph with 7 edge types
- 6-stage capsule assembly pipeline with token budget enforcement
- Three-layer session memory (version history, annotations, behavioral signals)
- Persistent background daemon with file watching and branch awareness
- MCP server integration with Claude Code via stdio bridge
- PreToolUse/PostToolUse hooks for automatic capsule injection and re-indexing
- CLI for all operations
- Cross-repo federation (read-only)
- Token savings analytics

**Out of Scope:**

- VS Code / IDE extension (v2)
- LLM-based summarization or embeddings
- Cloud sync of memory across machines
- HTTP dashboard for analytics (v2)
- Cross-repo edges in federation (v2)
- Support for non-Claude Code agents

### 1.3 Definitions and Acronyms

| Term | Definition |
|------|------------|
| Capsule | A token-budgeted, ranked assembly of context items (symbols, annotations, signals, docs) for a query target |
| NodeId | `hash(file_path, symbol_name, signature)` — stable identity for a code symbol |
| Skeleton | Pre-rendered `signature + docstring` stored at index time for fast capsule emission |
| Pin | A capsule item guaranteed inclusion regardless of score (target node, active signals, 1-hop structural) |
| MCP | Model Context Protocol — the standard for LLM tool integration |
| UDS | Unix Domain Socket — IPC mechanism between daemon, bridge, and hooks |
| FTS5 | SQLite full-text search extension with BM25 ranking |

---

## 2. Stakeholders and Concerns

| Stakeholder | Role | Key Concerns | Priority |
|-------------|------|--------------|----------|
| Developer (Claude Code user) | End user | Token savings, session continuity, zero-friction setup, never blocks Claude | Critical |
| Claude Code (AI agent) | Consumer | Capsule relevance, tool discoverability, annotation persistence | High |
| Open-source contributors | Developers | Code clarity, modular architecture, testability | High |
| Scavenger maintainers | Operations | Binary size, upgrade path, crash recovery, diagnostics | High |

---

## 3. Architectural Views

### 3.1 Context View

#### 3.1.1 System Scope

Scavenger runs as a local background daemon alongside Claude Code. It indexes the project's source code and documentation into a dependency graph, and intercepts Claude Code's file operations to inject relevant context. All data stays local.

#### 3.1.2 External Entities

| Entity | Type | Interaction | Data Exchanged | Protocol |
|--------|------|-------------|----------------|----------|
| Claude Code | AI Agent | MCP tools + hooks | Capsules, annotations, search results | stdio JSON-RPC (MCP bridge) |
| Git | VCS | Branch detection, diff | Branch name, changed files | CLI subprocess |
| Filesystem | Storage | File watching, indexing | Source files, .scavenger/ data | notify + direct I/O |
| Federated repos | Sibling projects | Read-only query fan-out | FTS5 search results | SQLite read-only connections |

#### 3.1.3 Context Diagram

```
                        ┌─────────────────────────────────────────────┐
                        │                Claude Code                   │
                        │                                              │
                        │  ┌──────────────┐     ┌───────────────────┐  │
                        │  │ MCP Bridge    │     │ Hooks (pre/post)  │  │
                        │  │ (stdio↔UDS)   │     │ (Read→capsule,    │  │
                        │  │               │     │  Write→re-index)  │  │
                        │  └───────┬───────┘     └────────┬──────────┘  │
                        └──────────┼──────────────────────┼─────────────┘
                                   │ UDS                  │ UDS
                                   ▼                      ▼
                        ┌─────────────────────────────────────────────┐
                        │             scavenger daemon                 │
                        │         (single background process)          │
                        │                                              │
                        │  ┌───────────────────────────────┐  ┌─────┐ │
                        │  │ UDS Listener (daemon.sock)    │  │File │ │
                        │  │ handles MCP + hook requests   │  │Watch│ │
                        │  └──────────────┬────────────────┘  └──┬──┘ │
                        │                 │                      │    │
                        │  ┌──────────────▼──────────────────────▼──┐ │
                        │  │       ReindexCoordinator               │ │
                        │  │  (branch detect, DB swap, cold start)  │ │
                        │  └──────────────┬─────────────────────────┘ │
                        │                 │                           │
                        │  ┌──────────────▼─────────────────────────┐ │
                        │  │    Core Graph (petgraph::StableGraph)   │ │
                        │  │    + reverse index (in-memory HashMap)  │ │
                        │  │    Arc<parking_lot::RwLock<GraphState>> │ │
                        │  └──────────────┬─────────────────────────┘ │
                        │                 │                           │
                        │  ┌──────────────▼─────────────────────────┐ │
                        │  │ Per-branch SQLite DB (WAL, FTS5, 1W/NR)│ │
                        │  │ + daemon_meta.db (shared)               │ │
                        │  └────────────────────────────────────────┘ │
                        │                                              │
                        │  ┌────────────────────────────────────────┐  │
                        │  │ Federation (read-only to other repos)   │  │
                        │  └────────────────────────────────────────┘  │
                        └─────────────────────────────────────────────┘
```

#### 3.1.4 External Dependencies

| Dependency | Purpose | Fallback |
|------------|---------|----------|
| Claude Code v2.1.9+ | additionalContext in hooks, session_id in payloads | Hooks degrade to empty responses |
| Git | Branch detection, diff for cold start | Treat as single-branch, full re-index |
| tree-sitter grammars | AST parsing for 15 languages | Skip unsupported languages silently |

---

### 3.2 Functional View

#### 3.2.1 Functional Elements

| Element | Responsibility | Interfaces | Dependencies |
|---------|----------------|------------|--------------|
| Graph (src/graph/) | AST parsing, graph building, PageRank, similarity | NodeId lookups, traversals, FTS5 search | db, tree-sitter |
| Capsule (src/capsule/) | 6-stage assembly pipeline | get_capsule() -> CapsuleResult | graph, query, memory |
| Query (src/query/) | Intent detection, traversal strategy, BM25 search | query(target, query_text) -> ranked nodes | graph |
| Memory (src/memory/) | Annotation CRUD, version history, signals, anti-patterns | read/write/delete annotations, detect patterns | db, graph |
| Daemon (src/daemon/) | Process lifecycle, UDS listener, file watcher, branch handling | start(), shutdown(), handle_request() | graph, capsule, memory, db |
| Bridge (src/bridge/) | MCP stdio-to-UDS translation | 5 MCP tools via rmcp | daemon (UDS) |
| Hooks (src/hooks/) | Claude Code hook handlers + registration | pre_tool_use(), post_tool_use() | daemon (UDS) |
| DB (src/db/) | SQLite connections, schema, migrations | open(), query(), migrate() | rusqlite |
| Config (src/config.rs) | .scavenger.toml loading and validation | load() -> Config | toml |
| CLI (src/main.rs) | Command dispatch | 15+ subcommands | all modules |

#### 3.2.2 Data Flow: Capsule Request

```
Claude Code Read tool
    │
    ▼
PreToolUse hook (src/hooks/)
    │ stdin JSON {session_id, tool_name, tool_input}
    ▼
UDS to daemon (src/daemon/socket.rs)
    │
    ▼
Request handler (src/daemon/handlers.rs)
    │ extract file_path, resolve target node
    ▼
Query engine (src/query/)
    │ detect intent, select traversal strategy
    ▼
Graph traversal (src/graph/traversal.rs)
    │ BFS/DFS with degree cap, node budget
    ▼
Capsule assembly (src/capsule/)
    │ GATHER → SCORE → PIN → TRIM → GROUP → RENDER
    ▼
Formatted capsule text
    │
    ▼
UDS response → hook stdout {"additionalContext": "..."}
    │
    ▼
Claude Code receives capsule before file content
```

---

### 3.3 Information View

#### 3.3.1 Data Entities

| Entity | Storage | Owner | Lifecycle | Access |
|--------|---------|-------|-----------|--------|
| Node | Per-branch SQLite + in-memory graph | Graph module | Created on index, updated on re-index, deleted on file removal | Read-heavy |
| Edge | Per-branch SQLite + in-memory graph | Graph module | Rebuilt on re-index of source file | Read-heavy |
| Annotation | Per-branch SQLite | Memory module | LLM creates/updates/deletes, staleness auto-detected, 30-day orphan cleanup | Read-write |
| Behavioral Signal | Per-branch SQLite | Memory module | Auto-detected, TTL pruned (48h/2 sessions) | Write-once, read |
| Session Log | Per-branch SQLite | Memory module | Append-only per session, TTL pruned | Append, read |
| Token Log | daemon_meta.db | Estimator | Append after each tool call | Append, aggregate |
| Doc Chunk | Per-branch SQLite | Doc indexer | Re-chunked on file change | Read-heavy |

#### 3.3.2 Data Quality and Integrity

- **Consistency**: SQLite ACID within each DB. Graph is derived from SQLite — SQLite is source of truth. On crash, graph rebuilds from SQLite.
- **Validation**: Schema migration with version checks. Downgrade guard prevents incompatible access.
- **Retention**: Annotations persist indefinitely (LLM-managed). Signals/session log TTL-pruned. Token log retained per config (default 30 days).
- **Backup**: `.scavenger/` is a rebuildable cache — delete and re-init to recover from any corruption.

---

### 3.4 Concurrency View

#### 3.4.1 Process Structure

| Process | Purpose | Scaling | State |
|---------|---------|---------|-------|
| scavenger daemon | Background indexer + query server | Single instance per project (flock) | Stateful (graph, DBs) |
| scavenger mcp-bridge | Per-session MCP shim | One per Claude Code session | Stateless (proxies to daemon) |
| scavenger hook | Per-tool-call hook handler | One per hook invocation | Stateless (proxies to daemon) |

#### 3.4.2 Thread Model

- **Tokio async runtime**: daemon main loop, UDS listener, request handlers
- **Rayon thread pool**: initial bulk indexing only (Parser is not Send — thread-local instances)
- **parking_lot::RwLock**: split-phase concurrency on GraphState
  - Phase 1 (no lock): re-parse + build new structures
  - Phase 2 (write lock, ~5-15ms): commit SQLite + update graph
  - Phase 3 (read lock): deferred PageRank, once per batch
- **tokio-rusqlite**: 1 writer connection + N reader connections

#### 3.4.3 Coordination

- **Graph lock ordering**: always SQLite-before-graph in Phase 2 — if SQLite fails, bail before touching graph
- **File watcher debounce**: 300ms trailing-edge prevents re-index storms
- **VCS deferral**: pause on `.git/index.lock` presence — prevents indexing during git operations
- **WAL checkpoint**: manual `PRAGMA wal_checkpoint(PASSIVE)` during idle (5s after last query)

---

### 3.5 Development View

#### 3.5.1 Module Dependency Rules

```
CLI (main.rs) ──> all modules

hooks/ ──> daemon (UDS client only)
bridge/ ──> daemon (UDS client only)

daemon/ ──> graph/, capsule/, memory/, query/, db/, config
capsule/ ──> graph/, query/, memory/
query/ ──> graph/, db/
memory/ ──> db/, graph/
graph/ ──> db/
db/ ──> (external: rusqlite, tokio-rusqlite)
config ──> (external: toml, serde)
```

No circular dependencies. hooks/ and bridge/ are thin UDS clients — they do not import daemon internals.

#### 3.5.2 Build and Testing

- **Build**: `cargo build --release` (single binary)
- **Lint**: `cargo clippy`, `cargo fmt --check`
- **Unit tests**: `cargo test` — per-module tests
- **Integration tests**: `tests/integration/` — end-to-end with fixture projects
- **Fixtures**: `tests/fixtures/` — sample multi-language projects for indexing tests

---

### 3.6 Deployment View

Scavenger is a local developer tool, not a server deployment.

- **Installation**: Single binary, no runtime dependencies. Copy to PATH or install via cargo.
- **Runtime**: `.scavenger/` directory per project (mode 0700). One daemon process per project.
- **Upgrade**: Replace binary. Schema migration runs automatically on next startup.
- **Multi-project**: Independent `.scavenger/` per project. Federation links are the only cross-project connection.
- **Git worktrees**: Each worktree gets its own `.scavenger/` — completely independent.

---

### 3.7 Operational View

#### 3.7.1 Monitoring and Diagnostics

- **Daemon log**: `.scavenger/daemon.log` — structured events (startup, shutdown, re-index, errors), rotated 10MB/2 files
- **Hook errors**: `.scavenger/hook-errors.log` — hook failure log
- **Doctor**: `scavenger doctor` — 5 categories (Process, FileIntegrity, Config, Dependencies, Resources), exit 0/1/2
- **Stats**: `scavenger stats` — token savings per session and all-time

#### 3.7.2 Recovery

- **Crash**: SQLite WAL guarantees DB consistency. Dirty-flag triggers freshness scan on restart. Graph rebuilds from SQLite.
- **Corruption**: Delete `.scavenger/` and re-run `scavenger init` — the index is a rebuildable cache. Annotations are the only non-recoverable data.
- **Disk full**: `SQLITE_FULL` → degraded read-only mode. `scavenger doctor` detects.

---

## 4. Architectural Perspectives

### 4.1 Security

- **No network**: All data local. No cloud calls, no external APIs, no telemetry.
- **File permissions**: `.scavenger/` created with mode 0700 (owner-only).
- **File locking**: `fs2::FileExt::lock_exclusive()` for settings.json writes.
- **Input validation**: all config values range-checked and clamped. SQL parameterized queries only.

### 4.2 Performance

| Metric | Target | Mechanism |
|--------|--------|-----------|
| Hook latency (p95) | <50ms | Binary startup 0.5ms + UDS 0.1ms + capsule 10-30ms |
| Hook fallback | 100ms | Partial capsule (pinned only) |
| Initial index (5k files) | <5s | Rayon parallel parsing |
| Incremental re-index | <50ms | Single file re-parse <1ms + split-phase swap ~15ms |
| Branch switch (warm) | <500ms | DB swap + graph reload + freshness check |
| PageRank (100k nodes) | ~30-60ms | 30 iterations, once per batch |

---

## 5. Global Constraints and Principles

From the project constitution (`.cx-spec/memory/constitution.md`):

1. **Single Binary, Local Only** — no runtime deps, no network
2. **Terminal-First CLI** — all features via CLI, JSON + human output
3. **Token Efficiency is the Product** — every feature must reduce tokens or improve relevance
4. **Symbol-Anchored, Not File-Anchored** — identity through NodeId, not file paths
5. **Fail Open, Never Block** — hooks exit 0 on error, degrade gracefully

---

## 6. Architecture Decision Records

| ID | Decision | Status | Date |
|----|----------|--------|------|
| ADR-001 | Single daemon with UDS, not in-process MCP server | Accepted | 2026-02-28 |
| ADR-002 | Per-branch SQLite databases, not single DB | Accepted | 2026-02-28 |
| ADR-003 | FTS5 BM25 for v1, tantivy deferred to v2 | Accepted | 2026-02-28 |
| ADR-004 | Split-phase graph locking, not lock-free | Accepted | 2026-02-28 |
| ADR-005 | rmcp on stable Rust, not hand-rolled MCP | Accepted | 2026-02-28 |

### ADR-001: Single Daemon with UDS

**Status**: Accepted
**Context**: Claude Code can run multiple sessions per project. Each session needs access to the index. Options: (A) daemon per session, (B) single daemon with UDS, (C) in-process library.
**Decision**: Single daemon with UDS listener. Each Claude Code session spawns a thin MCP bridge shim that translates stdio to UDS.
**Consequences**: Positive — single index, no duplication, multiple sessions share state. Negative — extra IPC hop. Risks — UDS availability on all target platforms (Linux/macOS only, acceptable for v1).

### ADR-002: Per-Branch SQLite Databases

**Status**: Accepted
**Context**: Branch switches change file contents. Options: (A) single DB with branch column, (B) per-branch DB files.
**Decision**: Per-branch SQLite DB under `.scavenger/indexes/`. Each branch is self-contained. Warm switch = DB swap. Cold start = copy parent + re-index diff.
**Consequences**: Positive — clean isolation, simple branch cleanup, no cross-branch query contamination. Negative — disk usage (one DB per branch), cold start cost. Risks — many branches could consume significant disk. Mitigation — cleanup on branch deletion.

### ADR-003: FTS5 BM25 for v1

**Status**: Accepted
**Context**: Need ranked text search over code symbols. Options: (A) FTS5 built-in BM25, (B) tantivy custom index, (C) trigram-based search.
**Decision**: FTS5 with BM25 (k1=1.2, b=0.75) and `heck` crate identifier splitting at index time. Composition: `0.6*bm25 + 0.4*centrality`.
**Consequences**: Positive — zero external dependencies, built into SQLite, simple. Negative — BM25 parameters not tunable, may underperform on code. Risks — ranking quality. Mitigation — empirical validation, tantivy as v2 upgrade path.

### ADR-004: Split-Phase Graph Locking

**Status**: Accepted
**Context**: Capsule reads and index writes are concurrent. Options: (A) single mutex, (B) RwLock with split-phase, (C) lock-free MVCC.
**Decision**: `parking_lot::RwLock` with 3-phase protocol: prep (no lock) -> swap (brief write lock ~5-15ms) -> deferred PageRank. SQLite committed before graph update — if SQLite fails, graph stays consistent.
**Consequences**: Positive — minimal read blocking, correct crash recovery invariant. Negative — slightly stale centrality between Phase 2 and 3. Risks — lock contention during bursty re-indexing. Mitigation — accepted for v1.

### ADR-005: rmcp on Stable Rust

**Status**: Accepted
**Context**: Need MCP SDK for tool declarations. Options: (A) rmcp crate, (B) hand-rolled JSON-RPC.
**Decision**: rmcp v0.17.0 on stable Rust (edition 2024). Provides `#[tool]` proc macros, schemars JSON Schema generation, tokio async, multiple transports.
**Consequences**: Positive — official SDK, stable API, large user base (4.3M downloads). Negative — dependency on external crate. Risks — API breaking changes. Mitigation — hand-rolled fallback documented.
