# Scavenger — Consolidated Design Document

**Date**: 2026-02-28
**Status**: Authoritative — single source of truth for all design decisions
**Consolidates**: Original design, brainstorm decisions, capsule assembly design, research audit, PindeX integration

---

## Table of Contents

1. [Overview](#1-overview)
2. [Architecture](#2-architecture)
3. [Data Model & Storage](#3-data-model--storage)
4. [Indexing Pipeline](#4-indexing-pipeline)
5. [Query Engine](#5-query-engine)
6. [Capsule Assembly](#6-capsule-assembly)
7. [Memory Architecture](#7-memory-architecture)
8. [Branch Handling](#8-branch-handling)
9. [MCP Server](#9-mcp-server)
10. [Hooks Integration](#10-hooks-integration)
11. [Daemon Lifecycle](#11-daemon-lifecycle)
12. [CLI Interface](#12-cli-interface)
13. [Configuration](#13-configuration)
14. [Project Structure](#14-project-structure)
15. [Dependencies](#15-dependencies)
16. [v2 Backlog](#16-v2-backlog)
17. [Component Status](#17-component-status)

---

## 1. Overview

### 1.1 Problem Statement

Claude Code suffers two compounding problems on growing codebases:

1. **Token waste** — Claude reads entire files to understand a single function, dumping hundreds of irrelevant lines into context.
2. **Session amnesia** — Observations, patterns, and decisions made in one session are lost in the next. CLAUDE.md is manually maintained and goes stale.

Scavenger is a terminal-first, open-source alternative to closed-source tools like vexp. It runs as a persistent daemon alongside Claude Code, serving focused "capsules" instead of full files and persisting session memory anchored to code symbols.

### 1.2 Goals

- Reduce token usage by sending focused capsules instead of full files
- Persist session memory anchored to code nodes, not file paths
- Work entirely locally — no embeddings, no external APIs, no cloud calls
- Be language-agnostic via tree-sitter grammars (15 languages at launch)
- Integrate natively with Claude Code CLI via MCP + hooks
- Ship as a single Rust binary with no runtime dependencies
- Provide token savings analytics to demonstrate value
- Support cross-repo search via federation

### 1.3 Non-Goals

- VS Code / IDE extension (out of scope for v1)
- LLM-based summarization or embedding generation
- Support for non-Claude Code agents (may come later)
- Cloud sync of memory across machines

---

## 2. Architecture

### 2.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────┐
│                    Claude Code                       │
│                                                      │
│  ┌──────────────────┐      ┌──────────────────────┐  │
│  │  MCP Bridge       │      │   Hooks (pre/post)   │  │
│  │  (stdio ↔ UDS     │      │   (Read → capsule,   │  │
│  │   per session)    │      │    Write → re-index)  │  │
│  └────────┬─────────┘      └──────────┬───────────┘  │
└───────────┼──────────────────────────┼───────────────┘
            │ UDS                      │ UDS
            ▼                          ▼
┌─────────────────────────────────────────────────────┐
│                  scavenger daemon                    │
│              (single background process)             │
│                                                      │
│  ┌──────────────────────────────────┐  ┌──────────┐ │
│  │  UDS Listener (daemon.sock)      │  │  File    │ │
│  │  handles MCP bridge + hook reqs  │  │  Watcher │ │
│  └──────────────┬───────────────────┘  └────┬─────┘ │
│                 │                           │       │
│  ┌──────────────▼───────────────────────────▼─────┐ │
│  │            ReindexCoordinator                   │ │
│  │  (branch detection, DB swap, cold start)        │ │
│  └──────────────────────┬─────────────────────────┘ │
│                         │                            │
│  ┌──────────────────────▼──────────────────────────┐ │
│  │     Core Graph (petgraph::StableGraph)           │ │
│  │     + reverse index (in-memory HashMap)          │ │
│  │     Arc<parking_lot::RwLock<GraphState>>          │ │
│  └──────────────────────┬──────────────────────────┘ │
│                         │                            │
│  ┌──────────────────────▼──────────────────────────┐ │
│  │  Per-branch SQLite DB (WAL, FTS5, 1W/NR)        │ │
│  │  + daemon_meta.db (shared)                       │ │
│  └─────────────────────────────────────────────────┘ │
│                                                      │
│  ┌─────────────────────────────────────────────────┐ │
│  │  Federation (read-only connections to            │ │
│  │  other repos' .scavenger/ indexes)               │ │
│  └─────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────┘
```

One daemon per project, running as a background process. It indexes on first run, watches for file changes via the `notify` crate, and incrementally re-indexes dirty files. Each git branch gets its own SQLite database under `.scavenger/indexes/`.

**MCP Bridge architecture**: the daemon is NOT the MCP server. Instead, it listens solely on a Unix domain socket (`daemon.sock`). Each Claude Code session spawns a thin MCP bridge shim (`scavenger mcp-bridge`) as its MCP server process. The bridge translates stdio MCP JSON-RPC to UDS daemon requests and back. This allows multiple concurrent Claude Code sessions per project — each session has its own bridge, all connecting to the single daemon. Hooks also connect to the same UDS listener.

### 2.2 Storage Layout

```
.scavenger/
  daemon.lock              # flock for single-instance
  daemon.pid               # daemon process ID
  daemon.sock              # Unix domain socket for hook communication
  daemon_meta.db           # current_branch, reindex_state, last_shutdown, federated_repos, token_log
  hook-errors.log          # hook failure log for scavenger doctor
  indexes/
    main.db                # per-branch: nodes, edges, FTS5, node_versions,
    feature-x.db           #   behavioral_signals, annotations, session_log,
    dev.db                 #   doc_chunks, files — all self-contained
```

Git worktrees: each worktree gets its own `.scavenger/` directory — completely independent daemon and indexes.

---

## 3. Data Model & Storage

### 3.1 Graph Model

The graph is built from tree-sitter parse output. Every meaningful symbol becomes a node; every relationship becomes a directed edge. The in-memory graph uses `petgraph::StableGraph` for stable indices across dynamic node addition/removal.

### 3.2 Node Types

```
Function | Method | Class | Interface | Type | Enum | ExportedVar | Module | File
```

### 3.3 Edge Types

```
Imports | Calls | TypeRef | Extends | Implements | Exports | Contains
```

Cross-language edges carry a confidence level: `precise`, `heuristic`, or `speculative`.

### 3.4 NodeId Scheme

```
NodeId = hash(file_path, symbol_name, signature)
```

Full signature hash gives perfect overload disambiguation with zero language-specific logic. Identity breaks on signature edits are handled by the similarity heuristic (§4.7).

**Known limitation (v1)**: nested functions with identical names and identical signatures in the same file produce NodeId collisions (e.g., two `def helper(): pass` inside different outer functions). This affects languages that support nested named functions (Python, JS/TS, Rust, C#, Kotlin, Swift) but not those that don't (Go, Java, C, C++). Real-world occurrence is rare. v2 fix: include parent scope in hash: `hash(file_path, parent_name + "." + symbol_name, signature)`.

### 3.5 Per-Branch Index DB Schema

All tables below live in each per-branch SQLite database (`.scavenger/indexes/<branch>.db`).

#### SQLite Configuration (applied at connection open)

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA cache_size = -64000;       -- 64 MB page cache
PRAGMA mmap_size = 268435456;     -- 256 MB mmap for reads
PRAGMA auto_vacuum = INCREMENTAL;
```

**Schema migration**: set `PRAGMA user_version = 1` on initial schema creation. On every connection open, read `user_version` and run sequential migration functions for each version step. This enables non-destructive upgrades between Scavenger versions. Each migration function runs inside a transaction — partial migrations cannot corrupt the schema.

**Schema downgrade guard**: if `user_version > KNOWN_MAX_VERSION`, refuse to open the DB and emit: "This index was created by a newer version of Scavenger (schema v{N}). Please upgrade or delete .scavenger/ to rebuild." Do not attempt to read incompatible schemas.

#### `nodes` table

```sql
CREATE TABLE nodes (
    _rowid              INTEGER PRIMARY KEY,
    id                  TEXT UNIQUE NOT NULL,  -- NodeId hash
    kind                TEXT NOT NULL,         -- Function, Method, Class, etc.
    name                TEXT NOT NULL,
    file_path           TEXT NOT NULL,
    line_start          INTEGER NOT NULL,
    line_end            INTEGER NOT NULL,
    signature           TEXT NOT NULL,
    signature_hash      TEXT NOT NULL,     -- MD5[0..8] of whitespace-normalized signature
    docstring           TEXT,
    skeleton            TEXT NOT NULL,     -- pre-rendered signature + docstring, ready to emit
    centrality          REAL DEFAULT 0.0,  -- PageRank score (primary source is in-memory graph; persisted during idle checkpoint)
    checksum            BLOB NOT NULL      -- body content hash for staleness detection
);
```

File-level token estimates live in the `files` table (§3.5 below) — join via `file_path` when needed.

#### `nodes_fts` virtual table

```sql
CREATE VIRTUAL TABLE nodes_fts USING fts5(
    name, signature, docstring,
    content=nodes,
    content_rowid=_rowid
);

CREATE TRIGGER nodes_ai AFTER INSERT ON nodes BEGIN
    INSERT INTO nodes_fts(rowid, name, signature, docstring)
    VALUES (new._rowid, new.name, new.signature, new.docstring);
END;
CREATE TRIGGER nodes_ad AFTER DELETE ON nodes BEGIN
    INSERT INTO nodes_fts(nodes_fts, rowid, name, signature, docstring)
    VALUES ('delete', old._rowid, old.name, old.signature, old.docstring);
END;
CREATE TRIGGER nodes_au AFTER UPDATE ON nodes BEGIN
    INSERT INTO nodes_fts(nodes_fts, rowid, name, signature, docstring)
    VALUES ('delete', old._rowid, old.name, old.signature, old.docstring);
    INSERT INTO nodes_fts(rowid, name, signature, docstring)
    VALUES (new._rowid, new.name, new.signature, new.docstring);
END;
```

**Index-time token splitting**: before inserting into FTS5, split camelCase/snake_case identifiers (via `heck` crate) and store both original and split forms. `getUserById` is indexed as `getUserById get User By Id`, enabling queries for "user" to match compound identifiers.

#### `edges` table

```sql
CREATE TABLE edges (
    from_id    TEXT NOT NULL,
    to_id      TEXT NOT NULL,
    kind       TEXT NOT NULL,      -- Imports, Calls, TypeRef, Extends, Implements, Exports, Contains
    weight     REAL DEFAULT 1.0,
    confidence TEXT DEFAULT 'precise',  -- precise, heuristic, speculative
    PRIMARY KEY (from_id, to_id, kind)
);

CREATE INDEX idx_edges_to ON edges(to_id);
```

#### `files` table

Tracks all indexed files — both code and documentation. Populated by the code indexer and the doc indexer. Used by the token estimator (§6.8) to calculate "without index" savings.

```sql
CREATE TABLE files (
    id                  INTEGER PRIMARY KEY,
    file_path           TEXT UNIQUE NOT NULL,
    file_type           TEXT NOT NULL,       -- 'code' or 'doc'
    raw_token_estimate  INTEGER NOT NULL,   -- len/4 of full file content
    last_indexed        INTEGER NOT NULL    -- Unix epoch seconds
);
```

#### `node_versions` table (Layer 1 memory)

```sql
CREATE TABLE node_versions (
    id              INTEGER PRIMARY KEY,
    symbol_hash     TEXT NOT NULL,       -- NodeId of the symbol
    version_num     INTEGER NOT NULL,
    file_path       TEXT NOT NULL,
    session_id      TEXT,
    node_kind       TEXT NOT NULL,
    signature       TEXT NOT NULL,
    signature_hash  TEXT NOT NULL,       -- MD5[0..8] for fast diff
    edges_json      TEXT NOT NULL,       -- JSON array of (edge_kind, target_id) tuples
    body_hash       BLOB,
    created_at      INTEGER NOT NULL,
    UNIQUE(symbol_hash, version_num)
);

CREATE INDEX idx_versions_lookup ON node_versions(symbol_hash, version_num DESC);
```

Retention: last 5 versions per symbol. No time-based expiry.

#### `annotations` table (Layer 2 memory)

```sql
CREATE TABLE annotations (
    _rowid       INTEGER PRIMARY KEY,
    id           TEXT UNIQUE NOT NULL,
    anchor_type  TEXT,             -- 'node', 'file', 'scope', NULL for project-level
    anchor_value TEXT,             -- NodeId, file path, scope name, or NULL
    text         TEXT NOT NULL,
    tags         TEXT,             -- comma-separated keywords for FTS5 retrieval
    stale        BOOLEAN DEFAULT FALSE,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE INDEX idx_annotations_anchor ON annotations(anchor_type, anchor_value);

CREATE VIRTUAL TABLE annotations_fts USING fts5(
    text, tags,
    content=annotations,
    content_rowid=_rowid
);

CREATE TRIGGER annotations_ai AFTER INSERT ON annotations BEGIN
    INSERT INTO annotations_fts(rowid, text, tags)
    VALUES (new._rowid, new.text, new.tags);
END;
CREATE TRIGGER annotations_ad AFTER DELETE ON annotations BEGIN
    INSERT INTO annotations_fts(annotations_fts, rowid, text, tags)
    VALUES ('delete', old._rowid, old.text, old.tags);
END;
CREATE TRIGGER annotations_au AFTER UPDATE ON annotations BEGIN
    INSERT INTO annotations_fts(annotations_fts, rowid, text, tags)
    VALUES ('delete', old._rowid, old.text, old.tags);
    INSERT INTO annotations_fts(rowid, text, tags)
    VALUES (new._rowid, new.text, new.tags);
END;
```

Staleness by anchor type:
- `node` — flagged stale when the node's checksum changes; migrated via similarity heuristic if NodeId changes
- `file` — flagged stale when the file is modified
- `scope` — no automatic staleness; LLM-managed
- `NULL` (project-level) — no automatic staleness; LLM-managed

#### `behavioral_signals` table (Layer 3 memory)

```sql
CREATE TABLE behavioral_signals (
    id         INTEGER PRIMARY KEY,
    kind       TEXT NOT NULL CHECK(kind IN (
                   'THRASHING', 'DEAD_END', 'CYCLE_INTRODUCED',
                   'LARGE_BLAST_RADIUS', 'UNTESTED', 'INDEX_BLIND_SPOT',
                   'FAILED_SEARCH'
               )),
    node_id    TEXT,              -- NULL for INDEX_BLIND_SPOT and FAILED_SEARCH
    file_path  TEXT,              -- set for INDEX_BLIND_SPOT
    session_id TEXT NOT NULL,
    timestamp  INTEGER NOT NULL,
    detail     TEXT               -- query string, advice text, or error context
);

CREATE INDEX idx_signals_node ON behavioral_signals(node_id, timestamp DESC);
CREATE INDEX idx_signals_session ON behavioral_signals(session_id);
```

TTL: 48 hours or 2 sessions, whichever is longer.

#### `session_log` table (Layer 3 memory)

```sql
CREATE TABLE session_log (
    id         INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL,
    event_type TEXT NOT NULL,     -- 'read', 'query', 'edit'
    file_path  TEXT,
    symbol     TEXT,
    timestamp  INTEGER NOT NULL
);

CREATE INDEX idx_session_log ON session_log(session_id, timestamp DESC);
```

TTL: 48 hours or 2 sessions, matching behavioral signals.

#### `doc_chunks` table (document indexing)

```sql
CREATE TABLE doc_chunks (
    id             INTEGER PRIMARY KEY,
    file_path      TEXT NOT NULL,
    chunk_index    INTEGER NOT NULL,
    heading        TEXT,              -- nearest heading above chunk
    start_line     INTEGER NOT NULL,
    end_line       INTEGER NOT NULL,
    content        TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,  -- len/4
    last_indexed   INTEGER NOT NULL,
    content_hash   TEXT NOT NULL,     -- MD5[0..8] for incremental update
    UNIQUE(file_path, chunk_index)
);

CREATE VIRTUAL TABLE doc_chunks_fts USING fts5(
    content, heading,
    content=doc_chunks,
    content_rowid=id
);

CREATE TRIGGER doc_chunks_ai AFTER INSERT ON doc_chunks BEGIN
    INSERT INTO doc_chunks_fts(rowid, content, heading)
    VALUES (new.id, new.content, new.heading);
END;
CREATE TRIGGER doc_chunks_ad AFTER DELETE ON doc_chunks BEGIN
    INSERT INTO doc_chunks_fts(doc_chunks_fts, rowid, content, heading)
    VALUES ('delete', old.id, old.content, old.heading);
END;
CREATE TRIGGER doc_chunks_au AFTER UPDATE ON doc_chunks BEGIN
    INSERT INTO doc_chunks_fts(doc_chunks_fts, rowid, content, heading)
    VALUES ('delete', old.id, old.content, old.heading);
    INSERT INTO doc_chunks_fts(rowid, content, heading)
    VALUES (new.id, new.content, new.heading);
END;
```

### 3.6 `daemon_meta.db` Schema (shared across branches)

`daemon_meta.db` holds project-level state that is NOT branch-specific.

```sql
CREATE TABLE daemon_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- Rows: 'current_branch', 'reindex_state' (ready|switching|cold_start), 'last_shutdown' (clean|dirty)

CREATE TABLE federated_repos (
    id         INTEGER PRIMARY KEY,
    path       TEXT UNIQUE NOT NULL,
    added_at   INTEGER NOT NULL,
    last_seen  INTEGER
);

CREATE TABLE token_log (
    id                  INTEGER PRIMARY KEY,
    timestamp           INTEGER NOT NULL,
    session_id          TEXT NOT NULL,
    branch              TEXT NOT NULL,     -- branch active when this call was made
    tool_name           TEXT NOT NULL,
    query               TEXT,
    intent              TEXT,
    tokens_actual       INTEGER NOT NULL,
    tokens_estimated    INTEGER NOT NULL,
    files_touched       TEXT              -- JSON array of file paths
);

CREATE INDEX idx_token_log_session ON token_log(session_id, timestamp);
CREATE INDEX idx_token_log_branch ON token_log(branch);
```

`token_log` lives in `daemon_meta.db` (not in per-branch index DBs) because token analytics measure Scavenger's value, not branch-specific data. Branch cleanup (§8.6) must not lose analytics history. A `branch` column enables per-branch filtering in `scavenger stats` when needed.

### 3.7 In-Memory Structures

**Core graph**: `petgraph::StableGraph<NodeWeight, EdgeWeight, Directed>` loaded from `nodes` + `edges` tables on startup. Reloaded on branch switch.

**Reverse index**: `HashMap<NodeId, Vec<PathBuf>>` mapping each target NodeId to the source files that have edges pointing to it. Rebuilt from the `edges` table joined with `nodes` on startup:

```sql
SELECT e.to_id, n.file_path FROM edges e JOIN nodes n ON e.from_id = n.id;
```

At Scavenger's scale (20k–100k edges), this query runs in 10–50ms with warm OS page cache. On cold cache (first boot after reboot), expect 200–1000ms as SQLite pages fault in from disk. The startup sequence already serves degraded responses during this window (§11.1 step 6). The HashMap is updated in-place during re-indexing when edges are rebuilt. No separate persistence needed — the `edges` table is the persistence layer.

**Concurrency control**: the graph and reverse index are bundled into a `GraphState` struct, wrapped in `Arc<parking_lot::RwLock<GraphState>>`. Capsule assembly acquires a read lock (multiple readers in parallel). Re-indexing uses a split-phase approach to minimize write lock duration:

- **Phase 1 — Prep (no lock)**: re-parse file with tree-sitter, build new node/edge sets in temporary structures, run similarity heuristic. Pure computation on local data.
- **Phase 2 — Swap (brief write lock, ~5–15ms)**: acquire write lock, then within the lock: (1) commit SQLite transaction (DELETE old nodes/edges, INSERT new ones, FTS5 triggers fire), (2) update in-memory graph (remove old, insert new), (3) update reverse index, (4) release lock. **SQLite is updated before the in-memory graph** — if the SQLite transaction fails (e.g., `SQLITE_FULL`), bail before touching the graph, leaving both consistent at the old state. If the graph update panics after SQLite succeeds, the graph is rebuilt from SQLite on restart (SQLite is the persistence layer, the graph is derived).
- **Phase 3 — Deferred PageRank (eventually consistent)**: compute PageRank on the graph (read lock or snapshot), then briefly write-lock to update in-memory centrality scores on node weights. Phase 3 runs **once per debounce batch**, not once per file within a batch — intermediate recomputes provide negligible quality improvement since BM25 dominates the scoring formula (60% BM25 vs 40% centrality). Capsule queries between Phase 2 and Phase 3 see correct graph structure with slightly stale centrality — acceptable trade-off.

**Initial index bypass**: during `scavenger init` (§12.2), the split-phase model is not used. No concurrent readers exist during initial indexing, so bulk-insert into SQLite and build the in-memory graph once at the end. Split-phase applies only to incremental re-indexing during normal daemon operation.

**MCP write contention**: MCP tools that write to the per-branch DB (`write_annotation`, `delete_annotation`) share the single SQLite writer connection with the indexer. During normal operation (single-file re-index), contention is bounded by one Phase 2 duration (~15ms). During bursty scenarios (freshness check after branch switch), per-file Phase 2 commits create natural gaps. Accepted for v1 — add a priority queue for MCP writes only if latency issues surface in practice.

**Centrality scores**: the primary source is the in-memory `StableGraph` node weights, NOT the `nodes.centrality` column in SQLite. BM25+centrality composition reads from memory. Centrality is persisted to SQLite during idle checkpoint (5 seconds after last query) for federation reads and startup seeding. On startup, PageRank is recomputed from the loaded graph (~30–60ms at 100k nodes).

**Anti-pattern deduplication**: `HashSet<(SignalType, String)>` per session, preventing duplicate signal emission. Cleared on daemon restart — this is acceptable; detectors may re-fire after restart, which is mildly redundant but not harmful.

**Session state**: current `session_id`, thrashing ring buffers, failed search counters.

**Pending orphans buffer**: `HashSet<OrphanData>` holding unmatched orphans from the previous re-index cycle. Fully replaced each cycle — see §4.7 for the migration flow.

---

## 4. Indexing Pipeline

### 4.1 Tree-sitter Symbol Extraction

Use tree-sitter `tags.scm` query files with the standard capture convention (`@definition.function`, `@definition.class`, `@name`, `@doc`). Grammar dependencies are crate dependencies via crates.io (not git submodules).

**Signature extraction**: capture the definition node, exclude the `body` child via `Node::child_by_field_name("body")`. Signature = source text from definition start to body start.

**Docstring extraction**: language-specific — Rust `///`, Python first `expression_statement(string)` in block, Java/C#/PHP/Kotlin/Swift `/** */` block comments, TypeScript/Go/C/C++ preceding `(comment)` nodes, Ruby `#` comments preceding definitions.

**Skeleton generation**: at index time, render `signature + docstring` into the `skeleton` TEXT column on the `nodes` table. Average ~300 bytes per skeleton. At 100k symbols, ~30 MB uncompressed — acceptable for v1.

**`signature_hash` computation**: first 8 hex characters of MD5 over the whitespace-normalized signature string. Stored alongside the full signature for fast equality checks in the re-index hot path.

**Parallelism**: `Parser` is NOT `Send` — create one per rayon thread via `par_iter()` with thread-local storage. Expect 6–7× speedup on 8 cores.

### 4.2 Language Support (15 languages)

| Language | Extensions | Key tree-sitter node types |
|----------|-----------|---------------------------|
| TypeScript | `.ts` | `function_declaration`, `method_definition`, `class_declaration`, `interface_declaration`, `type_alias_declaration`, `enum_declaration` |
| JavaScript | `.js`, `.mjs`, `.cjs` | Same as TypeScript (shared grammar) |
| TSX / JSX | `.tsx`, `.jsx` | Same as TypeScript + JSX element handling |
| Python | `.py` | `function_definition`, `class_definition`, `decorated_definition` |
| Go | `.go` | `function_declaration`, `method_declaration`, `type_spec(struct_type)`, `type_spec(interface_type)` |
| Rust | `.rs` | `function_item`, `struct_item`, `enum_item`, `trait_item`, `impl_item`, `type_item`, `mod_item` |
| Java | `.java` | `method_declaration`, `class_declaration`, `interface_declaration`, `enum_declaration` |
| C# | `.cs` | `method_declaration`, `class_declaration`, `interface_declaration`, `enum_declaration`, `struct_declaration` |
| C | `.c`, `.h` | `function_definition`, `struct_specifier`, `enum_specifier`, `type_definition` |
| C++ | `.cpp`, `.hpp`, `.cc` | Extends C with `class_specifier`, namespace-qualified `function_definition`, `template_declaration` |
| Ruby | `.rb` | `method`, `class`, `module`, `singleton_method` |
| Bash | `.sh` | `function_definition` |
| Kotlin | `.kt`, `.kts` | `function_declaration`, `class_declaration`, `object_declaration`, `interface_declaration` |
| PHP | `.php` | `function_definition`, `method_declaration`, `class_declaration`, `interface_declaration`, `trait_declaration` |
| Swift | `.swift` | `function_declaration`, `class_declaration`, `protocol_declaration`, `struct_declaration`, `enum_declaration` |

### 4.3 Document Indexing

`.md` and `.markdown` files are indexed into `doc_chunks`. YAML and TXT are deferred to v2.

**Chunking strategy**: split at `#`, `##`, `###` heading boundaries. Each section (from one heading to the next) is one chunk. Sections exceeding 200 lines are sub-split at 100-line boundaries. Empty chunks are dropped.

**Incremental indexing**: compare `content_hash` (MD5[0..8]) against the current file. If unchanged, skip. If changed, delete all `doc_chunks` rows for that file and re-chunk. Also upsert the `files` table entry with `file_type = 'doc'` and updated `raw_token_estimate` — the token estimator (§6.8) needs this to calculate "without index" savings for `search_docs`.

The file watcher routes `.md` file events to `doc_indexer.rs` instead of the code parser.

### 4.4 File Re-indexing Flow

When a file is edited (detected via file watcher or PostToolUse hook):

1. Collect all existing NodeIds for the changed file from the `nodes` table
2. Re-parse the entire file with tree-sitter (< 1ms for a 1000-line file)
3. Build new nodes and edges from the parse
4. Compute `signature_hash` for each new node
5. Compare old NodeId set vs new NodeId set:

| Case | Action |
|------|--------|
| NodeId in both old and new | Compare `signature_hash` (fast path). If changed: update `checksum`, rebuild edges, create `node_versions` snapshot, mark linked annotations stale. If unchanged: update edges only if body changed. |
| NodeId in old but not new | Orphaned — feed to similarity heuristic (§4.7) |
| NodeId in new but not old | Candidate — check against orphans |

6. Run similarity heuristic on orphans vs candidates
7. Migrate annotations for matched pairs
8. Generate pre-computed `skeleton` for each new/updated node
9. Rebuild edges: delete all edges originating from nodes in the changed file, rebuild from the new parse
10. Update the in-memory reverse index for affected edges
11. Queue affected cross-file nodes for async re-index (§4.8)
12. Recalculate PageRank in-memory (deferred Phase 3 per §3.7 concurrency model — eventually consistent)
13. Update the `files` table with new `raw_token_estimate` and `last_indexed`

### 4.5 Edge Rebuild Strategy

Delete all edges originating from nodes in the changed file, rebuild from the new parse. Edges are derived data. The reverse index (in-memory HashMap) is updated in-place as edges are removed and added.

### 4.6 Cross-File Edge Staleness

When a file is re-indexed and NodeIds change, edges from other files pointing to the old NodeIds become dangling.

**Mechanism**: Reverse index + async cascade queue + lazy fallback.

1. When NodeIds change, look up the reverse index to find affected source files
2. Queue affected files for re-indexing asynchronously in the background
3. If a capsule query hits a dangling edge before the queue is processed, do a lazy resolution: look up by name in the expected file

**WAL checkpoint**: schedule manual `PRAGMA wal_checkpoint(PASSIVE)` during idle periods (5 seconds after last query) to prevent unbounded WAL growth from background cascade re-indexing.

### 4.7 Identity Migration (Similarity Heuristic)

When a NodeId disappears and a new one appears, the heuristic scores candidate matches:

```
score = (name_similarity × 0.3)
      + (signature_similarity × 0.25)
      + (body_similarity × 0.25)
      + (edge_neighborhood_similarity × 0.15)
      + (file_proximity × 0.05)
```

- **Name similarity**: Jaro-Winkler distance (`strsim` crate)
- **Signature similarity**: compare parameter count, parameter names, return type as token sets
- **Body similarity**: hash-based comparison, or compare the set of called symbols within the body
- **Edge neighborhood**: Jaccard similarity on the set of connected NodeIds
- **File proximity**: same file > same directory > elsewhere

**Threshold**: score > 0.6 → match (migrate annotations + version history). Below threshold → archive (recoverable but not surfaced).

**Scope**: for single-file edits, candidates are new nodes in that file. For multi-file batches, widen to all new nodes across changed files. Never scan the whole project.

**Pending orphans buffer**: when orphans from a re-index cycle have no matching candidates, they are not archived immediately. Instead they are held in a `pending_orphans` set (§3.7) for one additional debounce cycle. On the next cycle:

1. Match new candidates against `pending_orphans` — migrate any matches
2. Archive anything still in `pending_orphans` — they've had their extra cycle, no match found. **Mark all annotations anchored to archived NodeIds as `stale = TRUE`** (`UPDATE annotations SET stale = TRUE, updated_at = ? WHERE anchor_type = 'node' AND anchor_value = ?`). This feeds them into the 30-day cleanup path (§7.3) and surfaces them with `[STALE ⚠]` in `read_annotations` results.
3. Replace `pending_orphans` with this cycle's unmatched orphans

This handles cross-file renames where the old symbol (file A, saved first) and new symbol (file B, saved 300ms+ later) fall into separate debounce windows. Each set of orphans gets exactly one extra cycle. The `pending_orphans` set is fully replaced each cycle — no timestamps or counters needed.

### 4.8 Cross-Language Edge Resolution

Heuristic-only for v1, with confidence levels on edges:

1. **Import path analysis** (highest confidence, language-specific): parse import statements to build cross-file dependency edges
2. **Name-based matching** (`heuristic` confidence): match function names across languages with case transformation (`get_user` ↔ `getUser`)
3. **FFI boundary detection** (`speculative` confidence): recognize `#[pyfunction]`/`extern "C"`/HTTP route patterns

Unresolved callees get **phantom nodes** (placeholder in graph, flagged in capsule output as `[UNRESOLVED]`).

User-configurable boundaries in `.scavenger.toml`:

```toml
[[boundaries]]
from = "frontend/src/**/*.ts"
to = "backend/src/**/*.py"
pattern = "api"
```

### 4.9 Centrality Recalculation

Full PageRank recompute via `petgraph::algo::page_rank(&g, 0.85, 30)`, triggered **once per debounce batch** after all files in the batch complete Phase 2 (see §3.7). Code graphs converge in 10–30 iterations (T3); 30 iterations is sufficient. At 100k nodes with 500k edges: ~30–60ms. Increase only with benchmarking evidence.

Centrality scores are updated in-memory on the `StableGraph` node weights (Phase 3 of the split-phase concurrency model, §3.7). Persisted to `nodes.centrality` in SQLite during idle checkpoint — not on the hot path. See §3.7 for details.

---

## 5. Query Engine

### 5.1 Intent Detection

Hybrid classifier: keyword priority list → fuzzy match (`strsim` crate) → BM25 fallback → default to `Understand`.

| Intent | Keywords |
|--------|----------|
| Debug | `error`, `bug`, `fix`, `crash`, `failing`, `broken`, `traceback`, `panic`, `why is` |
| Refactor | `refactor`, `clean up`, `simplify`, `extract`, `rename`, `restructure`, `move`, `split` |
| Understand | `explain`, `what does`, `how does`, `walk me through`, `overview`, `describe`, `where is` |
| Extend | `add`, `implement`, `create`, `new feature`, `integrate`, `build` |
| Review | `review`, `check`, `audit`, `inspect`, `validate` |

**Multi-intent handling**: score all intents, return top-1. If the top two scores are within 0.1, use a weighted union of their traversal strategies (60% primary, 40% secondary).

### 5.2 Strategy-to-Traversal Mapping

| Intent | Traversal | Hop Limit | Direction |
|--------|-----------|-----------|-----------|
| Debug | Reverse BFS (callers) | 3 hops up, 2 hops down | Primarily incoming |
| Refactor | Forward DFS (blast radius) | Transitive closure, budget cap 100 nodes | Outgoing (used-by) |
| Understand | Bidirectional BFS | 2 hops each direction | Both |
| Extend | BFS on sibling/implements edges | 1–2 hops | Lateral |
| Review | Bidirectional BFS | 2 hops all directions | Both |

**Explosion mitigation**:
- **Degree cap**: skip nodes with in-degree + out-degree > 50 (utility functions like `log()`, `unwrap()`, `toString()`)
- **Node budget**: hard cap at 100 collected nodes per traversal
- **Builtins blocklist**: configurable in `.scavenger.toml`, starts empty

Implementation: `petgraph::visit::Bfs` with manual depth tracking via `HashMap<NodeIndex, usize>`.

### 5.3 FTS5 BM25 Integration

```sql
SELECT n.id, n.name, n.signature, bm25(nodes_fts) AS bm25_score
FROM nodes_fts
JOIN nodes n ON nodes_fts.rowid = n.rowid
WHERE nodes_fts MATCH ?
ORDER BY bm25(nodes_fts)
LIMIT 50;
```

Application-level score composition: `final_score = 0.6 × normalize(bm25_score) + 0.4 × normalize(centrality)`.

**BM25 normalization**: FTS5 `bm25()` returns negative values (more negative = better match). The `normalize()` function must sign-flip and rescale: `normalize(bm25) = -bm25_score / max_observed_bm25_magnitude` (min-max normalization with sign inversion). Centrality is read from the in-memory graph (§3.7), not from `nodes.centrality` in SQLite.

**Token splitting**: FTS5 uses hard-coded BM25 parameters (k1=1.2, b=0.75) and its built-in tokenizers don't split compound identifiers. At index time, camelCase/snake_case identifiers are split via the `heck` crate and stored alongside the original in FTS5 (e.g., `getUserById` indexed as `getUserById get User By Id`). This enables queries for component words to match compound identifiers. If parameter tuning is needed later, `tantivy` is the upgrade path.

**BM25 validation**: k1=1.2, b=0.75 are tuned for natural language, not code (short documents, repetitive vocabulary). Run empirical tests early in implementation — index a real codebase, run 20+ representative queries, evaluate whether BM25 ranking matches expectations. If ranking quality is poor, evaluate `tantivy` for v1 scope rather than deferring to v2.

### 5.4 Scope Tags

Hybrid approach: path-prefix as primary mechanism, virtual scope nodes lazily materialized only when a scope is annotated.

When an annotation targets `Scope("auth")`, all nodes under `src/auth/` (or the configured mapping) are matched by path prefix at query time. If queried frequently, a virtual `Module` node is materialized in the graph with `Contains` edges.

Configuration in `.scavenger.toml`:

```toml
[scopes]
auth = "src/auth/"
api = "src/api/"
database = ["src/db/", "src/models/"]
```

If no explicit mapping exists, infer scope from directory structure.

---

## 6. Capsule Assembly

### 6.1 Core Insight

Memory is not a separate subsystem. All context sources — graph nodes, annotations, version history, behavioral signals, session activity, doc chunks — are inputs to the same problem: selecting and ranking information to fit a token budget. The pipeline treats all sources uniformly.

### 6.2 Data Model

```rust
struct ContextItem {
    content: String,       // pre-rendered text, ready to emit
    token_count: u32,      // estimated at item creation (len/4)
    score: f32,            // [0.0, 1.0] — per-source formula
    group: OutputGroup,
    pinned: bool,
}

enum OutputGroup {
    Pinned,         // target node + active behavioral signals
    Callers,        // nodes that call / depend-on the target
    Callees,        // nodes the target calls / depends-on
    Context,        // annotations, node history, session activity
    Documentation,  // doc chunks from indexed .md files
}
```

### 6.3 Pipeline Stages

```
1. GATHER  — collect candidate items from all sources in parallel
             (nodes via traversal, annotations via FTS5, doc_chunks via FTS5,
              node_versions by NodeId, session_log by recency, behavioral_signals by TTL,
              priority docs unconditionally)
2. SCORE   — apply per-source scoring formula → [0.0, 1.0] per item
3. PIN     — extract pinned items: target node, active behavioral signals,
             1-hop callers and callees (semi-pinned)
4. TRIM    — sort unpinned by score DESC, fill remaining budget greedily
5. GROUP   — assign survivors to output groups by relationship type
6. RENDER  — emit: Pinned → Callers → Callees → Context → Documentation → Body
```

Stages 1–4 are section-agnostic. Grouping is presentation-only.

### 6.4 Pinning Rules

Three categories of items are pinned or semi-pinned:

1. **Target node** — always present, deducted from budget first
2. **Active behavioral signals** — any `Event` within its TTL window for nodes in the traversal. Pinned because they are ephemeral (if they exist, they are relevant)
3. **1-hop structural guarantee** — the target's direct callers and callees (signatures only) are semi-pinned: included before the scored competition begins, deducted from budget alongside pinned items. Ensures every capsule provides structural orientation even when annotations dominate.

### 6.5 Per-Source Scoring Formulas

All formulas produce `[0.0, 1.0]`.

**Shared recency decay**:
```
recency_decay(t) = e^(−0.01 × hours_elapsed(t))
  1h  → 0.99
  24h → 0.79
  7d  → 0.19
  30d → ~0.0
```

**GraphNode** (no recency — structural truth, staleness handled by checksum):
```
score = 0.4 × normalize(centrality) + 0.6 × bm25(query, name + signature + docstring)
```

**Annotation**:
```
proximity =
    1.0  if Node(target_id)
    0.8  if File(target's file)
    0.7  if Node(1-hop neighbor of target)
    0.6  if Scope(tag matching target's path)
    0.5  if File(neighbor's file)
    0.3  if None (project-level)

score = (0.5 × bm25(query, text + tags) + 0.3 × proximity + 0.2 × recency_decay(updated_at))
      × (0.6 if stale else 1.0)
```

**NodeHistory** (ordinal decay, not time-based):
```
significance =
    1.0  signature change
    0.7  edge change (new/removed calls)
    0.4  body change
    0.2  docstring change

version_distance = versions between entry and current (1 = most recent)

score = 0.6 × significance + 0.4 × (1.0 / version_distance)
```

**SessionActivity**:
```
score = 0.5 × recency_decay(timestamp) + 0.5 × jaccard(activity_nodes, traversal_nodes)
```

**DocChunk**:
```
score = 0.7 × normalize(bm25_doc) + (0.3 if priority_doc else 0.0)
```
Where `priority_doc` = true for files in `[docs].priority` config (`CLAUDE.md`, `README.md` by default).

Priority docs are **unconditionally included** in the GATHER stage regardless of BM25 match. This ensures `CLAUDE.md` and `README.md` always compete for budget — their 0.3 base score means they only lose to items with meaningful relevance, not to zero-scoring noise.

**BehavioralSignal**: pinned, score fixed at 1.0 (ordered by recency within Pinned group).

### 6.6 Token Budget

- **Default**: 8000 tokens (`--budget 8000`)
- **Headroom**: 10% margin applied at config time (`--budget 8000` → effective 7200). This is appropriate because the capsule (8k tokens) is a small fraction of the model's total context window (128k–200k). The research recommendation to "stay at 70–80% of context limit" (T21) applies to total context, not individual injected blocks.
- **Estimator**: `s.len() / 4` — conservative for code, no external tokenizer
- **Warning threshold**: configurable in `.scavenger.toml` (default 20000). If `--budget` exceeds this, emit: "Large budgets may reduce model precision."

**Greedy fill**:
```
remaining = effective_budget − sum(pinned.token_count) − sum(semi_pinned.token_count)

for item in unpinned_items, sorted by score DESC:
    if item.token_count <= remaining:
        include(item)
        remaining −= item.token_count
    // else: skip this item, continue — don't stop on first miss
```

**Body inclusion**: after greedy fill, if remaining > 200 tokens, append full body of the target node as `[BODY]`.

**Budget exhausted**: if no unpinned items fit, emit pinned only and append:
```
// budget exhausted — increase with --budget <tokens>
```

### 6.7 Output Format

Scores never appear in output.

```
[!] THRASHING: validateToken edited 3× in 5 min (reverting same change)
[!] UNTESTED: validateToken has no incoming edges from test files

[TARGET] validateToken  src/auth/middleware.ts:42
fn validateToken(token: &str, options: TokenOptions) -> Result<Claims, AuthError>
/// Validates a JWT token and returns the decoded claims.

[CALLERS]
• handleRequest  src/api/router.ts:18
  fn handleRequest(req: Request, res: Response) -> ()
• authMiddleware  src/middleware/auth.ts:7
  fn authMiddleware(ctx: Context) -> Result<(), Error>

[CALLEES]
• checkRevocationList  src/auth/revocation.ts:33
  fn checkRevocationList(token: &str) -> bool
• TokenOptions  src/auth/types.ts:12
  struct TokenOptions { expiry: Duration, issuer: String }

[CONTEXT]
• [NOTE] "Performance-critical — called on every request"  2h ago
• [STALE ⚠] "Uses simple expiry check"  last week — signature changed since
• [CHANGED] Added param: options: TokenOptions; new call: checkRevocationList  yesterday

[DOCUMENTATION]
• Authentication (CLAUDE.md §3)
  JWT expiry: access=1h, refresh=7d. Refresh tokens stored in Redis.

[BODY] validateToken
pub fn validateToken(...) { ...full body... }
```

**Rendering rules**:
- `[!]` signals appear before `[TARGET]` — always first
- `[STALE ⚠]` suffix on stale annotations — visible, not suppressed
- Section headers omitted if the group is empty
- `[BODY]` only if leftover budget > 200 tokens
- `[DOCUMENTATION]` appears after `[CONTEXT]`, before `[BODY]`

### 6.8 Token Estimator ("Without Index")

Implemented in `graph/estimator.rs`. For each tool call, estimates what Claude would have consumed without Scavenger, enabling the token savings report in `scavenger stats` (§12.1).

**Per-tool estimation logic**:

| Tool | Without-index estimate | Rationale |
|------|----------------------|-----------|
| `get_capsule` | `files.raw_token_estimate` for the seed file + all 1-hop neighbor files | Without the index, Claude reads full files |
| `search_docs` | Sum of `files.raw_token_estimate` for all matched doc files | Without the index, Claude reads full doc files |
| `read_annotations` | `files.raw_token_estimate` for the anchor's file, or 0 if project-level | Without memory, Claude re-reads the relevant file or has no access |
| `write_annotation` | 0 for creates; same as `read_annotations` for updates | Information would not exist without Scavenger |
| `delete_annotation` | 0 | Write-only tool |

**Implementation constraints**:
- Non-blocking and fail-silent — errors produce `tokens_estimated = 0` and are logged
- `files.raw_token_estimate` lookups use the per-branch index DB; `token_log` writes go to `daemon_meta.db`
- The estimator runs after capsule assembly, issuing one small batch query for `files.raw_token_estimate` values (`SELECT file_path, raw_token_estimate FROM files WHERE file_path IN (...)` for the seed file + neighbor files). This is a single indexed query (~0.5–1ms), non-blocking and fail-silent

**Logging**: after each tool call, insert one row into `token_log` (in `daemon_meta.db`) with `tokens_actual` (the capsule size or response size), `tokens_estimated` (the without-index estimate), and the active branch name.

---

## 7. Memory Architecture

### 7.1 Three-Layer Model

| Layer | Purpose | Storage | Lifecycle |
|-------|---------|---------|-----------|
| 1. Node Version History | Structural change tracking | `node_versions` table, last 5 snapshots per symbol | Bounded by depth, auto-managed |
| 2. Semantic Annotations | LLM-managed knowledge | `annotations` table, flexible anchoring | LLM creates/updates/deletes via MCP tools |
| 3. Behavioral Signals + Session Log | Ephemeral diagnostic events | `behavioral_signals` + `session_log` tables | TTL-pruned: 48h or 2 sessions |

### 7.2 Layer 1 — Node Version History

State-snapshot pattern. When a node's `checksum` changes during re-indexing, a new version row is inserted. Diff is computed at read time by comparing consecutive version rows on `signature`, `edges_json`, and `body_hash`.

Scoring uses ordinal decay (`1.0 / version_distance`), not time-based — the most recent change always scores highest regardless of when it happened.

### 7.3 Layer 2 — Semantic Annotations

The LLM owns its semantic memory. Scavenger provides infrastructure (storage, staleness notifications, identity migration), but the LLM decides what to remember, revise, and delete.

**Flexible anchoring**:

| Anchor | Example | Staleness |
|--------|---------|-----------|
| `Node(NodeId)` | "this function is performance-critical" | Notified on checksum change; migrated via similarity heuristic if NodeId changes |
| `File(path)` | "this file is auto-generated, don't edit" | Notified on file modification |
| `Scope(name)` | "auth", "api", "deployment" | No automatic staleness; LLM-managed |
| `None` | Project-level architectural decisions | No automatic staleness; LLM-managed |

**Branch behavior**: annotations are per-branch. On branch creation (cold start), they are copied from the parent. On merge commit detection, they are union-merged from the source branch (§8.4).

**Orphan cleanup**: annotations with `anchor_type = 'node'` where the `anchor_value` NodeId doesn't exist in the `nodes` table and `stale = TRUE` for >30 days are deleted. Runs as part of the same periodic cleanup that handles behavioral signals TTL pruning. Project-level (`None`) and scope annotations are unaffected — they don't anchor to NodeIds.

### 7.4 Layer 3 — Behavioral Signals & Session Log

**Session activity log**: captures what Claude explored (reads, queries) alongside what it changed (edits). At session start, `read_annotations` with `session_summary: true` surfaces: "Last session you explored: auth/middleware.ts, auth/types.ts, token validation flow."

Session log is per-branch — when you switch to `feature`, you see activity from your last session on `feature`. New branches start with an empty session log.

### 7.5 Anti-Pattern Detection

Seven detectors, all using the fire-once-at-N rule with a per-session deduplication `HashSet<(SignalType, key)>`.

| Pattern | Algorithm | Threshold | Dedup Key |
|---------|-----------|-----------|-----------|
| THRASHING | Ring buffer of `(NodeIndex, timestamp, content_hash)`. Levenshtein distance between consecutive edits: similarity >0.9 = thrashing, otherwise iterative progress. | ≥3 edits to same node in 5-min window, similarity >0.9 | `node_id` |
| DEAD_END | Monitor `graph.neighbors_directed(node, Incoming).count()` after symbol creation. Exclude test files and public API endpoints. | Zero incoming edges from non-test code after ≥10 session actions or 15 min | `node_id` |
| CYCLE_INTRODUCED | Before adding edge (u,v), check `petgraph::algo::has_path_connecting(&graph, v, u)`. | Any new cycle | `from_node::to_node` |
| LARGE_BLAST_RADIUS | Forward BFS counting reachable nodes. | >20 direct dependents OR >50 transitive | `node_id` |
| UNTESTED | Check if any test-file node (path pattern: `*_test.rs`, `tests/*.rs`, `test_*.py`, `*.test.ts`, `__tests__/`) has an edge to the target. | Zero test edges | `node_id` |
| INDEX_BLIND_SPOT | Emitted from GATHER stage when seed file exists on disk but has no indexed nodes — checked AFTER federation fallback (§9.1 Tool 1), so a symbol found in a federated repo does not trigger a false blind spot. | File exists, zero nodes locally AND in federated repos | `file_path` |
| FAILED_SEARCH | Per-session `HashMap<String, u32>` keyed by normalized query. Increment on every zero-result FTS5 query. | ≥3 zero-result queries for same string | `normalized_query` |

---

## 8. Branch Handling

### 8.1 Index-per-Branch Architecture

Each git branch gets its own self-contained SQLite database. The daemon holds one active index DB open at a time, with the in-memory petgraph loaded from that DB. No component other than the `ReindexCoordinator` needs branch-awareness.

### 8.2 Warm Switch (existing index)

When the file watcher detects a branch change and an index DB already exists for the new branch:

1. **Set `daemon_meta.reindex_state = 'switching'`** — all tool calls during `'switching'` return degraded responses, same as `'cold_start'`. This prevents serving inconsistent results while the DB and graph are being swapped.
2. Update `daemon_meta.current_branch`
3. Close current branch's DB connections
4. Open `.scavenger/indexes/<new_branch>.db`
5. Drop in-memory petgraph + reverse index, reload from new DB
6. Freshness check: compare `(path, mtime_ns, size)` against filesystem
7. Re-index stale files via normal pipeline
8. Set `daemon_meta.reindex_state = 'ready'`

### 8.3 Cold Start (first time on branch)

When no index DB exists for the new branch:

1. Set `daemon_meta.reindex_state = 'cold_start'`
2. Run `git diff --name-only <current_branch>..<new_branch>` for changed files (includes both code and doc files)
3. Copy `.scavenger/indexes/<current_branch>.db` → `.scavenger/indexes/<new_branch>.db`
4. Clear `node_versions`, `behavioral_signals`, and `session_log` in the copy (annotations are preserved — see §8.4)
5. Re-index only the changed files — route by extension: `.md`/`.markdown` → doc indexer (§4.3), code extensions → code indexer (§4.4). This ensures `doc_chunks` are correct for the new branch.
6. Recompute PageRank
7. Swap: close old DB, open new DB, load petgraph
8. Set `daemon_meta.reindex_state = 'ready'`

Typical timing: 50 changed files out of 5000 ≈ 50ms. Full rebuild fallback for branches with no common ancestor: ~2s for 5000 files with rayon parallelism.

### 8.4 Annotation Fork & Merge

**Fork**: on cold start (step 3 above), all annotations are copied along with the DB. Each branch's annotations evolve independently.

**Merge** (on detected merge commit):
- Same anchor + same text → skip (deduplicate)
- Same anchor + different text → keep both
- New annotation from source branch → copy if anchor's NodeId exists in current graph
- Annotation in target but missing in source → leave untouched

**Manual merge**: `scavenger merge-annotations <branch>` for fast-forward and squash merges.

### 8.5 Merge Commit Detection

After a VCS-deferred batch completes and the branch has NOT changed:

1. Run `git log -1 --format=%P HEAD` to get parent hashes
2. If 2+ parents → merge commit
3. Identify source branch: `git branch --points-at <second-parent-hash>`
4. If source branch has an index DB → trigger annotation union-merge (§8.4)
5. Proceed with normal file re-indexing

Fast-forward and squash merges are not detectable — `scavenger merge-annotations` serves as manual fallback.

### 8.6 Cleanup

Delete `.scavenger/indexes/<branch>.db` when the git branch is deleted. Detection: periodically (on daemon startup and hourly) compare existing index files against `git branch --list`.

### 8.7 Edge Cases

- **Rapid branch switching**: VCS deferral handles overlapping `.git/index.lock` sequences. Only the final branch state is indexed.
- **Detached HEAD**: use commit hash (first 12 chars) as filename: `indexes/HEAD_<hash>.db`.
- **Git worktrees**: each worktree gets its own `.scavenger/` — completely independent.

---

## 9. MCP Server

### 9.1 v1 Tool Definitions (5 tools)

**SDK**: `rmcp` (official Rust MCP SDK) with `#[tool]` proc macros and automatic JSON Schema generation via `schemars`. Requires Rust nightly (Edition 2024). Fallback: hand-rolled JSON-RPC over stdio if `rmcp` doesn't stabilize.

**Transport**: stdio (newline-delimited JSON-RPC). Stderr for logging only.

---

#### Tool 1: `get_capsule`

Primary entry point. Returns a focused context capsule for a symbol.

```rust
#[tool(description = "Get a focused context capsule for a code symbol.
Returns the symbol's signature, callers, callees, annotations, and relevant context
within a token budget. Use before reading any file to get graph-aware context.")]
async fn get_capsule(
    &self,
    #[arg(description = "File path relative to project root")]
    file: String,
    #[arg(description = "Symbol name. If omitted, returns capsule for the file's primary export.")]
    symbol: Option<String>,
    #[arg(description = "Your intent or question — drives context selection strategy.")]
    query: Option<String>,
) -> Result<CapsuleResult>
```

**Federation fallback**: when the local GATHER stage returns empty and federation is configured, `get_capsule` searches federated repos' `nodes_fts` for the symbol. If found, returns a minimal capsule from the federated repo with a `[FEDERATED: /path/to/repo]` marker.

**INDEX_BLIND_SPOT**: emitted only if the file exists on disk AND both local and federated lookups return zero results. This avoids false warnings when a symbol lives in a federated repo.

---

#### Tool 2: `read_annotations`

Retrieve annotations by anchor, tags, or session summary.

```rust
#[tool(description = "Retrieve annotations. Use at session start with session_summary=true
to get a summary of last session's activity, stale annotations, and active signals.
Per-node annotations are also surfaced automatically via get_capsule.")]
async fn read_annotations(
    &self,
    #[arg(description = "Filter by anchor type: 'node', 'file', 'scope'.")]
    anchor_type: Option<String>,
    #[arg(description = "Filter by anchor value.")]
    anchor_value: Option<String>,
    #[arg(description = "Filter by tags (comma-separated).")]
    tags: Option<String>,
    #[arg(description = "Full-text search query across annotation text and tags.")]
    query: Option<String>,
    #[arg(description = "If true, returns a session start summary: last session activity, stale annotations, active behavioral signals.")]
    session_summary: Option<bool>,
    #[arg(description = "Maximum results (default 10).")]
    limit: Option<u32>,
) -> Result<Vec<AnnotationResult>>
```

When `session_summary: true`:
- Surface session log entries from the last session (what was explored)
- Surface all stale annotations
- Surface all active behavioral signals within TTL
- Format as: "Last session you explored: auth/middleware.ts, auth/types.ts. Stale: validateToken annotation. Signals: UNTESTED on checkRevocationList."

**Design note — session summary is voluntary**: `session_summary` is opt-in, not auto-injected on first capsule. Auto-injection would bloat context on every session start regardless of relevance — the user may be starting fresh work unrelated to the previous session. This undermines Scavenger's token efficiency goal. The LLM judges whether session continuity is relevant based on the user's first prompt.

---

#### Tool 3: `write_annotation`

Create or update an annotation. Upsert semantics: if `id` is provided, updates the existing annotation; if omitted, creates a new one. Anchors to a symbol, file, scope, or project level via cascading resolution.

```rust
#[tool(description = "Persist a fact, decision, or note anchored to code. Creates a new
annotation or updates an existing one. Use for cross-session knowledge: architectural
decisions, discovered bugs, learned patterns. Anchor to a symbol, file, or scope for
precise future retrieval via get_capsule and read_annotations.")]
async fn write_annotation(
    &self,
    #[arg(description = "Annotation ID to update. Omit to create a new annotation.")]
    id: Option<String>,
    #[arg(description = "The annotation text. Be specific — retrieved via keyword search.")]
    text: String,
    #[arg(description = "Comma-separated keywords for retrieval (e.g. 'auth,jwt,redis').")]
    tags: Option<String>,
    #[arg(description = "Symbol name to anchor to. Resolved via search — use the most specific name.")]
    symbol: Option<String>,
    #[arg(description = "File path to anchor to if no symbol specified.")]
    file: Option<String>,
    #[arg(description = "Scope name to anchor to (e.g. 'auth', 'api'). See project config for defined scopes.")]
    scope: Option<String>,
) -> Result<AnnotationResult>
```

**Anchor resolution** (for creates and updates that change anchor):
1. If `symbol` provided → resolve via FTS5 name search (top-1). If resolved → `Node(node_id)`. If not → fall through.
2. If `file` provided → `File(path)`.
3. If `scope` provided → `Scope(name)`.
4. Otherwise → `None` (project-level).

**Disambiguation**: when multiple FTS5 results for `symbol` score within 20% of the top result, include a disambiguation note in the return value listing alternatives: `"note": "Multiple matches for 'validate'. Anchored to validate@src/auth/validators.rs. Other matches: validate@src/form/validators.rs."` This lets the caller retry with `file` for disambiguation.

**Return value** (`AnnotationResult`):
```json
{
  "id": "ann_abc123",
  "anchor": "Node(validateToken@src/auth/tokens.rs)",
  "created_at": "2026-02-28T14:32:00Z",
  "updated_at": "2026-02-28T14:32:00Z",
  "retrieval_hint": "Use read_annotations(query='jwt expiry') or get_capsule to retrieve in future sessions."
}
```

If symbol resolution fails, `anchor` shows the fallback: `"File(src/auth/tokens.rs)"` or `"Scope(auth)"` or `"None (project-level)"`.

Stores in `annotations` table with resolved anchor.

---

#### Tool 4: `delete_annotation`

```rust
#[tool(description = "Delete an annotation.")]
async fn delete_annotation(
    &self,
    #[arg(description = "Annotation ID to delete.")]
    id: String,
) -> Result<DeleteResult>
```

---

#### Tool 5: `search_docs`

Search indexed documentation files.

```rust
#[tool(description = "Search indexed documentation files (CLAUDE.md, README.md, docs/**/*.md).
Use to find design rationale, architecture decisions, or project conventions
without loading entire documentation files.")]
async fn search_docs(
    &self,
    #[arg(description = "Search query.")]
    query: String,
    #[arg(description = "Maximum results (default 5).")]
    limit: Option<u32>,
) -> Result<Vec<DocChunkResult>>
```

Fans out to federated repos' `doc_chunks_fts` when federation is configured.

---

### 9.2 Error Handling

- JSON-RPC `-32602` (Invalid params) for validation failures
- `CallToolResult { isError: true, content: [...] }` for tool-level errors (symbol not found, daemon not indexed)

---

## 10. Hooks Integration

### 10.1 PreToolUse Hook (Read → capsule injection)

Fires when Claude calls `Read`. Hook receives JSON on stdin with `session_id`, `tool_name`, `tool_input`. Sends file path to daemon via Unix domain socket, receives capsule response.

Returns JSON to stdout:

```json
{
  "additionalContext": "... capsule text ..."
}
```

The `additionalContext` field (Claude Code v2.1.9+) injects the capsule before the Read result without blocking or modifying the Read.

Exit code 0 always.

### 10.2 PostToolUse Hook (Write/Edit → re-index)

Fires when Claude calls `Write`, `Edit`, or `MultiEdit`. Sends `(session_id, file_path, event_type)` to daemon via Unix socket. Daemon enqueues file for debounced re-index. Hook returns immediately with exit code 0, empty JSON response.

The daemon re-parses the entire file with tree-sitter (< 1ms) — it does not need the diff from Claude Code.

### 10.3 Hook-to-Daemon Communication

Primary: Unix domain socket at `.scavenger/daemon.sock`. Sub-millisecond round-trip.

Fallback: `scavenger hook pre-tool-use` / `scavenger hook post-tool-use` CLI subcommands that read stdin, connect to the socket, forward the request, and print the response.

### 10.4 Failure Modes

If the daemon is unreachable (socket connection fails), hooks exit 0 with empty JSON response — **fail open, never block Claude Code**. Log failures to `.scavenger/hook-errors.log` for `scavenger doctor` to detect.

### 10.5 Performance Budget

Target: < 50ms total PreToolUse latency. Budget: Rust binary startup ~0.5ms + socket connect ~0.1ms + capsule assembly ~10–30ms + response serialization ~1ms + overhead ~10ms. If capsule assembly exceeds 100ms, return a partial capsule (pinned items only).

### 10.6 Batching

PostToolUse hooks notify the daemon, which enqueues the file into the same 300ms debounce pipeline as the file watcher. Multiple rapid writes to the same file produce one re-index, not five.

---

## 11. Daemon Lifecycle

### 11.1 Startup Sequence

1. Acquire exclusive `flock` on `.scavenger/daemon.lock`
2. Write PID to `.scavenger/daemon.pid`
3. Open `daemon_meta.db` with WAL PRAGMAs
4. Detect current branch via `git rev-parse --abbrev-ref HEAD`
5. Open per-branch index DB at `.scavenger/indexes/<branch>.db` — if it doesn't exist, cold-start (§8.3)
6. **Start Unix domain socket listener immediately** — accept requests during indexing with degraded responses (`{ "status": "indexing", "progress": "42%" }`)
7. Check `daemon_meta.last_shutdown` — if `dirty`, trigger full freshness scan
8. Set `last_shutdown = 'dirty'` immediately
9. Compare `(path, mtime_ns, size)` of indexed files against filesystem, re-index mismatches
10. Load in-memory petgraph + reverse index from active DB; recompute PageRank (30 iterations, ~30–60ms)
11. Start file watcher (`notify-debouncer-full`)
12. Set `daemon_meta.reindex_state = 'ready'` — full responses now available
13. Ready

**Error handling**: if `SQLITE_FULL` is encountered during re-index or write operations, log a clear error, emit a diagnostic detectable by `scavenger doctor`, and continue in degraded read-only mode (serve capsules from existing data, stop writes until disk space is freed).

### 11.2 Shutdown & Signal Handling

Handle `SIGTERM` and `SIGINT` via `tokio::signal::unix::signal(SignalKind::terminate())` in a `tokio::select!` loop:

1. Stop accepting new UDS requests
2. Drain in-flight requests (5-second timeout)
3. Flush pending index writes
4. Set `daemon_meta.last_shutdown = 'clean'`
5. Close SQLite connections
6. Remove PID file
7. Exit 0

### 11.3 Crash Recovery

No special crash recovery logic needed. SQLite WAL with `synchronous=NORMAL` guarantees database consistency after any crash. On restart, the dirty-flag check (step 6 above) triggers a freshness scan that catches any partially-indexed files. The filesystem is always the source of truth — the index is a rebuildable cache.

### 11.4 File Watcher

**Crate**: `notify-debouncer-full` (v0.5.0) with 300ms trailing-edge debounce. Paired with `ignore` crate for `.gitignore`-aware filtering.

**VCS-aware deferral**: detect `.git/index.lock` presence and pause event processing until released. Handles `git checkout`, `git rebase`, `git merge` without re-indexing storms.

**Branch-switch detection**: after a VCS-deferred batch completes, run `git rev-parse --abbrev-ref HEAD` and compare against `daemon_meta.current_branch`. If changed → index-per-branch swap (§8.2 or §8.3). If same → check for merge commit (§8.5).

**File routing**: `.md`/`.markdown` files → document indexer. Code files (by extension) → code indexer.

### 11.5 Session Tracking

A session represents one Claude Code conversation. The daemon needs a `session_id` to stamp `token_log`, `behavioral_signals`, and `session_log` rows.

**Acquisition**:
- **Hooks**: the Claude Code hook payload includes `session_id` in the stdin JSON (Claude Code v2.1.9+). The PostToolUse hook forwards it to the daemon via the Unix socket.
- **MCP tools**: the `rmcp` SDK exposes request metadata. If `session_id` is available in the MCP protocol context, use it. If not, the daemon generates a UUID at startup and uses it as the fallback session ID.
- **Session change detection**: when the daemon receives a `session_id` that differs from the current one, it updates `current_session_id` in memory. No explicit "session start" event — the first tool call or hook in a new session implicitly starts it.

**Lifecycle**: the session ID is held in daemon memory only. It is NOT persisted to `daemon_meta.db`. On daemon restart, a new fallback UUID is generated. If Claude Code reconnects with the same `session_id`, continuity is maintained.

### 11.6 Concurrent Access

SQLite WAL mode: 1 writer + N readers. One `tokio-rusqlite::Connection` for the writer (daemon indexer), N `tokio-rusqlite::Connection` instances for UDS read handlers. Manual WAL checkpoint during idle periods. In-memory graph concurrency via `Arc<parking_lot::RwLock<GraphState>>` — see §3.7 for the split-phase locking model.

---

## 12. CLI Interface

### 12.1 Command Surface

```
scavenger init                            # index project, register hooks, start daemon
scavenger daemon                          # start MCP server + file watcher
scavenger index [path]                    # manual re-index (useful after large merges)
scavenger capsule <file> [symbol] [--query "..."]   # print capsule to stdout
scavenger memory [--query "..."] [--limit 10]       # query annotations
scavenger graph stats                     # node/edge counts, centrality top-10
scavenger graph show <symbol>             # print neighborhood as ASCII tree
scavenger annotate <symbol> "<text>"      # manually add an annotation
scavenger merge-annotations <branch>      # merge annotations from <branch>
scavenger doctor [--verbose] [--format=json]        # verify health
scavenger stats [--session <id>]          # token savings report
scavenger federate add <path>             # add federation link
scavenger federate remove <path>          # remove federation link
scavenger federate list                   # list federated repos with status
scavenger federate verify                 # check all federated repos accessible
```

### 12.2 `scavenger init`

Five sequential steps:

1. Create `.scavenger/` directory (mode `0700`, owner-only) with `daemon_meta.db`, `indexes/`, config. Add `.scavenger/` to project `.gitignore` if not already present.
2. Run initial index — parse all source files + docs, bulk-insert into SQLite, build graph once at the end (bypasses split-phase model, see §3.7). Display progress indicator.
3. Register hooks in `.claude/settings.local.json` (gitignored, per-machine).
4. Register MCP bridge in `.claude/settings.local.json`.
5. Start daemon in background. Print: "Scavenger daemon started. Ready for Claude Code sessions."

```json
{
  "hooks": {
    "PreToolUse": [{
      "matcher": "Read",
      "hooks": [{ "type": "command", "command": "scavenger hook pre-tool-use" }]
    }],
    "PostToolUse": [{
      "matcher": "Edit|Write|MultiEdit",
      "hooks": [{ "type": "command", "command": "scavenger hook post-tool-use" }]
    }]
  }
}
```

JSON merge: read existing settings → deep-merge Scavenger entries without overwriting other hooks → write atomically (temp file + rename). Use `fs2::FileExt::lock_exclusive()` during write.

### 12.3 `scavenger doctor`

Trait-based check registry with five categories:

```rust
trait DiagnosticCheck {
    fn name(&self) -> &str;
    fn category(&self) -> Category;
    fn run(&self, ctx: &DoctorContext) -> CheckResult;
}

enum Category { Process, FileIntegrity, Config, Dependencies, Resources }
enum CheckResult { Pass(String), Warning(String), Failure(String) }
```

**Checks**:
- **Process**: daemon running (PID + process alive), socket reachable, no stale lock
- **File integrity**: `PRAGMA integrity_check`, WAL size, schema version
- **Config**: CLAUDE.md markers present + version current, hooks registered, `.scavenger.toml` parseable
- **Dependencies**: Claude Code installed (`claude --version`), tree-sitter grammars loadable
- **Resources**: DB size, node/edge counts, index age, `INDEX_BLIND_SPOT` active signals, federation health

Output: `[✓]`/`[✗]`/`[!]` with color (respecting `NO_COLOR`). `--verbose` for detail. `--format=json` for CI. Exit codes: 0 = all pass, 1 = warnings, 2 = failures.

### 12.4 `scavenger stats`

Reads from `daemon_meta.db`'s `token_log` table. Accepts optional `--session <id>` filter or defaults to the latest session.

```
$ scavenger stats

Session: abc123 (2026-02-28 14:32 – 15:01)
────────────────────────────────────────────
Tool calls:          14
Tokens used:      1,240    (with Scavenger)
Tokens estimated:  9,760   (without Scavenger)
Net savings:       8,520   tokens   (88.7%)
────────────────────────────────────────────
By tool:
  get_capsule        8 calls    820 tokens   (est. 6,400 without)
  read_annotations   3 calls    180 tokens   (est. 1,200 without)
  search_docs        2 calls    140 tokens   (est. 1,800 without)
  write_annotation   1 calls    100 tokens   (est.   360 without)
────────────────────────────────────────────
All-time (30 sessions):
  Total saved:  243,400 tokens ≈ $0.73 at $3.00/1M
```

`--session all` aggregates across all sessions. Per-branch filtering available via `--branch <name>` (queries the `branch` column).

### 12.5 Tool Discoverability (no CLAUDE.md injection)

Scavenger does NOT inject into the project's CLAUDE.md. Tool discoverability relies on the MCP protocol: Claude discovers available tools via `tools/list`, and each tool's `#[tool(description)]` annotation explains when and how to use it. Per-node context (annotations, signals, version history) is surfaced automatically through capsules via PreToolUse hooks — no voluntary cooperation needed. The session summary is an opt-in convenience via `read_annotations(session_summary: true)` — see §9.1 Tool 2 design note for rationale.

---

## 13. Configuration

### `.scavenger.toml`

```toml
[budget]
default = 8000                # valid range: [1000, 100000]
warning_threshold = 20000     # warn if --budget exceeds this

[scopes]
auth = "src/auth/"
api = "src/api/"
database = ["src/db/", "src/models/"]

[traversal]
degree_cap = 50               # valid range: [5, 500]
node_budget = 100             # valid range: [10, 10000]
builtins_blocklist = []       # user-populated, e.g. ["log", "unwrap", "toString"]

[docs]
enabled = true
patterns = ["**/*.md", "**/*.markdown"]
exclude = ["**/node_modules/**", "**/target/**"]
priority = ["CLAUDE.md", "README.md"]

[analytics]
enabled = true
price_per_million_tokens = 3.00
session_retention_days = 30

[federation]
repos = []

[[boundaries]]
# from = "frontend/src/**/*.ts"
# to = "backend/src/**/*.py"
# pattern = "api"
```

**Config validation**: all numeric fields are validated at config load time. Out-of-range values are clamped to the nearest valid bound and a warning is logged: "Config: degree_cap=0 is below minimum 5, using 5." Valid ranges are enforced, not advisory.

**Federation DB validation**: on first connection to a federated repo's SQLite DB, verify: (1) expected tables exist (`nodes`, `nodes_fts`, `doc_chunks`, `doc_chunks_fts`), (2) `user_version` is in supported range, (3) `PRAGMA quick_check` passes. Cache the validation result. Reject databases that fail with a diagnostic in `scavenger doctor`.

---

## 14. Project Structure

```
scavenger/
├── Cargo.toml
├── rust-toolchain.toml              # pin nightly version for rmcp
├── .scavenger.toml.example
├── src/
│   ├── main.rs                      # CLI entry, subcommand dispatch (clap)
│   ├── daemon/
│   │   ├── mod.rs                   # daemon main loop: UDS listener + watcher
│   │   ├── handlers.rs              # request handlers (5 MCP tools + hook requests)
│   │   ├── coordinator.rs           # ReindexCoordinator: branch detection, DB swap, cold start
│   │   ├── watcher.rs               # File watcher: notify-debouncer-full + VCS deferral
│   │   ├── socket.rs                # Unix domain socket listener (hooks + MCP bridge)
│   │   └── federation.rs            # Federated repo discovery + fan-out query execution
│   ├── bridge/
│   │   └── mod.rs                   # MCP bridge: stdio JSON-RPC ↔ UDS daemon translation
│   ├── graph/
│   │   ├── mod.rs                   # StableGraph wrapper, centrality, reverse index
│   │   ├── index.rs                 # tree-sitter → graph builder, skeleton generation
│   │   ├── doc_indexer.rs           # Markdown chunking, doc_chunks table management
│   │   ├── traversal.rs             # Intent-driven traversals with degree cap
│   │   ├── similarity.rs            # Identity migration heuristic
│   │   └── estimator.rs             # "Without index" token estimator
│   ├── capsule/
│   │   ├── mod.rs                   # 6-stage pipeline orchestrator
│   │   ├── gather.rs                # GATHER stage: collect from all sources
│   │   ├── score.rs                 # SCORE stage: per-source formulas
│   │   └── render.rs                # GROUP + RENDER stages: output formatting
│   ├── query/
│   │   ├── mod.rs                   # Query engine entry
│   │   ├── intent.rs                # Intent detection + strategy selection
│   │   └── search.rs                # FTS5 BM25 + centrality ranking
│   ├── memory/
│   │   ├── mod.rs                   # Three-layer memory orchestration
│   │   ├── versions.rs              # Layer 1: node version history
│   │   ├── annotations.rs           # Layer 2: annotation CRUD + staleness + merge
│   │   ├── signals.rs               # Layer 3: behavioral signals + TTL pruning
│   │   ├── session.rs               # Layer 3: session activity log
│   │   └── antipattern.rs           # Anti-pattern detection engine (7 detectors)
│   ├── hooks/
│   │   ├── mod.rs                   # Hook handler (pre/post tool use)
│   │   └── register.rs              # settings.json hook + MCP bridge registration
│   └── db/
│       ├── mod.rs                   # SQLite connection management (per-branch + daemon_meta)
│       ├── schema.rs                # Migrations, all CREATE TABLE statements
│       └── queries.rs               # Typed query helpers
└── docs/
    ├── architecture-components.md   # Component status tracker (quick reference)
    ├── plans/
    │   └── 2026-02-28-consolidated-design.md   ← this file
    └── research/
        ├── 2026-02-25-market-research.md
        ├── 2026-02-26-deep-market-research.md
        └── deep-research-prompt.md
```

---

## 15. Dependencies

| Crate | Purpose |
|-------|---------|
| `rmcp` | Official Rust MCP SDK (nightly required) |
| `tree-sitter` | Language-agnostic AST parsing |
| `tree-sitter-{rust,python,typescript,go,...}` | Per-language grammars (15 crates) |
| `petgraph` | In-memory graph (`StableGraph`), PageRank, traversals, cycle detection |
| `rustworkx-core` | HITS, betweenness centrality (v2) |
| `rusqlite` | SQLite with FTS5 support |
| `tokio-rusqlite` | Async bridge: 1 writer + N readers |
| `tokio` | Async runtime for daemon |
| `notify` + `notify-debouncer-full` | File watching with debounce |
| `ignore` | `.gitignore`-aware file filtering |
| `clap` | CLI argument parsing |
| `serde` + `serde_json` | Serialization (MCP protocol, config, edges_json) |
| `schemars` | JSON Schema generation for MCP tools |
| `parking_lot` | Fast RwLock for in-memory graph concurrency (spin-first, avoids kernel syscalls) |
| `heck` | camelCase/snake_case splitting for FTS5 index-time token preprocessing |
| `strsim` | Jaro-Winkler, Levenshtein for fuzzy matching |
| `md5` | `signature_hash` computation |
| `rayon` | Parallel file parsing during initial index |
| `fs2` | File locking for settings.json writes |
| `toml` | `.scavenger.toml` parsing |
| `owo-colors` | Terminal coloring (respects `NO_COLOR`) |

---

## 16. v2 Backlog

Items explicitly deferred from v1, tracked for future implementation:

| Item | Source | Description |
|------|--------|-------------|
| Personalized PageRank | T3, §4.9 | Bias PageRank toward recently edited files (10× multiplier). Depends on session activity log (now in v1). |
| HITS algorithm | T3 | Distinguish "hub" modules (barrel files) from "authority" modules (core logic). `rustworkx-core` provides it. |
| Feedback loop | §7 | Track repeated queries and expand capsule budget for focused areas. Needs usage data from v1 to design. |
| `tiktoken-rs` token counting | T21, T22 | Replace `len/4` approximation with accurate tokenizer. Upgrade when benchmarking reveals quality issues near budget limits. |
| WASM grammar loading | Audit §2.1 | Dynamic language support without recompilation. Enables user-extensible grammars. |
| `tantivy` for tunable BM25 | T2 | Replace FTS5 when BM25 parameter tuning or custom code tokenizers are needed (camelCase/snake_case splitting). |
| Intent-specific budget sizing | T21 | Debug needs more call-chain context, refactor needs more blast-radius context. Adjust budget proportions by intent. |
| HTTP token analytics dashboard | PindeX §6 | Minimal HTML dashboard on port 7842. Schema already supports it via `token_log`. |
| Cross-repo edges (federation) | PindeX §7 | `ExternalImport` edges across repo boundaries. Requires coordinated NodeId resolution. |
| Benchmark framework | T22, audit §3.10 | Head-to-head comparison of context engines on SWE-bench tasks. Strongest credibility signal. |
| YAML/TXT document indexing | PindeX §5 | Extend doc indexer beyond `.md` files. |
| Skeleton compression (zstd) | T20 | Dictionary-trained zstd for skeleton column. 5–10× compression. Optimization for large projects. |
| `scavenger serve --remote` | Original design | Share index across a team via network. |
| VS Code extension | Original design | IDE integration beyond terminal-first. |
| MCP rate limiting | Design review | Per-second rate limiter on MCP tool calls to prevent runaway loops from flooding the daemon. |
| MCP write priority queue | Design review | Priority queue for annotation writes over indexing writes on the single SQLite writer. Add if latency issues surface. |
| `scavenger pause`/`resume` | Design review | Temporarily disable Scavenger without killing the daemon. UX polish. |
| Parent-scope NodeId | Design review §3.4 | Include parent scope in NodeId hash to disambiguate nested functions with identical names/signatures. |

---

## 17. Component Status

### Graph Layer (16 components)

| Component | Status |
|-----------|--------|
| Node data model (fields, types) | CLOSED |
| Edge types (Imports, Calls, TypeRef, Extends, Implements, Exports, Contains) | CLOSED |
| NodeId scheme (`hash(file_path, symbol_name, signature)`) | CLOSED |
| SQLite schema (all tables defined in §3.5) | CLOSED |
| FTS5 virtual tables (`nodes_fts`, `annotations_fts`, `doc_chunks_fts`) | CLOSED |
| PageRank / centrality (inline full recompute) | CLOSED |
| File re-indexing flow (delete-and-rebuild, 13 steps) | CLOSED |
| Edge rebuild strategy (replace all from changed file) | CLOSED |
| Cross-file edge staleness (reverse index + async cascade + lazy fallback) | CLOSED |
| Similarity heuristic (threshold 0.6) | CLOSED |
| Node version history (state-snapshot, last 5, ordinal decay) | CLOSED |
| Graph type (`petgraph::StableGraph`) | CLOSED |
| `signature_hash` column (MD5[0..8] fast diff) | CLOSED |
| `files` table (file-level token estimates) | CLOSED |
| Document indexer (markdown chunking, `.md` only for v1) | CLOSED |
| `doc_chunks` table + FTS5 | CLOSED |

### Memory & Observation Engine (15 components)

| Component | Status |
|-----------|--------|
| Three-layer memory model | CLOSED |
| Semantic annotations — data model (flexible anchoring) | CLOSED |
| Semantic annotations — staleness by anchor type | CLOSED |
| MCP annotation tools (read, write, delete) | CLOSED |
| Behavioral signals — 7 event types | CLOSED |
| Behavioral signals — TTL pruning (48h / 2 sessions) | CLOSED |
| Anti-pattern detection (7 detectors, fire-once-at-N) | CLOSED |
| `INDEX_BLIND_SPOT` signal | CLOSED |
| `FAILED_SEARCH` detector | CLOSED |
| Fire-once deduplication rule | CLOSED |
| `detail` column on behavioral_signals | CLOSED |
| Session activity log (per-branch, TTL-pruned) | CLOSED |
| Feedback loop | CLOSED (deferred v2) |
| Observation compaction | CLOSED (eliminated) |
| Annotation fork/merge (per-branch, union on merge commit) | CLOSED |

### Query Engine (6 components)

| Component | Status |
|-----------|--------|
| Intent detection (hybrid keyword classifier) | CLOSED |
| Strategy-to-traversal mapping (5 strategies, degree cap, node budget) | CLOSED |
| FTS5 BM25 integration (`0.6 × bm25 + 0.4 × centrality`) | CLOSED |
| TF-IDF scoring layer | CLOSED (eliminated, subsumed by BM25) |
| Capsule node ranking (per-source formulas → unified competition) | CLOSED |
| Scope tags (hybrid path-prefix + lazy virtual nodes) | CLOSED |

### Capsule Assembly (12 components)

| Component | Status |
|-----------|--------|
| Pipeline architecture (6 stages) | CLOSED |
| `DocChunk` context item source type | CLOSED |
| Pinning rules (target + signals + 1-hop structural) | CLOSED |
| Per-source scoring formulas (6 formulas, all → [0,1]) | CLOSED |
| Recency decay (`e^(−0.01 × hours)`) | CLOSED |
| Token budget (8k default, 10% headroom) | CLOSED |
| Token counting method (`len/4`) | CLOSED |
| Greedy fill (skip oversized, continue) | CLOSED |
| Budget exhausted safety net | CLOSED |
| Body inclusion (leftover > 200 tokens) | CLOSED |
| Pre-computed skeleton nodes | CLOSED |
| Output render format | CLOSED |

### MCP Server & Daemon Lifecycle (13 components)

| Component | Status |
|-----------|--------|
| MCP tool definitions (v1: 5 tools) | CLOSED |
| `search_docs` tool | CLOSED |
| `tags` column on annotations | CLOSED |
| Transport (stdio) | CLOSED |
| SQLite configuration (WAL, PRAGMAs) | CLOSED |
| Concurrent access (1 writer + N readers) | CLOSED |
| Daemon startup sequence (12 steps) | CLOSED |
| Daemon shutdown / signal handling | CLOSED |
| Crash recovery (WAL guarantees + dirty flag) | CLOSED |
| Daemon socket / PID file | CLOSED |
| File watcher batching (300ms debounce, VCS deferral) | CLOSED |
| Index-per-branch architecture | CLOSED |
| Merge commit detection | CLOSED |

### Hooks Integration (6 components)

| Component | Status |
|-----------|--------|
| PreToolUse hook (Read → capsule via `additionalContext`) | CLOSED |
| PostToolUse hook (Write/Edit → enqueue re-index) | CLOSED |
| Hook failure modes (fail open, exit 0) | CLOSED |
| Hook performance budget (< 50ms) | CLOSED |
| Batching rapid edits (subsumed by file watcher debounce) | CLOSED |
| Hook-to-daemon communication (Unix domain socket) | CLOSED |

### CLI Interface (3 components)

| Component | Status |
|-----------|--------|
| Command surface (init, daemon, index, capsule, etc.) | CLOSED |
| `scavenger init` behavior | CLOSED |
| `scavenger doctor` checks (5 categories, trait registry) | CLOSED |

### Language Support (4 components)

| Component | Status |
|-----------|--------|
| v1 language targets (15 languages) | CLOSED |
| Grammar dependency strategy (crate deps) | CLOSED |
| Symbol extraction per language (`tags.scm`) | CLOSED |
| Cross-language edge resolution (heuristic, confidence levels) | CLOSED |

### Token Analytics (4 components)

| Component | Status |
|-----------|--------|
| `token_log` table | CLOSED |
| "Without index" estimator | CLOSED |
| `scavenger stats` CLI command | CLOSED |
| v2 HTTP dashboard | OPEN (deferred) |

### Federation (7 components)

| Component | Status |
|-----------|--------|
| Federation configuration | CLOSED |
| `federated_repos` table | CLOSED |
| `get_capsule` federation fallback | CLOSED |
| `search_docs` fan-out | CLOSED |
| `scavenger federate` CLI subcommands | CLOSED |
| Federation check in `scavenger doctor` | CLOSED |
| v2 cross-repo edges | OPEN (deferred) |

### Totals

| Status | Count |
|--------|-------|
| CLOSED | 84 |
| OPEN (deferred v2) | 2 |

**All v1 components are closed. Implementation can begin.**
