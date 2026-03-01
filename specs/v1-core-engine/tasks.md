# Tasks: Scavenger v1 Core Engine

**Input**: Design documents from `/specs/v1-core-engine/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, architecture.md
**Design Doc**: `docs/plans/2026-02-28-consolidated-design.md` (authoritative for all implementation-level detail)

**Tests**: Tests are REQUIRED. TDD is disabled (`tdd=false`), so tests appear AFTER implementation in the Polish phase.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [SYNC/ASYNC] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[SYNC]**: Requires human review (complex logic, security-critical, ambiguous requirements)
- **[ASYNC]**: Can be delegated to async agents (well-defined CRUD, repetitive tasks, clear specs)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

- **Single project**: `src/`, `tests/` at repository root
- Rust binary crate, no workspace

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Bootable Rust project with module stubs, dependency manifest, and configuration loading.

- [X] T001 [ASYNC] Create `Cargo.toml` with all 22 crate dependencies per plan technical context (rmcp, tree-sitter grammars x15, petgraph, rusqlite, tokio-rusqlite, tokio, notify-debouncer-full, clap, serde, parking_lot, strsim, heck, rayon, fs2, owo-colors, md5, toml, schemars, ignore)
- [X] T002 [P] [ASYNC] Create `rust-toolchain.toml` pinning stable Rust (edition 2024, minimum 1.85+)
- [X] T003 [P] [ASYNC] Create `.scavenger.toml.example` reference config with all sections documented (token budget, degree cap, node budget, builtins blocklist, doc patterns, analytics pricing, federation repos) per design doc section 13
- [X] T004 [ASYNC] Create `src/main.rs` with clap subcommand dispatch skeleton — declare all module stubs (`mod config`, `mod db`, `mod graph`, `mod query`, `mod capsule`, `mod memory`, `mod daemon`, `mod bridge`, `mod hooks`) so `cargo build` succeeds with empty implementations
- [X] T005 [P] [ASYNC] Implement `src/config.rs` — `.scavenger.toml` loading via `toml` crate, serde deserialization, validation with range clamping (all numeric fields clamped to nearest valid bound with logged warning), missing file uses built-in defaults (FR-016)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Database layer and graph core that ALL user stories depend on.

**CRITICAL**: No user story work can begin until this phase is complete.

- [X] T006 [ASYNC] Implement `src/db/mod.rs` — SQLite connection management: per-branch DB open (`.scavenger/indexes/<branch>.db`), shared `daemon_meta.db` open, WAL PRAGMAs at connection open (`journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5000`, `cache_size=-64000`, `mmap_size=268435456`, `auto_vacuum=INCREMENTAL`)
- [X] T007 [SYNC] Implement `src/db/schema.rs` — all CREATE TABLE statements (nodes, edges, files, node_versions, annotations, behavioral_signals, session_log, doc_chunks), FTS5 virtual tables (nodes_fts, annotations_fts, doc_chunks_fts) with content sync triggers, indexes, daemon_meta tables (daemon_meta, federated_repos, token_log). `PRAGMA user_version = 1` on creation. Sequential forward migrations. Downgrade guard: refuse if `user_version > KNOWN_MAX_VERSION` (FR-017). All SQL verbatim from data-model.md.
- [X] T008 [P] [ASYNC] Implement `src/db/queries.rs` — typed query helpers for insert/update/delete/select on all tables, parameterized queries only
- [X] T009 [P] [SYNC] Implement `src/graph/types.rs` — `NodeWeight` struct (id, kind, name, file_path, line_start, line_end, signature, signature_hash, docstring, skeleton, centrality, checksum), `EdgeWeight` struct (kind, weight, confidence), `NodeId` type (`hash(file_path, symbol_name, signature)`), `NodeKind` enum (9 types: Function, Method, Class, Interface, Type, Enum, ExportedVar, Module, File), `EdgeKind` enum (7 types: Imports, Calls, TypeRef, Extends, Implements, Exports, Contains), `Confidence` enum (precise, heuristic, speculative)
- [X] T010 [SYNC] Implement `src/graph/mod.rs` — `GraphState` wrapping `petgraph::StableGraph<NodeWeight, EdgeWeight, Directed>` + reverse index `HashMap<NodeId, Vec<PathBuf>>` in `Arc<parking_lot::RwLock<GraphState>>`. Load graph from SQLite. Save centrality to SQLite during idle checkpoint (5s after last query). PageRank via `petgraph::algo::page_rank(&g, 0.85, 30)`. Add/remove node/edge methods.

**Checkpoint**: Foundation ready — `cargo build` succeeds, DB creates schema, graph loads/saves. User story implementation can now begin.

---

## Phase 3: User Story 1 — Initialize Scavenger on a Project (Priority: P1) MVP

**Goal**: `scavenger init` works end-to-end: indexes codebase, registers hooks/MCP, starts daemon.

**Independent Test**: Run `scavenger init` in a sample multi-language project, verify `.scavenger/` structure, SQLite contents, hook registration in `.claude/settings.local.json`, running daemon.

### Implementation for User Story 1

- [X] T011 [SYNC] [US1] Implement `src/graph/index.rs` — tree-sitter symbol extraction for all 15 languages using `tags.scm` query files: language-to-grammar dispatch, per-language node type mapping (plan Phase 3 table), signature extraction (definition start to body start), docstring extraction (per-language rules: Rust `///`, Python first expr_stmt string, Java/C#/PHP/Kotlin/Swift `/** */`, TS/Go/C/C++ preceding comment, Ruby `#`), skeleton generation (`signature + docstring`), `signature_hash` (first 8 hex of MD5 over normalized sig), FTS5 token splitting via `heck` crate. Parallelism: `rayon::par_iter()` with thread-local `Parser` instances (Parser is not Send). Initial bulk index (no split-phase for init).
- [X] T012 [P] [ASYNC] [US1] Implement `src/graph/doc_indexer.rs` — markdown chunking at heading boundaries, sub-split at 100 lines, `content_hash` (MD5[0..8]) for incremental updates, insert into `doc_chunks` + `doc_chunks_fts`
- [X] T013 [SYNC] [US1] Implement `src/daemon/mod.rs` — daemon main loop with tokio runtime, 12-step startup sequence (flock `.scavenger/daemon.lock`, write PID, open `daemon_meta.db`, detect branch via `git rev-parse`, open per-branch DB, start UDS listener for degraded mode, dirty check, set dirty flag, freshness scan, load graph + PageRank, start file watcher, set ready). Graceful shutdown: SIGTERM/SIGINT handler, stop accepting, 5s drain, flush writes, `last_shutdown = 'clean'`, close DBs, remove PID.
- [X] T014 [P] [SYNC] [US1] Implement `src/daemon/socket.rs` — UDS listener on `.scavenger/daemon.sock`, length-prefixed JSON protocol, concurrent request handling via tokio tasks
- [X] T015 [SYNC] [US1] Implement `src/daemon/handlers.rs` — request dispatch: route `capsule`, `hook_pre`, `hook_post`, `annotation_read`, `annotation_write`, `annotation_delete`, `search_docs`, `status` to appropriate modules. Session tracking: extract `session_id` from hook payloads or MCP context, fallback UUID.
- [X] T016 [P] [ASYNC] [US1] Implement `src/hooks/register.rs` — deep-merge Scavenger hook entries and MCP bridge config into `.claude/settings.local.json` (read existing → JSON merge → write via temp file + rename with `fs2::FileExt::lock_exclusive()`)
- [X] T017 [SYNC] [US1] Implement CLI `init` command in `src/main.rs` — 5 steps: `mkdir .scavenger/` with mode 0700 → bulk index all source files → register hooks via `hooks/register.rs` → register MCP bridge → start daemon. Add `.scavenger/` to `.gitignore`. Handle re-init (detect existing `.scavenger/`, provide guidance).
- [X] T018 [SYNC] [US1] Implement `src/daemon/coordinator.rs` — `ReindexCoordinator` struct: branch detection, DB open/close coordination, freshness scan (`compare (path, mtime_ns, size)` of indexed files vs filesystem, re-index mismatches). Used by daemon startup and branch switching.

**Checkpoint**: `scavenger init` works. Daemon running. Files indexed. Hooks registered. Claude Code can see the hooks.

---

## Phase 4: User Story 2 — Automatic Capsule Injection on File Read (Priority: P1)

**Goal**: PreToolUse hook intercepts Read, serves focused capsule via daemon, Claude receives token-efficient context.

**Independent Test**: Trigger Read hook with a known file path, verify capsule contains target symbol, callers, callees, annotations, within token budget.

### Implementation for User Story 2

- [X] T019 [P] [SYNC] [US2] Implement `src/query/intent.rs` — hybrid intent classifier: keyword priority table (Debug/Refactor/Understand/Extend/Review → keyword lists from plan Phase 4), fuzzy match via `strsim` Jaro-Winkler, BM25 fallback, default `Understand`. Multi-intent: top-2 within 0.1 score → 60/40 weighted union of traversal strategies.
- [X] T020 [P] [SYNC] [US2] Implement `src/query/search.rs` — FTS5 BM25 query execution (`SELECT ... FROM nodes_fts JOIN nodes ... WHERE nodes_fts MATCH ? ORDER BY bm25(nodes_fts) LIMIT 50`), sign-flip normalization (`-bm25 / max_magnitude`), score composition `0.6 * normalize(bm25) + 0.4 * normalize(centrality)`, centrality from in-memory graph
- [X] T021 [SYNC] [US2] Implement `src/query/mod.rs` — query engine entry: target resolution (file + optional symbol → NodeId), strategy dispatch based on detected intent, explosion mitigation (degree cap >50, node budget 100, builtins blocklist from config)
- [X] T022 [SYNC] [US2] Implement `src/graph/traversal.rs` — intent-driven traversals per plan Phase 4 mapping: Debug → reverse BFS (3 up, 2 down), Refactor → forward DFS (transitive cap 100), Understand → bidirectional BFS (2 each), Extend → BFS sibling/implements (1-2), Review → bidirectional BFS (2 all). Degree cap >50 skips utility functions.
- [X] T023 [SYNC] [US2] Implement `src/capsule/gather.rs` — GATHER stage: collect candidate items from all sources in parallel (nodes via traversal, annotations via FTS5, doc_chunks, node_versions, session_log, behavioral_signals, priority docs from config)
- [X] T024 [SYNC] [US2] Implement `src/capsule/score.rs` — SCORE stage: 6 per-source scoring formulas. Shared `recency(t) = e^(-0.01 * hours_elapsed(t))`. GraphNode: `0.4*centrality + 0.6*bm25`. Annotation: `(0.5*bm25 + 0.3*proximity + 0.2*recency) * (0.6 if stale)`. NodeHistory: `0.6*significance + 0.4*(1/version_distance)`. SessionActivity: `0.5*recency + 0.5*jaccard`. DocChunk: `0.7*bm25_doc + (0.3 if priority)`. BehavioralSignal: pinned, score 1.0.
- [X] T025 [SYNC] [US2] Implement `src/capsule/render.rs` — PIN stage: target node + active signals (pinned) + 1-hop structural (semi-pinned). TRIM stage: sort unpinned by score DESC, greedy fill remaining budget (skip oversized, continue). GROUP stage: assign to output groups. RENDER stage: emit section ordering per FR-018: `[!]` → `[TARGET]` → `[CALLERS]` → `[CALLEES]` → `[CONTEXT]` → `[DOCUMENTATION]` → `[BODY]` (if leftover >200 tokens). Empty sections omitted. Stale annotations get `[STALE]` suffix. Scores NOT in output.
- [X] T026 [SYNC] [US2] Implement `src/capsule/mod.rs` — 6-stage pipeline orchestrator: GATHER → SCORE → PIN → TRIM → GROUP → RENDER. Token budget enforcement: 8k default, 10% headroom → effective 7200. Budget exhausted → pinned only + message.
- [X] T027 [SYNC] [US2] Implement `src/bridge/mod.rs` — MCP bridge: stdio JSON-RPC ↔ UDS translation via `rmcp`. Declare 5 MCP tools with `#[tool]` proc macros: `get_capsule(file, symbol?, query?)`, `read_annotations(...)`, `write_annotation(...)`, `delete_annotation(id)`, `search_docs(query, limit?)`. Federation fallback in `get_capsule` if local GATHER empty.
- [X] T028 [SYNC] [US2] Implement `src/hooks/mod.rs` — PreToolUse handler: parse stdin JSON `{session_id, tool_name, tool_input}`, connect to daemon UDS, request capsule, format stdout `{"additionalContext": "..."}`. Exit 0 always. Partial fallback at 100ms (pinned items only). Performance target: binary startup ~0.5ms + socket ~0.1ms + capsule ~10-30ms + serialize ~1ms.

**Checkpoint**: Read tool triggers hook → capsule injected → Claude sees focused context instead of raw file. Core value proposition works.

---

## Phase 5: User Story 3 — Incremental Re-indexing on Edits (Priority: P1)

**Goal**: PostToolUse hook triggers re-index on writes; daemon updates graph, migrates annotations on renames, flags stale annotations.

**Independent Test**: Edit a file, wait for debounce, verify new/changed symbols in graph and stale annotations flagged.

### Implementation for User Story 3

- [X] T029 [SYNC] [US3] Implement `src/daemon/watcher.rs` — `notify-debouncer-full` file watcher: 300ms trailing-edge debounce, `.gitignore` filtering via `ignore` crate, VCS deferral (pause on `.git/index.lock` presence, resume on release), branch-switch detection after VCS batch, file routing (`.md`/`.markdown` → `doc_indexer`, code files → code indexer per extension-to-language mapping)
- [X] T030 [SYNC] [US3] Implement 13-step re-indexing flow in `src/graph/index.rs` — collect old NodeIds for changed file, re-parse with tree-sitter, build new nodes/edges, compute sig_hash, diff old vs new NodeId sets (unchanged/orphaned/new), run similarity heuristic on orphans, migrate annotations for matches, generate skeletons, rebuild edges (delete old from file, insert new), update reverse index, queue cross-file cascade, defer PageRank, update files table
- [X] T031 [SYNC] [US3] Implement split-phase concurrency in `src/graph/index.rs` — Phase 1 Prep (no lock): re-parse, build new structures, run similarity, pure local computation. Phase 2 Swap (write lock ~5-15ms): commit SQLite tx (DELETE old, INSERT new, FTS5 triggers fire), update graph, update reverse index. SQLite-before-graph ordering invariant. Phase 3 Deferred PageRank (read lock): once per debounce batch, not per file. Initial index bypass for `scavenger init`.
- [X] T032 [SYNC] [US3] Implement `src/graph/similarity.rs` — identity migration heuristic: 5-component weighted score (`name_similarity * 0.3` via Jaro-Winkler, `signature_similarity * 0.25` via param count/names/return type, `body_similarity * 0.25` via hash or called-symbol set, `edge_neighborhood * 0.15` via Jaccard on connected NodeIds, `file_proximity * 0.05`). Threshold >0.6 → match (migrate annotations + versions). Pending orphans buffer: hold unmatched for one extra debounce cycle before archiving.
- [X] T033 [ASYNC] [US3] Implement PostToolUse handler in `src/hooks/mod.rs` — parse stdin JSON, connect to daemon UDS, enqueue re-index for Write/Edit/MultiEdit tool_name, stdout `{}`, exit 0 always
- [X] T034 [SYNC] [US3] Implement cross-file edge staleness resolution in `src/graph/index.rs` — reverse index lookup for affected source files, queue for async re-index, lazy resolution if capsule hits dangling edge before queue processed, WAL checkpoint during idle

**Checkpoint**: Edits trigger re-index within debounce window. Renames migrate annotations. Stale annotations flagged. Cross-file edges resolved.

---

## Phase 6: User Story 4 — Session Memory Persistence (Priority: P2)

**Goal**: Annotations CRUD via MCP tools, three-layer memory model, anti-pattern detection, cross-session persistence.

**Independent Test**: Write annotation via MCP, restart daemon (new session), verify annotation in capsule for anchored symbol.

### Implementation for User Story 4

- [X] T035 [P] [ASYNC] [US4] Implement `src/memory/versions.rs` — Layer 1: node version history. Record new version on re-index (capture node_kind, signature, signature_hash, edges_json, body_hash). Retain last 5 per symbol (ordinal decay: delete oldest when exceeding 5). Version lookup for capsule inclusion.
- [X] T036 [SYNC] [US4] Implement `src/memory/annotations.rs` — Layer 2: annotation CRUD (create, read, update, delete). Flexible anchoring: node (NodeId), file (path), scope (name), project-level (NULL). Staleness detection: node → checksum change, file → mtime change, scope/None → manual only. Orphan cleanup: node-anchored where NodeId gone + stale >30 days → delete. FTS5 search over annotations_fts.
- [X] T037 [P] [ASYNC] [US4] Implement `src/memory/signals.rs` — Layer 3: behavioral signal storage. Insert signals from anti-pattern detectors. TTL pruning: delete where age >48h AND session_count >=2. Query by node_id or session_id for capsule inclusion.
- [X] T038 [P] [ASYNC] [US4] Implement `src/memory/session.rs` — Layer 3: session activity log. Append-only: record (session_id, event_type, file_path, symbol, timestamp) for read/query/edit events. TTL pruning same as signals. Query for capsule Jaccard scoring.
- [X] T039 [SYNC] [US4] Implement `src/memory/antipattern.rs` — 7 anti-pattern detectors, all with fire-once-at-N dedup via `HashSet<(SignalType, String)>`: **THRASHING** (ring buffer `HashMap<NodeId, VecDeque<(Instant, Vec<u8>)>>`, Levenshtein >0.9 via `strsim`, ≥3 edits in 5min, key: node_id), **DEAD_END** (zero incoming non-test edges after ≥10 actions or 15min, key: node_id), **CYCLE_INTRODUCED** (`has_path_connecting` before edge add, key: from::to), **LARGE_BLAST_RADIUS** (forward BFS >20 direct OR >50 transitive, key: node_id), **UNTESTED** (zero test-file edges, pattern matching test paths, key: node_id), **INDEX_BLIND_SPOT** (file on disk, zero nodes local + federated, key: file_path), **FAILED_SEARCH** (same normalized query 0 results ≥3 times, key: normalized_query).
- [X] T040 [SYNC] [US4] Implement `src/memory/mod.rs` — three-layer orchestration: coordinate version recording, annotation staleness checks, signal emission. Annotation fork on cold start (copy from parent branch DB). Annotation union-merge on merge commit (same anchor+text = dedup, different text = keep both).
- [X] T041 [SYNC] [US4] Implement MCP `write_annotation` and `delete_annotation` in `src/bridge/mod.rs` — write: upsert (id provided → update, omitted → create), anchor resolution cascade (symbol FTS5 top-1 → file → scope → None), disambiguation (multiple FTS5 matches within 20% → return alternatives in note field). Delete: by id.
- [X] T042 [SYNC] [US4] Implement MCP `read_annotations` with session summary in `src/bridge/mod.rs` — filters: anchor_type, anchor_value, tags, FTS5 query — combinable. Session summary mode (`session_summary: true`): return last session activity + stale annotations + active behavioral signals.

**Checkpoint**: Annotations persist across sessions. Anti-patterns surface in capsules. Memory fork/merge works on branches.

---

## Phase 7: User Story 5 — Branch-Aware Index Management (Priority: P2)

**Goal**: Daemon detects branch switches, swaps to correct per-branch index (warm or cold start), annotations are per-branch.

**Independent Test**: Create branch, make changes, switch back, verify independent index state and capsule results per branch.

### Implementation for User Story 5

- [X] T043 [SYNC] [US5] Implement warm switch in `src/daemon/coordinator.rs` — state machine: set `reindex_state = 'switching'` → update `current_branch` in daemon_meta → close current branch DB → open new branch DB → reload graph into memory → freshness scan → re-index stale files → set `reindex_state = 'ready'`
- [X] T044 [SYNC] [US5] Implement cold start in `src/daemon/coordinator.rs` — set `reindex_state = 'cold_start'` → `git diff` against parent → copy parent branch DB → clear ephemeral data (node_versions, behavioral_signals, session_log) → preserve annotations → re-index only changed files → recompute PageRank → swap → set `reindex_state = 'ready'`
- [X] T045 [SYNC] [US5] Implement merge detection in `src/daemon/coordinator.rs` — after branch switch, `git log -1 --format=%P HEAD` → if 2+ parent hashes → trigger annotation union-merge from source branch(es)
- [X] T046 [P] [ASYNC] [US5] Implement branch cleanup in `src/daemon/coordinator.rs` — delete per-branch index DB file when branch is deleted (hourly check via `git branch` + startup scan comparison)
- [X] T047 [SYNC] [US5] Implement branch-switch detection in `src/daemon/watcher.rs` — detect `.git/HEAD` change after VCS batch completes, compare new branch vs `current_branch` in daemon_meta, trigger warm switch or cold start via coordinator

**Checkpoint**: Branch switch → correct index. Cold start copies parent. Merge annotations union-merged. Deleted branches cleaned up.

---

## Phase 8: User Story 6 — Token Savings Analytics (Priority: P3)

**Goal**: Track per-tool token usage and report savings via `scavenger stats`.

**Independent Test**: Run several MCP tool calls, run `scavenger stats`, verify accurate per-tool counts and savings.

### Implementation for User Story 6

- [X] T048 [P] [ASYNC] [US6] Implement `src/graph/estimator.rs` — per-tool "without index" token estimates: `get_capsule` → seed file + 1-hop neighbor `raw_token_estimate`, `search_docs` → sum matched doc file estimates, `read_annotations` → anchor file estimate, `write_annotation` → 0 for creates / same as read for updates, `delete_annotation` → 0. Non-blocking, fail-silent.
- [X] T049 [ASYNC] [US6] Implement token logging in `src/daemon/handlers.rs` — after each tool call, insert into `token_log` in `daemon_meta.db` (timestamp, session_id, branch, tool_name, query, intent, tokens_actual, tokens_estimated, files_touched)
- [X] T050 [ASYNC] [US6] Implement CLI `stats` command in `src/main.rs` — `scavenger stats [--session] [--branch]`: per-session and all-time token savings report, SQL aggregation over token_log, percentage savings, cost estimates using configured pricing from `.scavenger.toml`, human-readable and JSON output

**Checkpoint**: `scavenger stats` shows accurate savings. Token log records all tool calls.

---

## Phase 9: User Story 7 — Health Diagnostics (Priority: P3)

**Goal**: `scavenger doctor` performs comprehensive health checks with actionable output.

**Independent Test**: Run `scavenger doctor` in various states (healthy, daemon stopped, corrupted DB, missing hooks) and verify diagnostic output.

### Implementation for User Story 7

- [X] T051 [ASYNC] [US7] Implement `DiagnosticCheck` trait and check registry in `src/main.rs` — trait with `name() -> &str`, `category() -> Category`, `run() -> CheckResult`. 5 categories: Process, FileIntegrity, Config, Dependencies, Resources.
- [X] T052 [ASYNC] [US7] Implement individual doctor checks — daemon process alive (PID file valid + process exists), PID file present, socket accessible, DB integrity (`PRAGMA integrity_check`), hook registration in settings.json, config validity, disk space, WAL size, branch DB existence
- [X] T053 [P] [ASYNC] [US7] Implement daemon log infrastructure in `src/daemon/mod.rs` — structured JSON events (startup, shutdown, branch switch, re-index start/complete, errors) to `.scavenger/daemon.log`, size rotation (10 MB max, 2 rotated files: `daemon.log.1`, `daemon.log.2`). Doctor log parsing for diagnostic insights (NFR-011).
- [X] T054 [ASYNC] [US7] Implement CLI `doctor` command in `src/main.rs` — `scavenger doctor [--verbose] [--format=json]`: iterate check registry, output `[check mark]`/`[x]`/`[!]` with color (respecting `NO_COLOR`), exit code 0 (all pass) / 1 (warnings) / 2 (failures)

**Checkpoint**: `scavenger doctor` reports accurate diagnostics. Daemon log captures structured events.

---

## Phase 10: User Story 8 — Federation (Priority: P3)

**Goal**: Cross-repo query fan-out for capsules and doc search when local results are empty.

**Independent Test**: Federate two repos, query symbol only in federated repo, verify capsule contains federated result with `[FEDERATED]` marker.

### Implementation for User Story 8

- [X] T055 [SYNC] [US8] Implement `src/daemon/federation.rs` — federated repo connection management: read federated repo's `daemon_meta.db` for `current_branch`, open that branch's DB read-only. Validation on first connect: tables exist, `user_version` in compatible range, `PRAGMA quick_check`. Cache validation results.
- [X] T056 [SYNC] [US8] Implement federation fan-out in `src/daemon/federation.rs` — FTS5 query against federated repos' `nodes_fts` and `doc_chunks_fts`, merge results with `[FEDERATED: /path/to/repo]` marker. Timeout per repo. Fail-open: log failure and return without federated results.
- [X] T057 [P] [ASYNC] [US8] Implement MCP `search_docs` tool with federation fan-out in `src/bridge/mod.rs` — FTS5 search over local `doc_chunks_fts`, then fan out to federated repos if configured
- [X] T058 [ASYNC] [US8] Implement CLI `federate` commands in `src/main.rs` — `scavenger federate add <path>` (validate + insert into federated_repos), `remove <path>`, `list` (show all with last_seen), `verify` (check accessibility, active branch DB, index freshness for all federated repos)

**Checkpoint**: `get_capsule` falls back to federated repos. `search_docs` fans out. CLI manages federation.

---

## Phase 11: Polish & Cross-Cutting Concerns

**Purpose**: Remaining CLI commands, all tests (TDD=false), integration validation, documentation, release build.

### Remaining CLI Commands

- [X] T072 [ASYNC] Implement remaining CLI commands in `src/main.rs` — `daemon` (start foreground), `index [path]` (manual re-index), `capsule <file> [symbol] [--query] [--budget]` (print to stdout), `memory [--query] [--limit]` (query annotations), `graph stats` (node/edge counts, centrality top-10), `graph show <symbol>` (ASCII neighborhood tree), `annotate <symbol> "<text>"` (add annotation), `merge-annotations <branch>` (manual merge), `hook pre-tool-use` / `post-tool-use` (CLI fallback)

### Tests (TDD disabled — tests after implementation)

- [X] T059 [P] [ASYNC] Create `tests/fixtures/` — sample multi-language project with files in Rust, Python, TypeScript, Go, Java (at minimum) with known symbol counts and relationships for test validation
- [X] T060 [P] [ASYNC] Unit tests for db module in `tests/` — schema creation on fresh DB, migration from v0 to v1, downgrade guard rejection, PRAGMA verification, CRUD operations on all tables
- [X] T061 [P] [ASYNC] Unit tests for graph module in `tests/` — node/edge add/remove, PageRank computation correctness, reverse index build and lookup, graph load/save roundtrip
- [X] T062 [P] [SYNC] Unit tests for tree-sitter extraction in `tests/` — per-language symbol extraction for all 15 languages using fixture files, verify node kinds, signatures, docstrings, skeleton content
- [X] T063 [P] [ASYNC] Unit tests for query engine in `tests/` — intent detection for each keyword set, fuzzy matching, multi-intent union, BM25 search ranking, traversal correctness per intent type
- [X] T064 [P] [ASYNC] Unit tests for memory module in `tests/` — annotation CRUD, staleness detection (checksum change, file modify), version history retention (cap at 5), behavioral signal TTL pruning, all 7 anti-pattern detector triggers on synthetic data
- [X] T065 [P] [ASYNC] Unit tests for capsule assembly in `tests/` — scoring formula correctness per source type, budget enforcement (under, at, over), section ordering per FR-018, stale annotation markers, empty section omission, body inclusion threshold
- [X] T066 [P] [ASYNC] Unit tests for similarity heuristic in `tests/` — weighted scoring calculation, threshold boundary (0.59 no match, 0.61 match), pending orphan buffer behavior, annotation migration on match
- [X] T067 [SYNC] Integration test in `tests/integration/` — full lifecycle: init → index → capsule → edit file → re-index → verify capsule updated with new content
- [X] T068 [SYNC] Integration test in `tests/integration/` — branch handling: create branch → cold start → edit → switch back → warm switch → verify independent state per branch
- [X] T069 [P] [ASYNC] Integration test in `tests/integration/` — concurrency: two MCP bridge connections → same daemon simultaneously → both receive valid capsules
- [X] T070 [SYNC] Performance validation in `tests/integration/` — benchmark against 5000-file project for SC-001 (70% token reduction), SC-002 (<50ms hook), SC-003 (<5s init index), SC-005 (<500ms branch switch)
- [X] T071 [SYNC] BM25 validation in `tests/integration/` — 20+ representative queries on real codebase, evaluate ranking quality, document results, flag if tantivy escalation needed

### Documentation & Release

- [X] T073 [ASYNC] Write `README.md` — installation, usage (`scavenger init`, hook-based workflow, MCP tools, CLI commands), configuration (`.scavenger.toml` options), troubleshooting (`scavenger doctor`)
- [X] T074 [ASYNC] Binary size and release build validation — `cargo build --release`, strip symbols (`strip` or `Cargo.toml` profile), LTO if needed, verify binary <50MB (SC-008)

### Additional Tasks (identified during audit 2026-03-01)

- [X] T075 [SYNC] [US2] Add `mcp-bridge` CLI subcommand to `src/main.rs` — starts the rmcp stdio MCP server, required for Claude Code to discover and invoke the 5 MCP tools
- [X] T076 [SYNC] [US2] Update `src/hooks/register.rs` — register the MCP bridge server in Claude Code plugin config so Claude Code auto-discovers Scavenger's MCP tools
- [X] T077 [ASYNC] [US2] Integration tests for MCP bridge — verify tool discovery, `get_capsule`, `read_annotations`, `write_annotation`, `delete_annotation`, `search_docs` round-trip through the rmcp bridge
- [X] T078 [ASYNC] Implement `[BODY]` section in `src/capsule/render.rs` — include full body of target node when remaining budget > 200 tokens per design section 6.6
- [X] T079 [ASYNC] Kotlin grammar — research `tree-sitter-kotlin` compatibility with tree-sitter 0.25; re-enable or document as known limitation. **Result**: tree-sitter-kotlin 0.3.8 requires tree-sitter <0.23. Documented as known limitation.

---

## Dependencies & Execution Order

### Phase Dependencies

```
Phase 1 (Setup) ─────► Phase 2 (Foundational) ─────┬──► Phase 3 (US1: Init) ──► Phase 4 (US2: Capsule)
                                                     │                            │
                                                     │                            ├──► Phase 5 (US3: Re-index)
                                                     │                            │
                                                     │   ┌───────────────────────────► Phase 6 (US4: Memory)
                                                     │   │
                                                     ├───┘
                                                     │
                                                     └──► Phase 7 (US5: Branch) [needs US1 daemon]
                                                          │
                                                          ├──► Phase 8 (US6: Analytics) [needs US2 capsule]
                                                          ├──► Phase 9 (US7: Doctor) [needs US1 daemon]
                                                          └──► Phase 10 (US8: Federation) [needs US2 capsule]
                                                               │
                                                               ▼
                                                          Phase 11 (Polish)
```

### User Story Dependencies

- **US1 (Init, P1)**: Depends on Phase 2 (Foundational). No dependencies on other stories. **MVP target.**
- **US2 (Capsule, P1)**: Depends on US1 (needs running daemon + indexed codebase)
- **US3 (Re-index, P1)**: Depends on US1 (needs daemon + watcher). Can develop in parallel with US2 (different files).
- **US4 (Memory, P2)**: Depends on Phase 2 (Foundational) for DB layer. Memory module implementation is independent, but MCP tools in US4 (T041-T042) build on bridge from US2.
- **US5 (Branch, P2)**: Depends on US1 (needs daemon + coordinator). Independent of US2-US4.
- **US6 (Analytics, P3)**: Depends on US2 (needs capsule pipeline for token tracking)
- **US7 (Doctor, P3)**: Depends on US1 (needs daemon). Independent of US2-US6.
- **US8 (Federation, P3)**: Depends on US2 (needs capsule assembly + query engine for fan-out)
- **Polish (Phase 11)**: Depends on all desired user stories being complete

### Within Each User Story

- Models/types before services/logic
- Core implementation before integration
- Services before endpoints/CLI
- Independent modules (marked [P]) can run in parallel

### Parallel Opportunities

- T002, T003, T005 can run in parallel with T001 (different files)
- T008, T009 can run in parallel with T006 (different files)
- T012 can run in parallel with T011 (different files)
- T014 can run in parallel with T013 (different files)
- T016 can run in parallel with T013, T014, T015 (different files)
- T019, T020 can run in parallel (different files in query/)
- T035, T037, T038 can run in parallel (different files in memory/)
- T048 can run in parallel with T049 (different files)
- All unit test tasks T059-T066 can run in parallel
- US3 and US4 implementation can overlap (different modules)
- US7 and US8 can overlap (different modules)

---

## Parallel Example: User Story 2 (Capsule Injection)

```text
# Parallel batch 1 — Query engine (different files):
T019: Intent classifier in src/query/intent.rs
T020: FTS5 BM25 search in src/query/search.rs

# Sequential — depends on T019+T020:
T021: Query engine entry in src/query/mod.rs
T022: Graph traversal in src/graph/traversal.rs

# Parallel batch 2 — Capsule pipeline (after T022):
T023: GATHER in src/capsule/gather.rs
T024: SCORE in src/capsule/score.rs
T025: RENDER in src/capsule/render.rs

# Sequential — depends on T023-T025:
T026: Pipeline orchestrator in src/capsule/mod.rs
T027: MCP bridge in src/bridge/mod.rs
T028: Hook handler in src/hooks/mod.rs
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001-T005)
2. Complete Phase 2: Foundational (T006-T010)
3. Complete Phase 3: User Story 1 — Init (T011-T018)
4. **STOP and VALIDATE**: `scavenger init` works on a real project, daemon runs, files indexed
5. Proceed to US2 for core value proposition

### Incremental Delivery

1. Setup + Foundational → Foundation ready (T001-T010)
2. US1 (Init) → Daemon running, files indexed (T011-T018)
3. US2 (Capsule) → Core value: token-efficient context injection (T019-T028)
4. US3 (Re-index) → Live updates on edits (T029-T034)
5. US4 (Memory) → Cross-session persistence (T035-T042)
6. US5 (Branch) → Multi-branch awareness (T043-T047)
7. US6-US8 (Analytics/Doctor/Federation) → Polish features (T048-T058)
8. Phase 11 → Tests, validation, documentation, release (T059-T074)

### Parallel Agent Strategy

With multiple agents after Foundational is complete:

- **Agent A**: US1 (Init) → US2 (Capsule) → US6 (Analytics)
- **Agent B**: US4 (Memory — module is independent) → US3 (Re-index, after US1 done)
- **Agent C**: US5 (Branch, after US1 done) → US7 (Doctor) → US8 (Federation)

---

## Task Summary

| Phase | Story | Tasks | [SYNC] | [ASYNC] | [P] |
|-------|-------|-------|--------|---------|-----|
| 1. Setup | — | 5 | 0 | 5 | 3 |
| 2. Foundational | — | 5 | 3 | 2 | 2 |
| 3. US1 Init | P1 | 8 | 6 | 2 | 2 |
| 4. US2 Capsule | P1 | 10 | 10 | 0 | 2 |
| 5. US3 Re-index | P1 | 6 | 5 | 1 | 0 |
| 6. US4 Memory | P2 | 8 | 4 | 4 | 3 |
| 7. US5 Branch | P2 | 5 | 4 | 1 | 1 |
| 8. US6 Analytics | P3 | 3 | 0 | 3 | 1 |
| 9. US7 Doctor | P3 | 4 | 0 | 4 | 1 |
| 10. US8 Federation | P3 | 4 | 2 | 2 | 1 |
| 11. Polish | — | 21 | 5 | 16 | 10 |
| **Total** | | **79** | **39** | **40** | **26** |

---

## Notes

- [P] tasks = different files, no dependencies on incomplete tasks
- [SYNC] tasks = require human review (complex algorithms, concurrency, contracts)
- [ASYNC] tasks = can be delegated to async agents (schema, config, scaffolding, standard patterns)
- [Story] label maps task to specific user story for traceability
- Each user story should be independently completable and testable
- Commit after each task or logical group
- Stop at any checkpoint to validate story independently
- Design doc (`docs/plans/2026-02-28-consolidated-design.md`) is the authoritative reference for all implementation-level detail
- Avoid: vague tasks, same file conflicts, cross-story dependencies that break independence
