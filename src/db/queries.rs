#![allow(dead_code)]
use rusqlite::{Connection, OptionalExtension, params};

use super::DbResult;
use crate::graph::types::{Confidence, EdgeKind, NodeKind};

// ── Node CRUD ──

#[allow(clippy::too_many_arguments)]
pub fn upsert_node(
    conn: &Connection,
    id: &str,
    kind: NodeKind,
    name: &str,
    file_path: &str,
    line_start: u32,
    line_end: u32,
    signature: &str,
    signature_hash: &str,
    docstring: Option<&str>,
    skeleton: &str,
    checksum: &[u8],
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, docstring, skeleton, checksum)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(id) DO UPDATE SET
            kind=excluded.kind, name=excluded.name, file_path=excluded.file_path,
            line_start=excluded.line_start, line_end=excluded.line_end,
            signature=excluded.signature, signature_hash=excluded.signature_hash,
            docstring=excluded.docstring, skeleton=excluded.skeleton,
            checksum=excluded.checksum",
        params![id, kind.as_str(), name, file_path, line_start, line_end, signature, signature_hash, docstring, skeleton, checksum],
    )?;
    Ok(())
}

pub fn delete_nodes_by_file(conn: &Connection, file_path: &str) -> DbResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM nodes WHERE file_path = ?1")?;
    let ids: Vec<String> = stmt
        .query_map(params![file_path], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    conn.execute("DELETE FROM nodes WHERE file_path = ?1", params![file_path])?;
    Ok(ids)
}

pub fn get_node_ids_for_file(conn: &Connection, file_path: &str) -> DbResult<Vec<String>> {
    let mut stmt = conn.prepare("SELECT id FROM nodes WHERE file_path = ?1")?;
    let ids = stmt
        .query_map(params![file_path], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

pub fn update_centrality(conn: &Connection, node_id: &str, centrality: f64) -> DbResult<()> {
    conn.execute(
        "UPDATE nodes SET centrality = ?1 WHERE id = ?2",
        params![centrality, node_id],
    )?;
    Ok(())
}

// ── Edge CRUD ──

pub fn upsert_edge(
    conn: &Connection,
    from_id: &str,
    to_id: &str,
    kind: EdgeKind,
    weight: f64,
    confidence: Confidence,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO edges (from_id, to_id, kind, weight, confidence)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(from_id, to_id, kind) DO UPDATE SET
            weight=excluded.weight, confidence=excluded.confidence",
        params![from_id, to_id, kind.as_str(), weight, confidence.as_str()],
    )?;
    Ok(())
}

pub fn delete_edges_from_node(conn: &Connection, from_id: &str) -> DbResult<()> {
    conn.execute("DELETE FROM edges WHERE from_id = ?1", params![from_id])?;
    Ok(())
}

pub fn delete_edges_for_nodes(conn: &Connection, node_ids: &[String]) -> DbResult<()> {
    for id in node_ids {
        conn.execute(
            "DELETE FROM edges WHERE from_id = ?1 OR to_id = ?1",
            params![id],
        )?;
    }
    Ok(())
}

pub struct EdgeRow {
    pub from_id: String,
    pub to_id: String,
    pub kind: String,
    pub weight: f64,
    pub confidence: String,
}

pub fn get_all_edges(conn: &Connection) -> DbResult<Vec<EdgeRow>> {
    let mut stmt = conn.prepare("SELECT from_id, to_id, kind, weight, confidence FROM edges")?;
    let rows = stmt
        .query_map([], |row| {
            Ok(EdgeRow {
                from_id: row.get(0)?,
                to_id: row.get(1)?,
                kind: row.get(2)?,
                weight: row.get(3)?,
                confidence: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ── Files CRUD ──

pub fn upsert_file(
    conn: &Connection,
    file_path: &str,
    file_type: &str,
    raw_token_estimate: u32,
    last_indexed: i64,
) -> DbResult<()> {
    upsert_file_with_hash(
        conn,
        file_path,
        file_type,
        raw_token_estimate,
        last_indexed,
        None,
    )
}

pub fn upsert_file_with_hash(
    conn: &Connection,
    file_path: &str,
    file_type: &str,
    raw_token_estimate: u32,
    last_indexed: i64,
    content_hash: Option<&str>,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO files (file_path, file_type, raw_token_estimate, last_indexed, content_hash)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(file_path) DO UPDATE SET
            file_type=excluded.file_type, raw_token_estimate=excluded.raw_token_estimate,
            last_indexed=excluded.last_indexed, content_hash=excluded.content_hash",
        params![
            file_path,
            file_type,
            raw_token_estimate,
            last_indexed,
            content_hash
        ],
    )?;
    Ok(())
}

pub fn get_file_content_hash(conn: &Connection, file_path: &str) -> DbResult<Option<String>> {
    let result = conn
        .query_row(
            "SELECT content_hash FROM files WHERE file_path = ?1",
            params![file_path],
            |row| row.get(0),
        )
        .optional()?;
    Ok(result.flatten())
}

pub fn get_file_last_indexed(conn: &Connection, file_path: &str) -> DbResult<Option<i64>> {
    let result = conn
        .query_row(
            "SELECT last_indexed FROM files WHERE file_path = ?1",
            params![file_path],
            |row| row.get(0),
        )
        .optional()?;
    Ok(result)
}

// ── Annotations CRUD ──

#[allow(clippy::too_many_arguments)]
pub fn insert_annotation(
    conn: &Connection,
    id: &str,
    anchor_type: Option<&str>,
    anchor_value: Option<&str>,
    text: &str,
    tags: Option<&str>,
    kind: &str,
    content_hash: Option<&str>,
    now: i64,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO annotations (id, anchor_type, anchor_value, text, tags, kind, content_hash, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        params![id, anchor_type, anchor_value, text, tags, kind, content_hash, now],
    )?;
    Ok(())
}

pub fn mark_annotations_stale_for_node(conn: &Connection, node_id: &str) -> DbResult<u64> {
    let changed = conn.execute(
        "UPDATE annotations SET stale = TRUE, updated_at = strftime('%s','now') WHERE anchor_type = 'node' AND anchor_value = ?1",
        params![node_id],
    )?;
    Ok(changed as u64)
}

pub fn get_annotations_for_anchor(
    conn: &Connection,
    anchor_type: &str,
    anchor_value: &str,
) -> DbResult<Vec<AnnotationRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, text, tags, stale, created_at, updated_at, kind, quality, retrieval_count
         FROM annotations WHERE anchor_type = ?1 AND anchor_value = ?2",
    )?;
    let rows = stmt
        .query_map(params![anchor_type, anchor_value], |row| {
            Ok(AnnotationRow {
                id: row.get(0)?,
                text: row.get(1)?,
                tags: row.get(2)?,
                stale: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                kind: row.get(6)?,
                quality: row.get(7)?,
                retrieval_count: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub struct AnnotationRow {
    pub id: String,
    pub text: String,
    pub tags: Option<String>,
    pub stale: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub kind: String,
    pub quality: f64,
    pub retrieval_count: i64,
}

pub fn find_annotation_by_content_hash(
    conn: &Connection,
    content_hash: &str,
) -> DbResult<Option<String>> {
    let result = conn
        .query_row(
            "SELECT id FROM annotations WHERE content_hash = ?1",
            params![content_hash],
            |row| row.get(0),
        )
        .optional()?;
    Ok(result)
}

pub fn increment_retrieval_count(conn: &Connection, ids: &[String]) -> DbResult<()> {
    let mut stmt =
        conn.prepare("UPDATE annotations SET retrieval_count = retrieval_count + 1 WHERE id = ?1")?;
    for id in ids {
        stmt.execute(params![id])?;
    }
    Ok(())
}

pub fn update_annotation_quality(conn: &Connection, id: &str, delta: f64) -> DbResult<()> {
    conn.execute(
        "UPDATE annotations SET quality = MIN(1.0, MAX(0.0, quality + ?1)) WHERE id = ?2",
        params![delta, id],
    )?;
    Ok(())
}

pub fn get_low_quality_annotations(
    conn: &Connection,
    threshold: f64,
    min_retrievals: u32,
) -> DbResult<Vec<AnnotationRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, text, tags, stale, created_at, updated_at, kind, quality, retrieval_count
         FROM annotations WHERE quality < ?1 AND retrieval_count >= ?2",
    )?;
    let rows = stmt
        .query_map(params![threshold, min_retrievals], |row| {
            Ok(AnnotationRow {
                id: row.get(0)?,
                text: row.get(1)?,
                tags: row.get(2)?,
                stale: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                kind: row.get(6)?,
                quality: row.get(7)?,
                retrieval_count: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ── Annotation Edges ──

pub struct AnnotationEdgeRow {
    pub from_id: String,
    pub to_id: String,
    pub relation: String,
    pub weight: f64,
    pub created_at: i64,
}

pub fn upsert_annotation_edge(
    conn: &Connection,
    from_id: &str,
    to_id: &str,
    relation: &str,
    weight: f64,
    created_at: i64,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO annotation_edges (from_id, to_id, relation, weight, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(from_id, to_id, relation) DO UPDATE SET
            weight = annotation_edges.weight + excluded.weight",
        params![from_id, to_id, relation, weight, created_at],
    )?;
    Ok(())
}

pub fn get_annotation_edges_from(
    conn: &Connection,
    from_id: &str,
) -> DbResult<Vec<AnnotationEdgeRow>> {
    let mut stmt = conn.prepare(
        "SELECT from_id, to_id, relation, weight, created_at
         FROM annotation_edges WHERE from_id = ?1",
    )?;
    let rows = stmt
        .query_map(params![from_id], |row| {
            Ok(AnnotationEdgeRow {
                from_id: row.get(0)?,
                to_id: row.get(1)?,
                relation: row.get(2)?,
                weight: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_related_annotation_ids(
    conn: &Connection,
    annotation_id: &str,
    limit: u32,
) -> DbResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT to_id FROM annotation_edges WHERE from_id = ?1
         UNION
         SELECT from_id FROM annotation_edges WHERE to_id = ?1
         ORDER BY 1 LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![annotation_id, limit], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn delete_annotation_edges(conn: &Connection, annotation_id: &str) -> DbResult<()> {
    conn.execute(
        "DELETE FROM annotation_edges WHERE from_id = ?1 OR to_id = ?1",
        params![annotation_id],
    )?;
    Ok(())
}

// ── Behavioral Signals ──

pub fn insert_behavioral_signal(
    conn: &Connection,
    kind: &str,
    node_id: Option<&str>,
    file_path: Option<&str>,
    session_id: &str,
    timestamp: i64,
    detail: Option<&str>,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO behavioral_signals (kind, node_id, file_path, session_id, timestamp, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![kind, node_id, file_path, session_id, timestamp, detail],
    )?;
    Ok(())
}

pub fn prune_old_signals(conn: &Connection, cutoff_timestamp: i64) -> DbResult<u64> {
    let deleted = conn.execute(
        "DELETE FROM behavioral_signals WHERE timestamp < ?1",
        params![cutoff_timestamp],
    )?;
    Ok(deleted as u64)
}

// ── Session Log ──

pub fn insert_session_event(
    conn: &Connection,
    session_id: &str,
    event_type: &str,
    file_path: Option<&str>,
    symbol: Option<&str>,
    timestamp: i64,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![session_id, event_type, file_path, symbol, timestamp],
    )?;
    Ok(())
}

pub fn prune_old_sessions(conn: &Connection, cutoff_timestamp: i64) -> DbResult<u64> {
    let deleted = conn.execute(
        "DELETE FROM session_log WHERE timestamp < ?1",
        params![cutoff_timestamp],
    )?;
    Ok(deleted as u64)
}

// ── Doc Chunks ──

#[allow(clippy::too_many_arguments)]
pub fn upsert_doc_chunk(
    conn: &Connection,
    file_path: &str,
    chunk_index: u32,
    heading: Option<&str>,
    start_line: u32,
    end_line: u32,
    content: &str,
    token_estimate: u32,
    last_indexed: i64,
    content_hash: &str,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO doc_chunks (file_path, chunk_index, heading, start_line, end_line, content, token_estimate, last_indexed, content_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(file_path, chunk_index) DO UPDATE SET
            heading=excluded.heading, start_line=excluded.start_line,
            end_line=excluded.end_line, content=excluded.content,
            token_estimate=excluded.token_estimate, last_indexed=excluded.last_indexed,
            content_hash=excluded.content_hash",
        params![file_path, chunk_index, heading, start_line, end_line, content, token_estimate, last_indexed, content_hash],
    )?;
    Ok(())
}

pub fn delete_doc_chunks_for_file(conn: &Connection, file_path: &str) -> DbResult<()> {
    conn.execute(
        "DELETE FROM doc_chunks WHERE file_path = ?1",
        params![file_path],
    )?;
    Ok(())
}

pub fn get_doc_chunk_hash(
    conn: &Connection,
    file_path: &str,
    chunk_index: u32,
) -> DbResult<Option<String>> {
    let result = conn
        .query_row(
            "SELECT content_hash FROM doc_chunks WHERE file_path = ?1 AND chunk_index = ?2",
            params![file_path, chunk_index],
            |row| row.get(0),
        )
        .optional()?;
    Ok(result)
}

// ── FTS5 Search ──

pub struct FtsMatch {
    pub id: String,
    pub rank: f64,
}

pub fn search_nodes_fts(conn: &Connection, query: &str, limit: u32) -> DbResult<Vec<FtsMatch>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, rank FROM nodes_fts
         JOIN nodes n ON nodes_fts.rowid = n._rowid
         WHERE nodes_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![query, limit], |row| {
            Ok(FtsMatch {
                id: row.get(0)?,
                rank: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn search_annotations_fts(
    conn: &Connection,
    query: &str,
    limit: u32,
) -> DbResult<Vec<FtsMatch>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, rank FROM annotations_fts
         JOIN annotations a ON annotations_fts.rowid = a._rowid
         WHERE annotations_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![query, limit], |row| {
            Ok(FtsMatch {
                id: row.get(0)?,
                rank: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn search_doc_chunks_fts(
    conn: &Connection,
    query: &str,
    limit: u32,
) -> DbResult<Vec<DocChunkMatch>> {
    let mut stmt = conn.prepare(
        "SELECT dc.file_path, dc.chunk_index, dc.heading, dc.content, dc.token_estimate, rank
         FROM doc_chunks_fts
         JOIN doc_chunks dc ON doc_chunks_fts.rowid = dc.id
         WHERE doc_chunks_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![query, limit], |row| {
            Ok(DocChunkMatch {
                file_path: row.get(0)?,
                chunk_index: row.get(1)?,
                heading: row.get(2)?,
                content: row.get(3)?,
                token_estimate: row.get(4)?,
                rank: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub struct DocChunkMatch {
    pub file_path: String,
    pub chunk_index: u32,
    pub heading: Option<String>,
    pub content: String,
    pub token_estimate: u32,
    pub rank: f64,
}

// ── Daemon Meta ──

pub fn set_meta(conn: &Connection, key: &str, value: &str) -> DbResult<()> {
    conn.execute(
        "INSERT INTO daemon_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_meta(conn: &Connection, key: &str) -> DbResult<Option<String>> {
    let result = conn
        .query_row(
            "SELECT value FROM daemon_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(result)
}

// ── Token Log ──

#[allow(clippy::too_many_arguments)]
pub fn insert_token_log(
    conn: &Connection,
    timestamp: i64,
    session_id: &str,
    branch: &str,
    tool_name: &str,
    query: Option<&str>,
    intent: Option<&str>,
    tokens_actual: u32,
    tokens_estimated: u32,
    files_touched: Option<&str>,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO token_log (timestamp, session_id, branch, tool_name, query, intent, tokens_actual, tokens_estimated, files_touched)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![timestamp, session_id, branch, tool_name, query, intent, tokens_actual, tokens_estimated, files_touched],
    )?;
    Ok(())
}

// ── Node loading (bulk) ──

pub struct NodeRow {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub signature: String,
    pub signature_hash: String,
    pub docstring: Option<String>,
    pub skeleton: String,
    pub centrality: f64,
    pub checksum: Vec<u8>,
}

pub fn load_all_nodes(conn: &Connection) -> DbResult<Vec<NodeRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, name, file_path, line_start, line_end, signature, signature_hash, docstring, skeleton, centrality, checksum FROM nodes",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(NodeRow {
                id: row.get(0)?,
                kind: row.get(1)?,
                name: row.get(2)?,
                file_path: row.get(3)?,
                line_start: row.get(4)?,
                line_end: row.get(5)?,
                signature: row.get(6)?,
                signature_hash: row.get(7)?,
                docstring: row.get(8)?,
                skeleton: row.get(9)?,
                centrality: row.get(10)?,
                checksum: row.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ── Node Versions ──

#[allow(clippy::too_many_arguments)]
pub fn insert_node_version(
    conn: &Connection,
    symbol_hash: &str,
    version_num: u32,
    file_path: &str,
    session_id: Option<&str>,
    node_kind: &str,
    signature: &str,
    signature_hash: &str,
    edges_json: &str,
    body_hash: Option<&[u8]>,
    created_at: i64,
) -> DbResult<()> {
    conn.execute(
        "INSERT INTO node_versions (symbol_hash, version_num, file_path, session_id, node_kind, signature, signature_hash, edges_json, body_hash, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![symbol_hash, version_num, file_path, session_id, node_kind, signature, signature_hash, edges_json, body_hash, created_at],
    )?;
    Ok(())
}

pub fn get_latest_version_num(conn: &Connection, symbol_hash: &str) -> DbResult<Option<u32>> {
    let result = conn
        .query_row(
            "SELECT MAX(version_num) FROM node_versions WHERE symbol_hash = ?1",
            params![symbol_hash],
            |row| row.get(0),
        )
        .optional()?;
    Ok(result.flatten())
}

pub fn prune_old_versions(conn: &Connection, symbol_hash: &str, keep: u32) -> DbResult<u64> {
    let deleted = conn.execute(
        "DELETE FROM node_versions WHERE symbol_hash = ?1 AND version_num <= (
            SELECT version_num FROM node_versions WHERE symbol_hash = ?1
            ORDER BY version_num DESC LIMIT 1 OFFSET ?2
        )",
        params![symbol_hash, keep],
    )?;
    Ok(deleted as u64)
}

pub fn get_node_signature_hash(conn: &Connection, node_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT signature_hash FROM nodes WHERE id = ?1",
        params![node_id],
        |row| row.get(0),
    )
    .optional()
    .ok()
    .flatten()
}

pub fn get_doc_chunks_for_file(conn: &Connection, file_name: &str) -> DbResult<Vec<DocChunkMatch>> {
    let pattern = format!("%{file_name}");
    let mut stmt = conn.prepare(
        "SELECT file_path, chunk_index, heading, content, token_estimate, 0.0 AS rank
         FROM doc_chunks WHERE file_path LIKE ?1
         ORDER BY chunk_index LIMIT 10",
    )?;
    let rows = stmt
        .query_map(params![pattern], |row| {
            Ok(DocChunkMatch {
                file_path: row.get(0)?,
                chunk_index: row.get(1)?,
                heading: row.get(2)?,
                content: row.get(3)?,
                token_estimate: row.get(4)?,
                rank: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_project_level_annotations(conn: &Connection) -> DbResult<Vec<AnnotationRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, text, tags, stale, created_at, updated_at, kind, quality, retrieval_count
         FROM annotations WHERE anchor_type IS NULL",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(AnnotationRow {
                id: row.get(0)?,
                text: row.get(1)?,
                tags: row.get(2)?,
                stale: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                kind: row.get(6)?,
                quality: row.get(7)?,
                retrieval_count: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
