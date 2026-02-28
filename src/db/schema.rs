use super::{DbError, DbResult};
use rusqlite::Connection;

pub const KNOWN_MAX_VERSION: u32 = 1;

/// Ensure the per-branch index database has all required tables, FTS5 virtual
/// tables, triggers, and indexes. Uses PRAGMA user_version for migration tracking.
pub fn ensure_branch_schema(conn: &Connection) -> DbResult<()> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if current > KNOWN_MAX_VERSION {
        return Err(DbError::VersionTooNew {
            found: current,
            max: KNOWN_MAX_VERSION,
        });
    }

    if current == 0 {
        create_branch_schema_v1(conn)?;
        conn.pragma_update(None, "user_version", 1)?;
    }

    Ok(())
}

/// Ensure the shared daemon_meta database has all required tables.
pub fn ensure_daemon_meta_schema(conn: &Connection) -> DbResult<()> {
    let current: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if current > KNOWN_MAX_VERSION {
        return Err(DbError::VersionTooNew {
            found: current,
            max: KNOWN_MAX_VERSION,
        });
    }

    if current == 0 {
        create_daemon_meta_schema_v1(conn)?;
        conn.pragma_update(None, "user_version", 1)?;
    }

    Ok(())
}

fn create_branch_schema_v1(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(BRANCH_SCHEMA_V1)?;
    Ok(())
}

fn create_daemon_meta_schema_v1(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(DAEMON_META_SCHEMA_V1)?;
    Ok(())
}

const BRANCH_SCHEMA_V1: &str = r#"
-- nodes table
CREATE TABLE IF NOT EXISTS nodes (
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

-- nodes FTS5 virtual table
CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts USING fts5(
    name, signature, docstring,
    content=nodes, content_rowid=_rowid
);

CREATE TRIGGER IF NOT EXISTS nodes_ai AFTER INSERT ON nodes BEGIN
    INSERT INTO nodes_fts(rowid, name, signature, docstring)
    VALUES (new._rowid, new.name, new.signature, new.docstring);
END;

CREATE TRIGGER IF NOT EXISTS nodes_ad AFTER DELETE ON nodes BEGIN
    INSERT INTO nodes_fts(nodes_fts, rowid, name, signature, docstring)
    VALUES ('delete', old._rowid, old.name, old.signature, old.docstring);
END;

CREATE TRIGGER IF NOT EXISTS nodes_au AFTER UPDATE ON nodes BEGIN
    INSERT INTO nodes_fts(nodes_fts, rowid, name, signature, docstring)
    VALUES ('delete', old._rowid, old.name, old.signature, old.docstring);
    INSERT INTO nodes_fts(rowid, name, signature, docstring)
    VALUES (new._rowid, new.name, new.signature, new.docstring);
END;

-- edges table
CREATE TABLE IF NOT EXISTS edges (
    from_id    TEXT NOT NULL,
    to_id      TEXT NOT NULL,
    kind       TEXT NOT NULL,
    weight     REAL DEFAULT 1.0,
    confidence TEXT DEFAULT 'precise',
    PRIMARY KEY (from_id, to_id, kind)
);

CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_id);

-- files table
CREATE TABLE IF NOT EXISTS files (
    id                  INTEGER PRIMARY KEY,
    file_path           TEXT UNIQUE NOT NULL,
    file_type           TEXT NOT NULL,
    raw_token_estimate  INTEGER NOT NULL,
    last_indexed        INTEGER NOT NULL
);

-- node_versions table (Layer 1 memory)
CREATE TABLE IF NOT EXISTS node_versions (
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

CREATE INDEX IF NOT EXISTS idx_versions_lookup
    ON node_versions(symbol_hash, version_num DESC);

-- annotations table (Layer 2 memory)
CREATE TABLE IF NOT EXISTS annotations (
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

CREATE INDEX IF NOT EXISTS idx_annotations_anchor
    ON annotations(anchor_type, anchor_value);

-- annotations FTS5 virtual table
CREATE VIRTUAL TABLE IF NOT EXISTS annotations_fts USING fts5(
    text, tags,
    content=annotations, content_rowid=_rowid
);

CREATE TRIGGER IF NOT EXISTS annotations_ai AFTER INSERT ON annotations BEGIN
    INSERT INTO annotations_fts(rowid, text, tags)
    VALUES (new._rowid, new.text, new.tags);
END;

CREATE TRIGGER IF NOT EXISTS annotations_ad AFTER DELETE ON annotations BEGIN
    INSERT INTO annotations_fts(annotations_fts, rowid, text, tags)
    VALUES ('delete', old._rowid, old.text, old.tags);
END;

CREATE TRIGGER IF NOT EXISTS annotations_au AFTER UPDATE ON annotations BEGIN
    INSERT INTO annotations_fts(annotations_fts, rowid, text, tags)
    VALUES ('delete', old._rowid, old.text, old.tags);
    INSERT INTO annotations_fts(rowid, text, tags)
    VALUES (new._rowid, new.text, new.tags);
END;

-- behavioral_signals table (Layer 3 memory)
CREATE TABLE IF NOT EXISTS behavioral_signals (
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

CREATE INDEX IF NOT EXISTS idx_signals_node
    ON behavioral_signals(node_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_signals_session
    ON behavioral_signals(session_id);

-- session_log table (Layer 3 memory)
CREATE TABLE IF NOT EXISTS session_log (
    id         INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    file_path  TEXT,
    symbol     TEXT,
    timestamp  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_session_log
    ON session_log(session_id, timestamp DESC);

-- doc_chunks table
CREATE TABLE IF NOT EXISTS doc_chunks (
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

-- doc_chunks FTS5 virtual table
CREATE VIRTUAL TABLE IF NOT EXISTS doc_chunks_fts USING fts5(
    content, heading,
    content=doc_chunks, content_rowid=id
);

CREATE TRIGGER IF NOT EXISTS doc_chunks_ai AFTER INSERT ON doc_chunks BEGIN
    INSERT INTO doc_chunks_fts(rowid, content, heading)
    VALUES (new.id, new.content, new.heading);
END;

CREATE TRIGGER IF NOT EXISTS doc_chunks_ad AFTER DELETE ON doc_chunks BEGIN
    INSERT INTO doc_chunks_fts(doc_chunks_fts, rowid, content, heading)
    VALUES ('delete', old.id, old.content, old.heading);
END;

CREATE TRIGGER IF NOT EXISTS doc_chunks_au AFTER UPDATE ON doc_chunks BEGIN
    INSERT INTO doc_chunks_fts(doc_chunks_fts, rowid, content, heading)
    VALUES ('delete', old.id, old.content, old.heading);
    INSERT INTO doc_chunks_fts(rowid, content, heading)
    VALUES (new.id, new.content, new.heading);
END;
"#;

const DAEMON_META_SCHEMA_V1: &str = r#"
-- daemon_meta key-value store
CREATE TABLE IF NOT EXISTS daemon_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- federated_repos
CREATE TABLE IF NOT EXISTS federated_repos (
    id         INTEGER PRIMARY KEY,
    path       TEXT UNIQUE NOT NULL,
    added_at   INTEGER NOT NULL,
    last_seen  INTEGER
);

-- token_log (analytics)
CREATE TABLE IF NOT EXISTS token_log (
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

CREATE INDEX IF NOT EXISTS idx_token_log_session
    ON token_log(session_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_token_log_branch
    ON token_log(branch);
"#;

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;

    #[test]
    fn test_branch_schema_creates_all_tables() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_branch_schema(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"nodes".to_string()));
        assert!(tables.contains(&"edges".to_string()));
        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"node_versions".to_string()));
        assert!(tables.contains(&"annotations".to_string()));
        assert!(tables.contains(&"behavioral_signals".to_string()));
        assert!(tables.contains(&"session_log".to_string()));
        assert!(tables.contains(&"doc_chunks".to_string()));
    }

    #[test]
    fn test_branch_schema_fts5_tables() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_branch_schema(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE '%_fts%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.iter().any(|t| t.contains("nodes_fts")));
        assert!(tables.iter().any(|t| t.contains("annotations_fts")));
        assert!(tables.iter().any(|t| t.contains("doc_chunks_fts")));
    }

    #[test]
    fn test_branch_schema_version_set() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_branch_schema(&conn).unwrap();
        let ver: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(ver, 1);
    }

    #[test]
    fn test_branch_schema_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_branch_schema(&conn).unwrap();
        ensure_branch_schema(&conn).unwrap();
    }

    #[test]
    fn test_downgrade_guard() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        let result = ensure_branch_schema(&conn);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("999"));
    }

    #[test]
    fn test_daemon_meta_schema_creates_all_tables() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_daemon_meta_schema(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"daemon_meta".to_string()));
        assert!(tables.contains(&"federated_repos".to_string()));
        assert!(tables.contains(&"token_log".to_string()));
    }

    #[test]
    fn test_nodes_fts_trigger() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_branch_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum)
             VALUES ('test1', 'Function', 'getUserById', 'src/users.rs', 10, 20, 'fn get_user_by_id(id: u32)', 'abcdef01', 'fn get_user_by_id(id: u32)', X'DEADBEEF')",
            [],
        ).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM nodes_fts WHERE nodes_fts MATCH 'getUserById'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
