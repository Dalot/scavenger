# Scavenger — Architecture Components Status

Quick-reference tracker for all architectural components. For full specifications, schemas, algorithms, and rationale, see the authoritative design document: **`docs/plans/2026-02-28-consolidated-design.md`**.

**Statuses**:
- `CLOSED` — decision made, documented, no open questions
- `OPEN` — deferred to v2

---

## Graph Layer

| Component | Status | Notes |
|-----------|--------|-------|
| Node data model (fields, types) | CLOSED | Defined in design doc |
| Edge types | CLOSED | Imports, Calls, TypeRef, Extends, Implements, Exports, Contains |
| NodeId scheme | CLOSED | `hash(file_path, symbol_name, signature)` |
| SQLite schema (nodes, edges, observations) | CLOSED | `_rowid INTEGER PRIMARY KEY` on nodes/annotations for stable FTS5 mapping. Schema migration via `PRAGMA user_version`. See consolidated design §3.5 |
| FTS5 virtual tables + sync triggers | CLOSED | `nodes_fts`, `annotations_fts`, `doc_chunks_fts` — all with `content_rowid`, all with AFTER INSERT/DELETE/UPDATE sync triggers. See consolidated design §3.5 |
| PageRank / centrality recalculation | CLOSED | 30 iterations, in-memory primary, idle-persisted to SQLite. Personalized PageRank noted for v2 (T3). See consolidated design §4.9, §3.7 |
| File re-indexing flow | CLOSED | Delete-and-rebuild per file, compare old vs new NodeId sets |
| Edge rebuild strategy | CLOSED | Replace all edges from changed file's nodes |
| Cross-file edge staleness | CLOSED | Reverse index + async cascade queue + lazy fallback. Add WAL checkpoint during idle (T7) |
| Similarity heuristic (identity migration) | CLOSED | Scoring function defined, threshold 0.6, pending orphans buffer (one extra debounce cycle). See consolidated design §4.7 |
| Node version history (Layer 1 memory) | CLOSED | State-snapshot pattern, `node_versions` table, last 5 versions per symbol (no time-based expiry), ordinal decay scoring. See consolidated design §7.2 |
| Graph type | CLOSED | `petgraph::StableGraph` in `Arc<parking_lot::RwLock<GraphState>>` with split-phase concurrency. See consolidated design §3.7 |
| `signature_hash` column on `nodes` and `node_versions` | CLOSED | 8-char MD5 of whitespace-normalized signature. Fast O(1) equality check for AST diff hot path. See consolidated design §4.1 |
| `files` table (file-level token estimates) | CLOSED | One row per indexed file (code and doc): `file_path`, `file_type` (code/doc), `raw_token_estimate` (len/4), `last_indexed`. Sole source of file-level token estimates (removed from `nodes` table). See consolidated design §3.5 |
| Document indexer (`doc_indexer.rs`) | CLOSED | Markdown chunking at heading boundaries, 100-line sub-split for large sections, incremental via content_hash, v1 = `.md`/`.markdown` only. See consolidated design §4.3 |
| `doc_chunks` table + FTS5 | CLOSED | `id, file_path, chunk_index, heading, start_line, end_line, content, token_estimate, content_hash`. FTS5 triggers keep index in sync. See consolidated design §3.5 |

---

## Memory & Observation Engine

| Component | Status | Notes |
|-----------|--------|-------|
| Three-layer memory model | CLOSED | Version history / Semantic annotations / Behavioral signals |
| Semantic annotations — data model | CLOSED | Flexible anchoring: Node, File, Scope, None |
| Semantic annotations — staleness by anchor type | CLOSED | Node→checksum, File→modify, Scope/None→manual |
| MCP annotation tools | CLOSED | read_annotations, write_annotation (upsert), delete_annotation |
| Behavioral signals — event types | CLOSED | THRASHING, DEAD_END, CYCLE_INTRODUCED, LARGE_BLAST_RADIUS, UNTESTED, INDEX_BLIND_SPOT, FAILED_SEARCH |
| Behavioral signals — TTL pruning | CLOSED | 48h or 2 sessions, whichever longer |
| Anti-pattern detection logic | CLOSED | All 7 detectors with fire-once-at-N dedup rule. CHECK constraint on `behavioral_signals.kind`. See consolidated design §3.5, §7.5 |
| `INDEX_BLIND_SPOT` behavioral signal | CLOSED | Emitted from GATHER stage AFTER federation fallback — only fires when file exists on disk but has no nodes locally or in federated repos. Doctor check included. See consolidated design §7.5 |
| `FAILED_SEARCH` anti-pattern detector | CLOSED | 7th detector: same FTS5 query → 0 results ≥3 times. Fires exactly once per session per query via dedup HashSet. See consolidated design §7.5 |
| "Fire once at N" deduplication rule | CLOSED | Per-session `HashSet<(SignalType, key)>` in AntiPatternDetector prevents duplicate signal emission across all 7 detectors. See consolidated design §7.5 |
| `detail TEXT` column on `behavioral_signals` | CLOSED | Stores query string (FAILED_SEARCH), tool name (tool error), or advice text (INDEX_BLIND_SPOT). See consolidated design §3.5 |
| Session activity log | CLOSED | Lightweight ephemeral log in Layer 3, TTL-pruned, per-branch, feeds SessionActivity scoring. See consolidated design §7.4 |
| Feedback loop (query frequency → budget) | CLOSED | Explicitly deferred to v2. See consolidated design §16 |
| Observation compaction / lifecycle | CLOSED | Eliminated — LLM manages its own annotations |
| Annotation fork (copy-on-create) | CLOSED | Annotations copied from parent branch on cold start. Each branch evolves independently. See consolidated design §8.4 |
| Annotation merge (union on merge commit) | CLOSED | On detected merge commit, union-merge annotations from source branch. Same anchor + same text = dedup; different text = keep both. Manual fallback: `scavenger merge-annotations`. See consolidated design §8.4 |

---

## Query Engine

| Component | Status | Notes |
|-----------|--------|-------|
| Intent detection (keyword classifier) | CLOSED | Hybrid: keyword priority → fuzzy match → BM25 fallback → default Understand. Multi-intent weighted union. See consolidated design §5.1 |
| Strategy-to-traversal mapping | CLOSED | 5 strategies with hop counts, degree cap >50, node budget 100, builtins blocklist. See consolidated design §5.2 |
| FTS5 search integration | CLOSED | FTS5 `bm25()` with post-query composition: `0.6 × bm25 + 0.4 × centrality`. See consolidated design §5.3 |
| TF-IDF scoring layer | CLOSED | Eliminated — FTS5 BM25 subsumes this |
| Capsule node ranking | CLOSED | Per-source formulas → [0,1], unified competition. See consolidated design §6.5 |
| Scope tags — anchoring to graph | CLOSED | Hybrid: path-prefix primary, virtual scope nodes lazily materialized on annotation. See consolidated design §5.4 |

---

## Capsule Assembly

| Component | Status | Notes |
|-----------|--------|-------|
| Capsule format | CLOSED | Unified pipeline: PIN→TRIM→GROUP→RENDER. Sections are presentation-only |
| Pipeline architecture | CLOSED | 6 stages: GATHER, SCORE, PIN, TRIM, GROUP, RENDER. See consolidated design §6 |
| `DocChunk` context item source type | CLOSED | New `ItemSource::DocChunk(i64)` and `OutputGroup::Documentation`. BM25 + priority-doc boost scoring formula. Rendered in `[DOCUMENTATION]` section after `[CONTEXT]`. See consolidated design §5 |
| Pinning rules | CLOSED | Target node + active behavioral signals + 1-hop structural guarantee (semi-pinned). See consolidated design §6.4 |
| Per-source scoring formulas | CLOSED | 6 formulas: GraphNode, Annotation, NodeHistory, SessionActivity, DocChunk, BehavioralSignal — all → [0,1] |
| Recency decay | CLOSED | e^(−0.01 × hours), no recency on GraphNodes (structural truth) |
| Token budget | CLOSED | 8k default, 10% headroom margin. Add CLI warning for budgets >30% of model context (T21) |
| Token counting method | CLOSED | len/4 approximation. tiktoken-rs upgrade path noted for v2 (T21, T22) |
| What gets cut when over budget | CLOSED | Greedy fill: sort by score DESC, skip if doesn't fit, continue past misses |
| Safety net (budget exhausted) | CLOSED | Emit pinned only + `// budget exhausted — increase with --budget` message |
| Body inclusion rule | CLOSED | Target body appended as [BODY] if leftover budget > 200 tokens post-fill |
| Pre-computed skeleton nodes | CLOSED | Pre-compute at index time, store in `skeleton` column on nodes table. See consolidated design §4.1 |
| Output render format | CLOSED | [!] signals → [TARGET] → [CALLERS] → [CALLEES] → [CONTEXT] → [DOCUMENTATION] → [BODY] |

---

## MCP Server & Daemon Lifecycle

| Component | Status | Notes |
|-----------|--------|-------|
| MCP tool definitions (v1: 5 tools) | CLOSED | `get_capsule`, `read_annotations` (with `session_summary`), `write_annotation` (upsert), `delete_annotation`, `search_docs`. See consolidated design §9.1 |
| `search_docs` MCP tool | CLOSED | FTS5 search over `doc_chunks_fts`. Fans out to federated repos. See consolidated design §9.1 |
| `tags TEXT` column on `annotations` | CLOSED | Added to `annotations` table and `annotations_fts` FTS5 virtual table. See consolidated design §3.5 |
| Transport (UDS + MCP bridge) | CLOSED | Daemon listens on UDS only. Per-session MCP bridge shim translates stdio ↔ UDS. Supports multiple concurrent Claude sessions. See consolidated design §2.1 |
| SQLite configuration | CLOSED | WAL mode, `synchronous=NORMAL`, `busy_timeout=5000`, `cache_size=-64000`, `mmap_size=268435456`. See consolidated design §3.5, §3.6 |
| Concurrent access (multiple Claude instances) | CLOSED | SQLite: 1 writer + N readers via `tokio-rusqlite`. Graph: `Arc<parking_lot::RwLock<GraphState>>` with split-phase locking. See consolidated design §3.7, §11.6 |
| Daemon startup sequence | CLOSED | flock → PID file → open daemon_meta.db → detect branch → open per-branch index DB → **start UDS listener early (degraded mode)** → dirty-flag check → freshness scan → load petgraph + recompute PageRank → watcher → ready. See consolidated design §11.1 |
| Daemon shutdown / signal handling | CLOSED | SIGTERM/SIGINT via tokio::signal, drain requests, flush writes, set clean flag. See consolidated design §11.2 |
| Crash recovery | CLOSED | SQLite WAL guarantees consistency. Dirty-flag triggers freshness scan on restart. See consolidated design §11.3 |
| Daemon socket / PID file | CLOSED | Unix domain socket at `.scavenger/daemon.sock`, PID file, flock for exclusion. See consolidated design §11.1 |
| File watcher batching | CLOSED | `notify-debouncer-full` 300ms trailing-edge, VCS-aware deferral on `.git/index.lock`. Branch-switch detection after VCS deferral. See consolidated design §11.4 |
| Index-per-branch architecture | CLOSED | Per-branch SQLite DB under `.scavenger/indexes/`. Warm switch = DB swap + graph reload + freshness check (with `reindex_state='switching'` gate). Cold start = copy parent DB + re-index diff files. `daemon_meta.db` shared. See consolidated design §8.1, §8.2, §8.3 |
| Merge commit detection | CLOSED | After VCS batch, check `git log -1 --format=%P HEAD` for 2+ parents. Identify source branch, trigger annotation union-merge. FF/squash limitation → manual `merge-annotations` CLI fallback. See consolidated design §8.5 |

---

## Hooks Integration

| Component | Status | Notes |
|-----------|--------|-------|
| PreToolUse hook (Read → capsule) | CLOSED | JSON stdin → UDS to daemon → capsule → `additionalContext` response. See consolidated design §10.1 |
| PostToolUse hook (Write/Edit → re-index) | CLOSED | JSON stdin → UDS to daemon → enqueue re-index → immediate exit 0. See consolidated design §10.2 |
| Hook failure modes | CLOSED | Fail open (exit 0, empty response). Log to `.scavenger/hook-errors.log`. See consolidated design §10.4 |
| Hook performance budget | CLOSED | Target <50ms total PreToolUse latency. Partial capsule fallback at 100ms. See consolidated design §10.5 |
| Batching rapid edits | CLOSED | Subsumed by file watcher debounce (300ms). Hooks feed same queue. See consolidated design §10.6 |
| Hook-to-daemon communication | CLOSED | Unix domain socket primary, CLI subcommand fallback. See consolidated design §10.3 |

Tool discoverability: no CLAUDE.md injection. Rely on MCP tool descriptions. Session summary is voluntary, not auto-injected. See consolidated design §12.5.

---

## CLI Interface

| Component | Status | Notes |
|-----------|--------|-------|
| Command surface | CLOSED | init, daemon, index, capsule, memory, graph stats/show, annotate, merge-annotations, doctor, stats, federate (add/remove/list/verify) |
| `scavenger init` behavior | CLOSED | Create .scavenger/ → initial index (bulk, bypasses split-phase) → register hooks → start daemon. See consolidated design §12.2 |
| `scavenger doctor` checks | CLOSED | 5 categories, trait-based registry, Flutter-style output, `--format=json`, exit codes 0/1/2. See consolidated design §12.3 |

---

## Language Support (tree-sitter grammars)

| Component | Status | Notes |
|-----------|--------|-------|
| v1 language targets | CLOSED | 15 languages: TypeScript, JavaScript, TSX/JSX, Python, Go, Rust, Java, C#, C, C++, Ruby, Bash, Kotlin, PHP, Swift |
| Grammar dependency strategy | CLOSED | Crate dependencies via crates.io. WASM-based dynamic loading noted for v2 extensibility |
| Symbol extraction per language | CLOSED | `tags.scm` queries, node-type mapping defined per language, signature via body exclusion. See consolidated design §4.1 |
| Cross-language edge resolution | CLOSED | Heuristic-only for v1: import path analysis, name matching, FFI detection. Confidence levels on edges. See consolidated design §4.8 |

---

## Token Analytics

| Component | Status | Notes |
|---|---|---|
| `token_log` table | CLOSED | Per-session, per-call: `tool_name, query, intent, tokens_actual, tokens_estimated, files_touched, branch`. In `daemon_meta.db` (shared, not per-branch). See consolidated design §3.6 |
| "Without index" estimator (`graph/estimator.rs`) | CLOSED | Per-tool heuristics using `files.raw_token_estimate`. Non-blocking, fail-silent. See consolidated design §6.8 |
| `scavenger stats` CLI command | CLOSED | Per-session and all-time token savings report. Configurable price/million in `.scavenger.toml [analytics]`. See consolidated design §12.4 |
| v2 HTTP dashboard | OPEN | Token analytics HTTP endpoint (default port 7842). Not v1 scope. Data infrastructure ready in `token_log`. |

---

## Federation (Cross-Repo)

| Component | Status | Notes |
|---|---|---|
| Federation configuration (`.scavenger.toml [federation]`) | CLOSED | `repos = ["/path/to/other"]`. Project-local config. See consolidated design §7 |
| `federated_repos` table in `daemon_meta.db` | CLOSED | `path, added_at, last_seen`. Shared across branches. Validates `.scavenger/` presence on add. See consolidated design §7 |
| Active index resolution for federated repos | CLOSED | Reads `current_branch` from federated repo's `daemon_meta.db`, opens that branch's index DB read-only at query time. See consolidated design §7 |
| `get_capsule` federation fallback | CLOSED | When local GATHER returns empty, searches federated repos' `nodes_fts`. Returns minimal capsule with `[FEDERATED]` marker. INDEX_BLIND_SPOT fires only after both local and federated lookups fail. See consolidated design §9.1 |
| `search_docs` fan-out | CLOSED | Fans out to federated repos' `doc_chunks_fts` at query time. Same read-only connection pattern as `get_capsule` fallback. See consolidated design §9.1 |
| `scavenger federate` CLI subcommands | CLOSED | `add, remove, list, verify`. Validates `.scavenger/` presence before adding. `list` shows branch + freshness. See consolidated design §7 |
| Federation check in `scavenger doctor` | CLOSED | Verifies accessibility, active branch DB existence, index freshness (>24h = warning). See consolidated design §7 |
| v2 cross-repo edges | OPEN | `ExternalImport` edges across repo boundaries. Requires coordinated NodeId resolution. Not v1 scope. |

---

## Summary

| Status | Count |
|--------|-------|
| CLOSED | 84 |
| OPEN | 2 (v2 HTTP dashboard, v2 cross-repo edges — both explicitly deferred) |

**All v1 architectural components are closed. Implementation can begin.**

Full specifications for every component are in **`docs/plans/2026-02-28-consolidated-design.md`**.
