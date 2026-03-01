# Data Model: Scavenger v1 Core Engine

**Branch**: `v1-core-engine` | **Date**: 2026-02-28
**Source**: `docs/plans/2026-02-28-consolidated-design.md` §3.5-3.7

## Overview

Scavenger uses two SQLite databases per project plus in-memory structures:
- **Per-branch index DB** (`.scavenger/indexes/<branch>.db`) — graph, memory, documents
- **Shared daemon_meta DB** (`.scavenger/daemon_meta.db`) — project-level state, analytics
- **In-memory** — StableGraph, reverse index, session state

## Per-Branch Index DB Schema

### SQLite Configuration (applied at connection open)

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;
PRAGMA cache_size = -64000;
PRAGMA mmap_size = 268435456;
PRAGMA auto_vacuum = INCREMENTAL;
```

Schema migration: `PRAGMA user_version = 1` on creation. On connect, read `user_version` and run sequential migration functions. Downgrade guard: if `user_version > KNOWN_MAX_VERSION`, refuse to open with error message.

---

### `nodes` table

```sql
CREATE TABLE nodes (
    _rowid              INTEGER PRIMARY KEY,
    id                  TEXT UNIQUE NOT NULL,
    kind                TEXT NOT NULL,
    name                TEXT NOT NULL,
    file_path           TEXT NOT NULL,
    line_start          INTEGER NOT NULL,
    line_end            INTEGER NOT NULL,
    signature           TEXT NOT NULL,
    signature_hash      TEXT NOT NULL,
    docstring           TEXT,
    skeleton            TEXT NOT NULL,
    centrality          REAL DEFAULT 0.0,
    checksum            BLOB NOT NULL
);
```

| Column | Description |
|--------|-------------|
| id | NodeId = hash(file_path, symbol_name, signature) |
| kind | Function, Method, Class, Interface, Type, Enum, ExportedVar, Module, File |
| signature_hash | First 8 hex chars of MD5 over whitespace-normalized signature |
| skeleton | Pre-rendered signature + docstring, ready to emit in capsule |
| centrality | PageRank score (primary in-memory; persisted during idle) |
| checksum | Body content hash for staleness detection |

### `nodes_fts` virtual table

```sql
CREATE VIRTUAL TABLE nodes_fts USING fts5(
    name, signature, docstring,
    content=nodes, content_rowid=_rowid
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

Index-time: split camelCase/snake_case via `heck` crate. `getUserById` indexed as `getUserById get User By Id`.

---

### `edges` table

```sql
CREATE TABLE edges (
    from_id    TEXT NOT NULL,
    to_id      TEXT NOT NULL,
    kind       TEXT NOT NULL,
    weight     REAL DEFAULT 1.0,
    confidence TEXT DEFAULT 'precise',
    PRIMARY KEY (from_id, to_id, kind)
);

CREATE INDEX idx_edges_to ON edges(to_id);
```

| Column | Description |
|--------|-------------|
| kind | Imports, Calls, TypeRef, Extends, Implements, Exports, Contains |
| confidence | precise, heuristic, speculative |

---

### `files` table

```sql
CREATE TABLE files (
    id                  INTEGER PRIMARY KEY,
    file_path           TEXT UNIQUE NOT NULL,
    file_type           TEXT NOT NULL,
    raw_token_estimate  INTEGER NOT NULL,
    last_indexed        INTEGER NOT NULL
);
```

| Column | Description |
|--------|-------------|
| file_type | 'code' or 'doc' |
| raw_token_estimate | len/4 of full file content |
| last_indexed | Unix epoch seconds |

---

### `node_versions` table (Layer 1 memory)

```sql
CREATE TABLE node_versions (
    id              INTEGER PRIMARY KEY,
    symbol_hash     TEXT NOT NULL,
    version_num     INTEGER NOT NULL,
    file_path       TEXT NOT NULL,
    session_id      TEXT,
    node_kind       TEXT NOT NULL,
    signature       TEXT NOT NULL,
    signature_hash  TEXT NOT NULL,
    edges_json      TEXT NOT NULL,
    body_hash       BLOB,
    created_at      INTEGER NOT NULL,
    UNIQUE(symbol_hash, version_num)
);

CREATE INDEX idx_versions_lookup ON node_versions(symbol_hash, version_num DESC);
```

Retention: last 5 versions per symbol. No time-based expiry.

---

### `annotations` table (Layer 2 memory)

```sql
CREATE TABLE annotations (
    _rowid       INTEGER PRIMARY KEY,
    id           TEXT UNIQUE NOT NULL,
    anchor_type  TEXT,
    anchor_value TEXT,
    text         TEXT NOT NULL,
    tags         TEXT,
    stale        BOOLEAN DEFAULT FALSE,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
);

CREATE INDEX idx_annotations_anchor ON annotations(anchor_type, anchor_value);
```

| anchor_type | anchor_value | Staleness |
|-------------|-------------|-----------|
| 'node' | NodeId | Flagged on checksum change; migrated via similarity |
| 'file' | file path | Flagged on file modification |
| 'scope' | scope name | No auto staleness; LLM-managed |
| NULL | NULL | No auto staleness; project-level |

### `annotations_fts` virtual table

```sql
CREATE VIRTUAL TABLE annotations_fts USING fts5(
    text, tags,
    content=annotations, content_rowid=_rowid
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

---

### `behavioral_signals` table (Layer 3 memory)

```sql
CREATE TABLE behavioral_signals (
    id         INTEGER PRIMARY KEY,
    kind       TEXT NOT NULL CHECK(kind IN (
                   'THRASHING', 'DEAD_END', 'CYCLE_INTRODUCED',
                   'LARGE_BLAST_RADIUS', 'UNTESTED', 'INDEX_BLIND_SPOT',
                   'FAILED_SEARCH'
               )),
    node_id    TEXT,
    file_path  TEXT,
    session_id TEXT NOT NULL,
    timestamp  INTEGER NOT NULL,
    detail     TEXT
);

CREATE INDEX idx_signals_node ON behavioral_signals(node_id, timestamp DESC);
CREATE INDEX idx_signals_session ON behavioral_signals(session_id);
```

TTL: 48 hours or 2 sessions, whichever is longer.

---

### `session_log` table (Layer 3 memory)

```sql
CREATE TABLE session_log (
    id         INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    file_path  TEXT,
    symbol     TEXT,
    timestamp  INTEGER NOT NULL
);

CREATE INDEX idx_session_log ON session_log(session_id, timestamp DESC);
```

TTL: 48 hours or 2 sessions.

---

### `doc_chunks` table

```sql
CREATE TABLE doc_chunks (
    id             INTEGER PRIMARY KEY,
    file_path      TEXT NOT NULL,
    chunk_index    INTEGER NOT NULL,
    heading        TEXT,
    start_line     INTEGER NOT NULL,
    end_line       INTEGER NOT NULL,
    content        TEXT NOT NULL,
    token_estimate INTEGER NOT NULL,
    last_indexed   INTEGER NOT NULL,
    content_hash   TEXT NOT NULL,
    UNIQUE(file_path, chunk_index)
);
```

### `doc_chunks_fts` virtual table

```sql
CREATE VIRTUAL TABLE doc_chunks_fts USING fts5(
    content, heading,
    content=doc_chunks, content_rowid=id
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

---

## Shared DB Schema (`daemon_meta.db`)

### `daemon_meta` table

```sql
CREATE TABLE daemon_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

Rows: `current_branch`, `reindex_state` (ready|switching|cold_start), `last_shutdown` (clean|dirty).

### `federated_repos` table

```sql
CREATE TABLE federated_repos (
    id         INTEGER PRIMARY KEY,
    path       TEXT UNIQUE NOT NULL,
    added_at   INTEGER NOT NULL,
    last_seen  INTEGER
);
```

### `token_log` table

```sql
CREATE TABLE token_log (
    id                  INTEGER PRIMARY KEY,
    timestamp           INTEGER NOT NULL,
    session_id          TEXT NOT NULL,
    branch              TEXT NOT NULL,
    tool_name           TEXT NOT NULL,
    query               TEXT,
    intent              TEXT,
    tokens_actual       INTEGER NOT NULL,
    tokens_estimated    INTEGER NOT NULL,
    files_touched       TEXT
);

CREATE INDEX idx_token_log_session ON token_log(session_id, timestamp);
CREATE INDEX idx_token_log_branch ON token_log(branch);
```

Lives in daemon_meta.db (not per-branch) so analytics survive branch cleanup.

---

## In-Memory Structures

### GraphState

```rust
struct GraphState {
    graph: StableGraph<NodeWeight, EdgeWeight, Directed>,
    reverse_index: HashMap<NodeId, Vec<PathBuf>>,
}
// Wrapped in Arc<parking_lot::RwLock<GraphState>>
```

### NodeWeight

```rust
struct NodeWeight {
    id: NodeId,
    kind: NodeKind,
    name: String,
    file_path: PathBuf,
    line_start: u32,
    line_end: u32,
    signature: String,
    signature_hash: String,
    docstring: Option<String>,
    skeleton: String,
    centrality: f32,
    checksum: Vec<u8>,
}
```

### EdgeWeight

```rust
struct EdgeWeight {
    kind: EdgeKind,
    weight: f32,
    confidence: Confidence,
}
```

### Session State (in daemon memory)

- `current_session_id: String` — from hook payloads or fallback UUID
- `antipattern_dedup: HashSet<(SignalType, String)>` — cleared on daemon restart
- `pending_orphans: HashSet<OrphanData>` — held for one debounce cycle
- `thrashing_buffer: HashMap<NodeId, VecDeque<(Instant, Vec<u8>)>>` — ring buffer per node
- `failed_search_counts: HashMap<String, u32>` — per normalized query

### ContextItem (capsule pipeline)

```rust
struct ContextItem {
    content: String,
    token_count: u32,
    score: f32,
    group: OutputGroup,
    pinned: bool,
}

enum OutputGroup {
    Pinned,
    Callers,
    Callees,
    Context,
    Documentation,
}
```

---

## Entity Relationship Summary

```
Node ---[edges]---> Node          (7 edge types, directed)
Node ---[versions]-> NodeVersion  (last 5 per symbol)
Node <--[anchor]--- Annotation    (flexible: node/file/scope/None)
Node <--[signal]--- BehavioralSignal (7 types, TTL-pruned)
File ---[contains]-> Node         (via file_path)
File ---[chunks]--> DocChunk      (heading-boundary split)
Session ---[log]---> SessionLog   (read/query/edit events)
Session ---[token]-> TokenLog     (per-tool-call in daemon_meta.db)
```
