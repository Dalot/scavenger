# Implementation Plan: Scavenger v1 Core Engine

**Branch**: `v1-core-engine` | **Date**: 2026-02-28 | **Spec**: [spec.md](specs/v1-core-engine/spec.md)
**Input**: Feature specification from `specs/v1-core-engine/spec.md`
**Design Doc**: `docs/plans/2026-02-28-consolidated-design.md` (authoritative for all implementation-level detail)

## Summary

Build the complete Scavenger v1 system: a persistent Rust daemon that indexes codebases via tree-sitter, serves focused "capsules" to Claude Code via MCP + hooks, and persists session memory anchored to AST symbols. Ships as a single binary. 18 FRs, 11 NFRs, 84 CLOSED design components across 11 build phases.

## Technical Context

**Language/Version**: Rust stable, edition 2024, minimum 1.85+
**Primary Dependencies**: rmcp v0.17.0 (MCP SDK), tree-sitter (15 grammars), petgraph, rusqlite/tokio-rusqlite, tokio, notify-debouncer-full, clap, serde, parking_lot, strsim, heck, rayon, fs2, owo-colors, md5, toml, schemars, ignore
**Storage**: SQLite per branch (WAL, FTS5) + shared daemon_meta.db
**Testing**: `cargo test` (unit + integration)
**Target Platform**: Linux/macOS (Unix domain sockets)
**Project Type**: Single Rust binary crate
**Performance Goals**: <50ms hook latency, <5s initial index (5k files), <50ms incremental re-index
**Constraints**: No network egress, no runtime deps, single binary <50MB
**Scale/Scope**: 5,000-file codebases, 100k nodes / 500k edges upper bound

## Constitution Check

*GATE: All five principles verified — no violations.*

- **I. Single Binary, Local Only**: NFR-002 — no cloud, no external APIs, no network egress
- **II. Terminal-First CLI**: FR-013 + NFR-009 — all commands via CLI, JSON + human-readable output
- **III. Token Efficiency is the Product**: FR-003, FR-015, FR-018, SC-001 — capsule pipeline, token logging, savings report
- **IV. Symbol-Anchored, Not File-Anchored**: NodeId = hash(file_path, symbol_name, signature), FR-010 identity migration
- **V. Fail Open, Never Block**: NFR-001, NFR-006 — hooks exit 0, <50ms budget, partial fallback at 100ms

## Build Dependency Graph

```
Phase 1: Scaffolding & DB ──┬──> Phase 2: Graph Core ──┬──> Phase 3: Indexing ──┬──> Phase 7: Daemon
                             │                          │                        │
                             │                          └──> Phase 4: Query ─────┤
                             │                                                   │
                             └──> Phase 5: Memory ──> Phase 6: Capsule ──────────┘
                                                                                 │
                                                           Phase 7 ──> Phase 8: MCP/Hooks ──> Phase 9: CLI
                                                                                                    │
                                                                                    Phase 10: Federation/Analytics
                                                                                                    │
                                                                                    Phase 11: Integration & Polish
```

## Phase 1: Project Scaffolding & Database Foundation

**Goal**: Bootable Rust project with SQLite schema, config loading, and module structure.
**Estimated effort**: Small — well-defined, mostly boilerplate.

**Source files to create**:

```
Cargo.toml                    # 22 crate dependencies (design doc §15)
rust-toolchain.toml           # pin stable Rust version
.scavenger.toml.example       # reference config (design doc §13)
src/main.rs                   # clap entry, subcommand dispatch skeleton
src/config.rs                 # .scavenger.toml loading, validation, range clamping (FR-016)
src/db/mod.rs                 # SQLite connection management (per-branch + daemon_meta)
src/db/schema.rs              # all CREATE TABLE, FTS5, triggers, indexes (design doc §3.5-3.6)
src/db/queries.rs             # typed query helpers (initial stubs)
```

**Key implementation details**:

- All SQL schemas are defined verbatim in design doc §3.5-3.6 — transcribe exactly
- Schema migration: `PRAGMA user_version = 1` on creation, sequential migration functions per version step, downgrade guard refusing incompatible DBs (FR-017)
- SQLite PRAGMAs at connection open: `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5000`, `cache_size=-64000`, `mmap_size=268435456`, `auto_vacuum=INCREMENTAL`
- Config validation: all numeric fields validated at load, out-of-range values clamped to nearest bound with logged warning
- Module tree: declare all `mod` stubs so `cargo build` succeeds with empty implementations

**Deliverable**: `cargo build` succeeds, `cargo test` runs schema creation/migration tests, config loading tests.

---

## Phase 2: Graph Core

**Goal**: In-memory StableGraph with node/edge types, reverse index, PageRank, SQLite persistence.
**Estimated effort**: Medium — data structures are well-defined, PageRank is provided by petgraph.

**Source files**:

```
src/graph/mod.rs              # GraphState struct, Arc<RwLock<>> wrapper, load/save
src/graph/types.rs            # NodeWeight, EdgeWeight, NodeId, enums
```

**Key implementation details**:

- `GraphState` wraps `petgraph::StableGraph<NodeWeight, EdgeWeight, Directed>` + reverse index `HashMap<NodeId, Vec<PathBuf>>` in `Arc<parking_lot::RwLock<GraphState>>`
- 9 node types: Function, Method, Class, Interface, Type, Enum, ExportedVar, Module, File
- 7 edge types: Imports, Calls, TypeRef, Extends, Implements, Exports, Contains
- Edge confidence: precise, heuristic, speculative
- `NodeId = hash(file_path, symbol_name, signature)` — full signature hash for overload disambiguation
- Reverse index built from: `SELECT e.to_id, n.file_path FROM edges e JOIN nodes n ON e.from_id = n.id`
- PageRank: `petgraph::algo::page_rank(&g, 0.85, 30)` — run once per debounce batch (Phase 3 of split-phase)
- Centrality persisted to `nodes.centrality` during idle checkpoint (5s after last query), primary source is in-memory

**Deliverable**: Graph loads from SQLite, add/remove nodes/edges, PageRank computes, reverse index lookups work.

---

## Phase 3: Tree-sitter Indexing Pipeline

**Goal**: Parse 15 languages, extract symbols, build graph, incremental re-index.
**Estimated effort**: Large — 15 language mappings, complex re-index flow, concurrency model.

**Source files**:

```
src/graph/index.rs            # tree-sitter extraction, skeleton gen, sig_hash
src/graph/doc_indexer.rs      # markdown chunking, doc_chunks table
src/graph/similarity.rs       # identity migration heuristic
```

**Key implementation details**:

*Symbol extraction (design doc §4.1)*:
- Use `tags.scm` query files with standard captures (`@definition.function`, `@definition.class`, `@name`, `@doc`)
- Grammar deps via crates.io (15 crates: tree-sitter-{rust,python,typescript,javascript,go,java,c-sharp,c,cpp,ruby,bash,kotlin,php,swift} + tsx/jsx via typescript)
- Signature = source text from definition start to body start (exclude body child via `Node::child_by_field_name("body")`)
- Docstring extraction: per-language rules (Rust `///`, Python first `expression_statement(string)`, Java/C#/PHP/Kotlin/Swift `/** */`, TS/Go/C/C++ preceding `(comment)`, Ruby `#`)
- Skeleton: `signature + docstring` stored in `skeleton` column (~300 bytes avg, ~30MB at 100k symbols)
- `signature_hash`: first 8 hex chars of MD5 over whitespace-normalized signature
- FTS5 token splitting: `heck` crate splits camelCase/snake_case at index time (`getUserById` → `getUserById get User By Id`)
- Parallelism: `Parser` is NOT `Send` — use rayon `par_iter()` with thread-local `Parser` instances

*15-language node type mapping* (design doc §4.2):

| Language | Key tree-sitter node types |
|----------|---------------------------|
| TypeScript/JavaScript | `function_declaration`, `method_definition`, `class_declaration`, `interface_declaration`, `type_alias_declaration`, `enum_declaration` |
| TSX/JSX | Same as TS + JSX element handling |
| Python | `function_definition`, `class_definition`, `decorated_definition` |
| Go | `function_declaration`, `method_declaration`, `type_spec(struct_type)`, `type_spec(interface_type)` |
| Rust | `function_item`, `struct_item`, `enum_item`, `trait_item`, `impl_item`, `type_item`, `mod_item` |
| Java | `method_declaration`, `class_declaration`, `interface_declaration`, `enum_declaration` |
| C# | `method_declaration`, `class_declaration`, `interface_declaration`, `enum_declaration`, `struct_declaration` |
| C | `function_definition`, `struct_specifier`, `enum_specifier`, `type_definition` |
| C++ | Extends C + `class_specifier`, namespace-qualified `function_definition`, `template_declaration` |
| Ruby | `method`, `class`, `module`, `singleton_method` |
| Bash | `function_definition` |
| Kotlin | `function_declaration`, `class_declaration`, `object_declaration`, `interface_declaration` |
| PHP | `function_definition`, `method_declaration`, `class_declaration`, `interface_declaration`, `trait_declaration` |
| Swift | `function_declaration`, `class_declaration`, `protocol_declaration`, `struct_declaration`, `enum_declaration` |

*13-step re-indexing flow* (design doc §4.4):

1. Collect existing NodeIds for changed file
2. Re-parse entire file with tree-sitter (<1ms for 1000-line file)
3. Build new nodes and edges from parse
4. Compute `signature_hash` for each new node
5. Compare old vs new NodeId sets (unchanged / orphaned / new)
6. Run similarity heuristic on orphans vs candidates
7. Migrate annotations for matched pairs
8. Generate pre-computed skeleton for each new/updated node
9. Rebuild edges: delete all from changed file's nodes, rebuild from new parse
10. Update in-memory reverse index for affected edges
11. Queue affected cross-file nodes for async re-index (cascade)
12. Recalculate PageRank (deferred Phase 3 — eventually consistent)
13. Update `files` table with new `raw_token_estimate` and `last_indexed`

*Split-phase concurrency* (design doc §3.7):

- **Phase 1 — Prep (no lock)**: re-parse, build new structures, run similarity. Pure local computation.
- **Phase 2 — Swap (write lock, ~5-15ms)**: commit SQLite tx (DELETE old, INSERT new, FTS5 triggers), update graph, update reverse index. SQLite before graph — if SQLite fails, bail before touching graph.
- **Phase 3 — Deferred PageRank (read lock)**: once per batch, not per file. Capsule queries between Phase 2 and 3 see correct structure with stale centrality.
- **Initial index bypass**: during `scavenger init`, no split-phase — bulk insert, build graph once at end.

*Identity migration / similarity heuristic* (design doc §4.7):

```
score = name_similarity × 0.3       (Jaro-Winkler via strsim)
      + signature_similarity × 0.25  (param count, names, return type)
      + body_similarity × 0.25       (hash or called-symbol set)
      + edge_neighborhood × 0.15     (Jaccard on connected NodeIds)
      + file_proximity × 0.05        (same file > same dir > elsewhere)

Threshold: > 0.6 → match (migrate annotations + versions)
```

Pending orphans buffer: unmatched orphans held for one extra debounce cycle before archiving.

*Document indexing* (design doc §4.3): split `.md`/`.markdown` at heading boundaries, sub-split at 100 lines, incremental via `content_hash` (MD5[0..8]).

*Cross-file edge staleness* (design doc §4.6): reverse index lookup → queue affected source files → lazy resolution if capsule hits dangling edge before queue processed. WAL checkpoint during idle.

*Cross-language edges* (design doc §4.8): import path analysis (highest confidence), name matching with case transform (heuristic), FFI detection (speculative). Phantom nodes for unresolved callees.

**Deliverable**: Index a real multi-language codebase, verify node/edge counts, verify incremental re-index, verify identity migration on rename.

---

## Phase 4: Query Engine

**Goal**: Intent detection, traversal strategies, FTS5 BM25 search.
**Estimated effort**: Medium — algorithms defined, FTS5 is provided by SQLite.

**Source files**:

```
src/query/mod.rs              # query engine entry, strategy dispatch
src/query/intent.rs           # intent detection + strategy selection
src/query/search.rs           # FTS5 BM25 + centrality ranking
```

**Key implementation details**:

*Intent detection* (design doc §5.1): hybrid classifier — keyword priority → fuzzy match (strsim) → BM25 fallback → default Understand.

| Intent | Keywords |
|--------|----------|
| Debug | error, bug, fix, crash, failing, broken, traceback, panic, why is |
| Refactor | refactor, clean up, simplify, extract, rename, restructure, move, split |
| Understand | explain, what does, how does, walk me through, overview, describe, where is |
| Extend | add, implement, create, new feature, integrate, build |
| Review | review, check, audit, inspect, validate |

Multi-intent: top-2 within 0.1 → 60/40 weighted union of traversal strategies.

*Traversal mapping* (design doc §5.2):

| Intent | Traversal | Hop Limit | Direction |
|--------|-----------|-----------|-----------|
| Debug | Reverse BFS (callers) | 3 up, 2 down | Primarily incoming |
| Refactor | Forward DFS (blast radius) | Transitive, cap 100 | Outgoing |
| Understand | Bidirectional BFS | 2 each | Both |
| Extend | BFS sibling/implements | 1-2 | Lateral |
| Review | Bidirectional BFS | 2 all | Both |

Explosion mitigation: degree cap >50 (skip utility functions), node budget 100, builtins blocklist from config.

*FTS5 BM25* (design doc §5.3):

```sql
SELECT n.id, n.name, n.signature, bm25(nodes_fts) AS bm25_score
FROM nodes_fts JOIN nodes n ON nodes_fts.rowid = n.rowid
WHERE nodes_fts MATCH ? ORDER BY bm25(nodes_fts) LIMIT 50;
```

Normalization: sign-flip (`-bm25 / max_magnitude`). Composition: `0.6 × normalize(bm25) + 0.4 × normalize(centrality)`. Centrality from in-memory graph, not SQLite.

*Scope tags* (design doc §5.4): path-prefix primary, lazy virtual scope nodes materialized on annotation.

**Deliverable**: Query "validateToken" with intent "debug" returns correct caller/callee subgraph. BM25 search returns ranked results.

---

## Phase 5: Memory Architecture

**Goal**: Three-layer memory model, anti-pattern detection, annotation CRUD.
**Estimated effort**: Large — 7 anti-pattern detectors, annotation lifecycle, fork/merge.

**Source files**:

```
src/memory/mod.rs             # three-layer orchestration, fork/merge
src/memory/versions.rs        # Layer 1: node version history
src/memory/annotations.rs     # Layer 2: annotation CRUD + staleness
src/memory/signals.rs         # Layer 3: behavioral signals + TTL
src/memory/session.rs         # Layer 3: session activity log
src/memory/antipattern.rs     # 7 anti-pattern detectors
```

**Three-layer model** (design doc §7):

| Layer | Storage | Lifecycle |
|-------|---------|-----------|
| 1. Node Version History | `node_versions`, last 5 snapshots | Auto-managed, ordinal decay |
| 2. Semantic Annotations | `annotations`, flexible anchoring | LLM creates/updates/deletes via MCP |
| 3. Behavioral Signals + Session Log | `behavioral_signals` + `session_log` | TTL: 48h or 2 sessions |

*Annotation staleness by anchor type*: Node → checksum change, File → file modified, Scope/None → manual.
*Orphan cleanup*: node-anchored annotations where NodeId gone + stale >30 days → deleted.
*Fork*: copy annotations from parent on cold start. *Merge*: union on merge commit (same anchor+text = dedup, different text = keep both).

*7 anti-pattern detectors* (spec FR-008, design doc §7.5): all use fire-once-at-N dedup `HashSet<(SignalType, key)>`:

- **THRASHING**: ring buffer of `(NodeIndex, timestamp, content_hash)`, Levenshtein >0.9 for ≥3 edits in 5min (key: `node_id`)
- **DEAD_END**: `graph.neighbors_directed(node, Incoming).count()` = 0 from non-test after ≥10 actions or 15min (key: `node_id`)
- **CYCLE_INTRODUCED**: `has_path_connecting(&graph, v, u)` before adding (u,v) (key: `from::to`)
- **LARGE_BLAST_RADIUS**: forward BFS >20 direct OR >50 transitive (key: `node_id`)
- **UNTESTED**: zero test-file edges (key: `node_id`)
- **INDEX_BLIND_SPOT**: file on disk, zero nodes local + federated (key: `file_path`)
- **FAILED_SEARCH**: same query → 0 results ≥3 times (key: `normalized_query`)

**Deliverable**: Annotation CRUD works, staleness flags on checksum change, all 7 detectors fire on synthetic scenarios, version history captures changes.

---

## Phase 6: Capsule Assembly

**Goal**: Full 6-stage pipeline producing formatted capsule output within token budget.
**Estimated effort**: Large — 6 scoring formulas, pinning logic, budget algorithm, output format.

**Source files**:

```
src/capsule/mod.rs            # pipeline orchestrator
src/capsule/gather.rs         # GATHER: collect from all sources
src/capsule/score.rs          # SCORE: 6 per-source formulas
src/capsule/render.rs         # GROUP + RENDER: output formatting
src/graph/estimator.rs        # "without index" token estimator
```

**6-stage pipeline** (design doc §6.3):

1. **GATHER** — collect candidate items from all sources in parallel (nodes via traversal, annotations via FTS5, doc_chunks, node_versions, session_log, behavioral_signals, priority docs)
2. **SCORE** — apply per-source formula → [0.0, 1.0]
3. **PIN** — extract pinned items: target node, active behavioral signals, 1-hop structural (semi-pinned)
4. **TRIM** — sort unpinned by score DESC, greedy fill remaining budget
5. **GROUP** — assign survivors to output groups (Pinned, Callers, Callees, Context, Documentation)
6. **RENDER** — emit: `[!]` → `[TARGET]` → `[CALLERS]` → `[CALLEES]` → `[CONTEXT]` → `[DOCUMENTATION]` → `[BODY]`

**6 per-source scoring formulas** (design doc §6.5):

Shared recency: `recency(t) = e^(-0.01 × hours_elapsed(t))`

- **GraphNode**: `0.4 × centrality + 0.6 × bm25(query, name+sig+doc)` (no recency)
- **Annotation**: `(0.5 × bm25(text+tags) + 0.3 × proximity + 0.2 × recency) × (0.6 if stale else 1.0)`
- **NodeHistory**: `0.6 × significance + 0.4 × (1.0 / version_distance)`
- **SessionActivity**: `0.5 × recency + 0.5 × jaccard(activity_nodes, traversal_nodes)`
- **DocChunk**: `0.7 × bm25_doc + (0.3 if priority_doc else 0.0)`
- **BehavioralSignal**: pinned, score fixed at 1.0

**Token budget** (design doc §6.6): 8k default, 10% headroom → effective 7200. Greedy fill (skip oversized, continue). Body inclusion if leftover >200 tokens. Budget exhausted → pinned only + message.

**Token estimator** (design doc §6.8): per-tool "without index" estimates using `files.raw_token_estimate`. Non-blocking, fail-silent. Logged to `token_log` in `daemon_meta.db`.

**Deliverable**: Capsule for a known symbol produces correctly formatted output matching design doc §6.7 example, within budget.

---

## Phase 7: Daemon Lifecycle

**Goal**: Persistent background daemon with UDS listener, file watcher, branch handling.
**Estimated effort**: Large — 12-step startup, branch handling, concurrency, signal handling.

**Source files**:

```
src/daemon/mod.rs             # daemon main loop (tokio)
src/daemon/socket.rs          # UDS listener, length-prefixed JSON
src/daemon/watcher.rs         # notify-debouncer-full + VCS deferral
src/daemon/coordinator.rs     # ReindexCoordinator: branch swap, cold start
src/daemon/handlers.rs        # request handlers
```

**12-step startup** (design doc §11.1):

1. Acquire exclusive flock on `.scavenger/daemon.lock`
2. Write PID to `.scavenger/daemon.pid`
3. Open `daemon_meta.db` with WAL PRAGMAs
4. Detect branch via `git rev-parse --abbrev-ref HEAD`
5. Open per-branch index DB (cold start if doesn't exist)
6. Start UDS listener immediately — degraded responses during indexing
7. Check `last_shutdown` — if dirty, full freshness scan
8. Set `last_shutdown = 'dirty'`
9. Compare (path, mtime_ns, size) of indexed files vs filesystem, re-index mismatches
10. Load petgraph + reverse index; recompute PageRank (30 iter, ~30-60ms)
11. Start file watcher (notify-debouncer-full)
12. Set `reindex_state = 'ready'`

**Shutdown** (design doc §11.2): SIGTERM/SIGINT → stop accepting, drain 5s, flush writes, `last_shutdown = 'clean'`, close DBs, remove PID.

**File watcher** (design doc §11.4): 300ms trailing-edge debounce, `.gitignore` via `ignore` crate, VCS deferral (pause on `.git/index.lock`), branch-switch detection after VCS batch, file routing (.md → doc_indexer, code → code indexer).

**Branch handling**:
- Warm switch (design doc §8.2): set switching → update branch → close DB → open new → reload graph → freshness check → re-index stale → set ready
- Cold start (design doc §8.3): set cold_start → git diff → copy parent DB → clear ephemeral → re-index changed → PageRank → swap → set ready
- Merge detection (design doc §8.5): `git log -1 --format=%P HEAD`, 2+ parents → annotation union-merge
- Cleanup: delete index when branch deleted (hourly check + startup)

**Daemon log** (NFR-011): structured events to `.scavenger/daemon.log`, rotated 10MB / 2 files.

**Session tracking** (design doc §11.5): session_id from hook payloads or MCP context, fallback UUID.

**Deliverable**: Daemon starts, accepts UDS, serves capsules, re-indexes on changes, handles branch switches, shuts down cleanly.

---

## Phase 8: MCP Bridge & Hooks

**Goal**: MCP stdio bridge and Claude Code hook executables.
**Estimated effort**: Medium — rmcp provides the framework, hook logic is straightforward.

**Source files**:

```
src/bridge/mod.rs             # MCP bridge: stdio JSON-RPC ↔ UDS
src/hooks/mod.rs              # PreToolUse / PostToolUse handlers
src/hooks/register.rs         # settings.json hook + MCP registration
```

**5 MCP tools** (design doc §9.1, implemented via rmcp `#[tool]` proc macros):

1. `get_capsule(file, symbol?, query?)` → CapsuleResult — federation fallback if local empty
2. `read_annotations(anchor_type?, anchor_value?, tags?, query?, session_summary?, limit?)` → Vec
3. `write_annotation(id?, text, tags?, symbol?, file?, scope?)` → AnnotationResult — upsert, anchor cascade (symbol FTS5 → file → scope → None), disambiguation within 20%
4. `delete_annotation(id)` → DeleteResult
5. `search_docs(query, limit?)` → Vec — fans out to federated repos

**Hook contracts**:
- PreToolUse (Read): stdin JSON → UDS → capsule → stdout `{"additionalContext": "..."}`. Exit 0 always.
- PostToolUse (Write/Edit/MultiEdit): stdin JSON → UDS → enqueue → stdout `{}`. Exit 0 always.
- Performance: binary startup ~0.5ms + socket ~0.1ms + capsule ~10-30ms + serialize ~1ms. Partial at 100ms.

**Hook registration**: deep-merge into `.claude/settings.local.json` (read → merge → temp file + rename with `fs2` exclusive lock).

**Deliverable**: MCP bridge end-to-end. Hooks fire correctly. Failure = exit 0 + empty response.

---

## Phase 9: CLI

**Goal**: Complete command surface via clap.
**Estimated effort**: Medium — many commands but each delegates to existing modules.

**Commands** (design doc §12):

- `scavenger init` — 5 steps: mkdir .scavenger/ (mode 0700) → bulk index → register hooks → register MCP bridge → start daemon
- `scavenger daemon` — start daemon foreground
- `scavenger index [path]` — manual re-index
- `scavenger capsule <file> [symbol] [--query] [--budget]` — print capsule to stdout
- `scavenger memory [--query] [--limit]` — query annotations
- `scavenger graph stats` — node/edge counts, centrality top-10
- `scavenger graph show <symbol>` — ASCII neighborhood tree
- `scavenger annotate <symbol> "<text>"` — add annotation
- `scavenger merge-annotations <branch>` — manual merge
- `scavenger doctor [--verbose] [--format=json]` — 5 categories, trait-based registry, exit 0/1/2
- `scavenger stats [--session] [--branch]` — token savings report
- `scavenger federate add|remove|list|verify` — federation management
- `scavenger hook pre-tool-use` / `post-tool-use` — CLI fallback

**Doctor checks** (design doc §12.3): trait `DiagnosticCheck` with `name()`, `category()`, `run()`. Categories: Process, FileIntegrity, Config, Dependencies, Resources. Output: `[✓]`/`[✗]`/`[!]` with color (respecting `NO_COLOR`).

**Deliverable**: All commands parse and execute. `scavenger init` works end-to-end.

---

## Phase 10: Federation & Analytics

**Goal**: Cross-repo query fan-out and token savings tracking.
**Estimated effort**: Small — well-defined, additive.

**Source files**:

```
src/daemon/federation.rs      # federated repo connections + fan-out
src/graph/estimator.rs        # per-tool "without index" estimates
```

**Federation** (design doc §7 federation): read federated repo's `daemon_meta.db` for `current_branch`, open that branch's DB read-only, query FTS5. Validate on first connect (tables exist, user_version in range, quick_check). Cache validation.

**Token estimator per-tool logic** (design doc §6.8):

| Tool | Without-index estimate |
|------|----------------------|
| get_capsule | `files.raw_token_estimate` for seed file + all 1-hop neighbor files |
| search_docs | Sum of matched doc files' estimates |
| read_annotations | Anchor file's estimate, or 0 if project-level |
| write_annotation | 0 for creates; same as read for updates |
| delete_annotation | 0 |

**Deliverable**: `get_capsule` falls back to federated repo. `scavenger stats` shows accurate savings.

---

## Phase 11: Integration Testing & Polish

**Goal**: End-to-end validation, performance, documentation.
**Estimated effort**: Medium — testing against real codebases.

**Test scenarios**:
- Init → index → capsule → edit → re-index → verify capsule updated
- Branch: create → cold start → edit → switch back → warm switch → verify independent state
- Multi-session: two MCP bridges → same daemon simultaneously
- Performance: 5000-file project benchmarks for SC-001 through SC-008
- BM25 validation: real codebase, 20+ representative queries, evaluate ranking quality
- Anti-pattern: synthetic scenarios for all 7 detectors
- Daemon log: verify structured events, rotation, doctor integration

**Documentation**:
- `.scavenger.toml.example` with all sections documented
- README.md with installation, usage, configuration

**Deliverable**: All success criteria (SC-001 through SC-008) verified. Binary ships.

---

## Project Structure

### Documentation (this feature)

```text
specs/v1-core-engine/
├── spec.md              # Feature specification (clarified)
├── context.md           # Feature context
├── plan.md              # This file
├── research.md          # Phase 0 research findings
├── data-model.md        # Full data model
├── architecture.md      # Feature-level architecture
└── tasks.md             # Task breakdown (created by /cx-spec.tasks)
```

### Source Code (repository root)

```text
scavenger/
├── Cargo.toml
├── rust-toolchain.toml
├── .scavenger.toml.example
├── src/
│   ├── main.rs                     # CLI entry, clap subcommand dispatch
│   ├── config.rs                   # .scavenger.toml loading and validation
│   ├── daemon/
│   │   ├── mod.rs                  # daemon main loop: UDS + watcher + signals
│   │   ├── handlers.rs             # request handlers (5 tools + hooks)
│   │   ├── coordinator.rs          # ReindexCoordinator: branch swap, cold start
│   │   ├── watcher.rs              # notify-debouncer-full + VCS deferral
│   │   ├── socket.rs               # UDS listener, length-prefixed JSON
│   │   └── federation.rs           # federated repo connections + fan-out
│   ├── bridge/
│   │   └── mod.rs                  # MCP bridge: stdio JSON-RPC ↔ UDS
│   ├── graph/
│   │   ├── mod.rs                  # GraphState, StableGraph, reverse index
│   │   ├── types.rs                # NodeWeight, EdgeWeight, NodeId
│   │   ├── index.rs                # tree-sitter → graph, skeleton, sig_hash
│   │   ├── doc_indexer.rs          # markdown chunking, doc_chunks
│   │   ├── traversal.rs            # intent-driven BFS/DFS with caps
│   │   ├── similarity.rs           # identity migration heuristic
│   │   └── estimator.rs            # "without index" token estimator
│   ├── capsule/
│   │   ├── mod.rs                  # 6-stage pipeline orchestrator
│   │   ├── gather.rs               # GATHER: collect from all sources
│   │   ├── score.rs                # SCORE: 6 per-source formulas
│   │   └── render.rs               # GROUP + RENDER: output formatting
│   ├── query/
│   │   ├── mod.rs                  # query engine entry
│   │   ├── intent.rs               # intent detection + strategy selection
│   │   └── search.rs               # FTS5 BM25 + centrality ranking
│   ├── memory/
│   │   ├── mod.rs                  # three-layer orchestration
│   │   ├── versions.rs             # Layer 1: node version history
│   │   ├── annotations.rs          # Layer 2: annotation CRUD + staleness
│   │   ├── signals.rs              # Layer 3: behavioral signals + TTL
│   │   ├── session.rs              # Layer 3: session activity log
│   │   └── antipattern.rs          # 7 anti-pattern detectors
│   ├── hooks/
│   │   ├── mod.rs                  # PreToolUse / PostToolUse handlers
│   │   └── register.rs             # settings.json hook + MCP registration
│   └── db/
│       ├── mod.rs                  # connection management
│       ├── schema.rs               # migrations, all CREATE TABLE
│       └── queries.rs              # typed query helpers
├── tests/
│   ├── integration/                # end-to-end tests
│   └── fixtures/                   # sample multi-language projects
└── docs/
    └── plans/
        └── 2026-02-28-consolidated-design.md
```

**Structure Decision**: Single Rust binary crate with module-per-subsystem organization matching the design doc §14 layout. No workspace, no multiple crates — keeps the single-binary constraint simple.

## Triage Framework: [SYNC] vs [ASYNC] Classification

**Execution Strategy**: Hybrid model — complex algorithmic work requires human review, well-defined infrastructure can be agent-delegated.

### Preliminary Task Classification

| Task Category | Est. [SYNC] | Est. [ASYNC] | Rationale |
|---------------|-------------|--------------|-----------|
| Core Algorithms | 8 | 0 | Scoring formulas, similarity heuristic, anti-pattern detectors, concurrency model — complex logic requiring correctness review |
| Data Operations | 1 | 4 | Schema SQL is verbatim; only migration logic needs review |
| Integrations | 2 | 2 | MCP tools and daemon protocol need contract review; hook registration is mechanical |
| Infrastructure | 2 | 6 | Startup/shutdown need review; config, logging, CLI scaffolding are standard |
| Testing | 2 | 3 | Integration test design needs review; fixture creation and unit tests are standard |

### High-Risk [SYNC] Classifications

- Tree-sitter symbol extraction per language (15 languages, nuanced node-type mappings)
- Split-phase concurrency model (graph locking correctness, SQLite-before-graph ordering)
- Similarity heuristic (weighted scoring, pending orphans buffer, annotation migration)
- Capsule pipeline scoring (6 formulas, recency decay, proximity scoring, stale penalties)
- Anti-pattern detectors (7 algorithms with specific thresholds and dedup logic)
- Branch handling (warm/cold switch state machine, annotation fork/merge semantics)
- MCP tool implementations (contract compliance, anchor resolution cascade, disambiguation)
- Daemon startup/shutdown (12-step sequence, signal handling, crash recovery invariants)

### Agent-Delegated [ASYNC] Classifications

- Cargo.toml + module scaffolding (well-defined dependency list)
- SQLite schema (verbatim SQL from design doc)
- FTS5 virtual tables and sync triggers (exact SQL defined)
- Config loading (.scavenger.toml with toml crate, range clamping)
- CLI command scaffolding (clap derive macros, subcommand dispatch)
- Token estimator (simple per-tool heuristics)
- Stats reporting (SQL aggregation + formatted output)
- Doctor checks (trait-based, straightforward diagnostic checks)
- Hook registration (JSON deep-merge)
- Daemon log (structured events, file rotation)
- Test fixtures (sample multi-language projects)

### Triage Audit Trail

| Task | Classification | Primary Criteria | Risk | Rationale |
|------|---------------|-----------------|------|-----------|
| Schema creation | ASYNC | Well-defined CRUD | Low | SQL is verbatim in design doc |
| Config loading | ASYNC | Standard patterns | Low | toml parsing + validation is mechanical |
| Graph data structures | SYNC | Architectural | Med | Concurrency wrapper design affects all consumers |
| Tree-sitter extraction | SYNC | Complex logic | High | 15 languages, each with unique node types |
| Similarity heuristic | SYNC | Complex algorithm | High | Weighted scoring affects annotation survival |
| Capsule scoring | SYNC | Complex algorithm | High | 6 formulas, correctness affects token efficiency |
| Anti-pattern detectors | SYNC | Complex logic | High | False positives degrade UX, false negatives lose value |
| Daemon lifecycle | SYNC | Architectural | High | 12-step startup, crash recovery, state machine |
| MCP tools | SYNC | Integration + contracts | Med | Contract compliance, disambiguation logic |
| CLI scaffolding | ASYNC | Standard patterns | Low | clap derive, delegation to existing modules |
| Doctor checks | ASYNC | Standard patterns | Low | Trait-based, straightforward checks |
| Federation | SYNC | Integration | Med | Cross-DB reads, validation, error handling |
| Token estimator | ASYNC | Clear spec | Low | Simple per-tool heuristics |

## Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| BM25 ranking quality poor for code | Medium | High | Empirical validation in Phase 11 with 20+ queries; tantivy as escape hatch |
| rmcp API breaking changes | Low | Medium | v0.17.0 stable, 4.3M downloads; hand-rolled JSON-RPC fallback documented |
| Tree-sitter grammar gaps | Medium | Low | Test each language early in Phase 3; accept partial for v1 |
| Split-phase lock contention | Low | Medium | Accepted for v1; add priority queue only if latency issues surface |
| Binary size exceeds 50MB | Low | Low | 15 grammar crates may be large; strip symbols, LTO if needed |

## Complexity Tracking

No constitution violations — no entries needed.
