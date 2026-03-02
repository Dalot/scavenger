#![allow(dead_code)]
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::db::queries;

/// Anchor types for annotations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnchorType {
    Node,
    File,
    Scope,
    Project,
}

impl AnchorType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::File => "file",
            Self::Scope => "scope",
            Self::Project => "project",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "node" => Some(Self::Node),
            "file" => Some(Self::File),
            "scope" => Some(Self::Scope),
            "project" | "" => Some(Self::Project),
            _ => None,
        }
    }
}

/// Annotation kinds inspired by ALMA memory layers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnnotationKind {
    Fact,
    Strategy,
    Pitfall,
    Context,
}

impl AnnotationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fact => "fact",
            Self::Strategy => "strategy",
            Self::Pitfall => "pitfall",
            Self::Context => "context",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s {
            "strategy" => Self::Strategy,
            "pitfall" => Self::Pitfall,
            "context" => Self::Context,
            _ => Self::Fact,
        }
    }
}

/// Create or update an annotation.
/// If `id` exists, updates text/tags/kind. Otherwise checks for content-hash
/// dedup, then creates a new one. Returns the annotation ID (which may differ
/// from `id` if a duplicate was found).
pub fn upsert_annotation(
    conn: &Connection,
    id: &str,
    anchor_type: Option<AnchorType>,
    anchor_value: Option<&str>,
    text: &str,
    tags: Option<&str>,
    kind: AnnotationKind,
) -> Result<UpsertResult, Box<dyn std::error::Error>> {
    let now = now_secs();
    let at_str = anchor_type.map(|a| a.as_str());
    let hash = compute_content_hash(at_str, anchor_value, text);

    // Content-hash dedup: if identical annotation exists, bump quality and return it
    if let Ok(Some(existing_id)) = queries::find_annotation_by_content_hash(conn, &hash) {
        let _ = queries::update_annotation_quality(conn, &existing_id, 0.1);
        return Ok(UpsertResult {
            id: existing_id,
            deduplicated: true,
        });
    }

    // Try update by ID first
    let updated = conn.execute(
        "UPDATE annotations SET text = ?1, tags = ?2, stale = FALSE, updated_at = ?3, kind = ?5, content_hash = ?6 WHERE id = ?4",
        rusqlite::params![text, tags, now, id, kind.as_str(), hash],
    )?;

    if updated == 0 {
        queries::insert_annotation(
            conn,
            id,
            at_str,
            anchor_value,
            text,
            tags,
            kind.as_str(),
            Some(&hash),
            now,
        )?;
    }

    Ok(UpsertResult {
        id: id.to_string(),
        deduplicated: false,
    })
}

#[derive(Debug, Clone)]
pub struct UpsertResult {
    pub id: String,
    pub deduplicated: bool,
}

/// Read annotations by anchor.
pub fn read_by_anchor(
    conn: &Connection,
    anchor_type: &str,
    anchor_value: &str,
) -> Result<Vec<AnnotationView>, Box<dyn std::error::Error>> {
    let rows = queries::get_annotations_for_anchor(conn, anchor_type, anchor_value)?;
    Ok(rows
        .into_iter()
        .map(|r| AnnotationView {
            id: r.id,
            text: r.text,
            tags: r.tags,
            stale: r.stale,
            created_at: r.created_at,
            updated_at: r.updated_at,
            kind: r.kind,
            quality: r.quality,
        })
        .collect())
}

/// Delete an annotation by ID. Also cleans up any relationship edges.
pub fn delete_annotation(conn: &Connection, id: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let _ = queries::delete_annotation_edges(conn, id);
    let deleted = conn.execute(
        "DELETE FROM annotations WHERE id = ?1",
        rusqlite::params![id],
    )?;
    Ok(deleted > 0)
}

/// Detect staleness for node-anchored annotations by checking if the
/// node's checksum has changed since the annotation was last updated.
pub fn detect_staleness_for_node(
    conn: &Connection,
    node_id: &str,
    current_checksum: &[u8],
) -> Result<u64, Box<dyn std::error::Error>> {
    let stored_checksum: Option<Vec<u8>> = conn
        .query_row(
            "SELECT checksum FROM nodes WHERE id = ?1",
            rusqlite::params![node_id],
            |row| row.get(0),
        )
        .ok();

    if let Some(stored) = stored_checksum {
        if stored != current_checksum {
            return Ok(queries::mark_annotations_stale_for_node(conn, node_id)?);
        }
    }
    Ok(0)
}

/// Detect staleness for file-anchored annotations by mtime.
pub fn detect_staleness_for_file(
    conn: &Connection,
    file_path: &str,
) -> Result<u64, Box<dyn std::error::Error>> {
    let fs_mtime = std::fs::metadata(file_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let changed = conn.execute(
        "UPDATE annotations SET stale = TRUE, updated_at = ?3
         WHERE anchor_type = 'file' AND anchor_value = ?1 AND updated_at < ?2",
        rusqlite::params![file_path, fs_mtime, now_secs()],
    )?;
    Ok(changed as u64)
}

/// Orphan cleanup: delete node-anchored annotations where the NodeId
/// no longer exists and the annotation has been stale for >30 days.
pub fn cleanup_orphans(conn: &Connection) -> Result<u64, Box<dyn std::error::Error>> {
    let cutoff = now_secs() - 30 * 86400;
    let deleted = conn.execute(
        "DELETE FROM annotations WHERE anchor_type = 'node'
         AND stale = TRUE AND updated_at < ?1
         AND anchor_value NOT IN (SELECT id FROM nodes)",
        rusqlite::params![cutoff],
    )?;
    Ok(deleted as u64)
}

/// Search annotations via FTS5.
pub fn search_fts(
    conn: &Connection,
    query: &str,
    limit: u32,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let matches = queries::search_annotations_fts(conn, query, limit)?;
    Ok(matches.into_iter().map(|m| m.id).collect())
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn compute_content_hash(
    anchor_type: Option<&str>,
    anchor_value: Option<&str>,
    text: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(anchor_type.unwrap_or(""));
    hasher.update("|");
    hasher.update(anchor_value.unwrap_or(""));
    hasher.update("|");
    hasher.update(normalize_text(text));
    format!("{:x}", hasher.finalize())
}

/// Decay quality for all annotations anchored to a node (called on THRASHING/DEAD_END).
pub fn decay_quality_for_node(
    conn: &Connection,
    node_id: &str,
    factor: f64,
) -> Result<u64, Box<dyn std::error::Error>> {
    let changed = conn.execute(
        "UPDATE annotations SET quality = quality * ?1
         WHERE anchor_type = 'node' AND anchor_value = ?2",
        rusqlite::params![factor, node_id],
    )?;
    Ok(changed as u64)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, Clone)]
pub struct AnnotationView {
    pub id: String,
    pub text: String,
    pub tags: Option<String>,
    pub stale: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub kind: String,
    pub quality: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_create_and_read_annotation() {
        let conn = setup_db();
        upsert_annotation(
            &conn,
            "a1",
            Some(AnchorType::Node),
            Some("nid"),
            "test note",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        let annotations = read_by_anchor(&conn, "node", "nid").unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].text, "test note");
        assert!(!annotations[0].stale);
    }

    #[test]
    fn test_update_annotation() {
        let conn = setup_db();
        upsert_annotation(
            &conn,
            "a1",
            Some(AnchorType::File),
            Some("/test.rs"),
            "v1",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        upsert_annotation(
            &conn,
            "a1",
            Some(AnchorType::File),
            Some("/test.rs"),
            "v2",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        let annotations = read_by_anchor(&conn, "file", "/test.rs").unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].text, "v2");
    }

    #[test]
    fn test_delete_annotation() {
        let conn = setup_db();
        upsert_annotation(
            &conn,
            "a1",
            Some(AnchorType::Project),
            None,
            "note",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        assert!(delete_annotation(&conn, "a1").unwrap());
        assert!(!delete_annotation(&conn, "a1").unwrap());
    }

    #[test]
    fn test_kind_defaults_to_fact() {
        let conn = setup_db();
        upsert_annotation(
            &conn,
            "a1",
            Some(AnchorType::Node),
            Some("n1"),
            "note",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        let annotations = read_by_anchor(&conn, "node", "n1").unwrap();
        assert_eq!(annotations[0].kind, "fact");
    }

    #[test]
    fn test_kind_round_trip() {
        let conn = setup_db();
        upsert_annotation(
            &conn,
            "a1",
            Some(AnchorType::Node),
            Some("n1"),
            "watch out!",
            None,
            AnnotationKind::Pitfall,
        )
        .unwrap();
        let annotations = read_by_anchor(&conn, "node", "n1").unwrap();
        assert_eq!(annotations[0].kind, "pitfall");
    }

    #[test]
    fn test_kind_from_str_unknown_defaults() {
        assert_eq!(AnnotationKind::from_str("bogus"), AnnotationKind::Fact);
        assert_eq!(
            AnnotationKind::from_str("strategy"),
            AnnotationKind::Strategy
        );
    }

    #[test]
    fn test_content_hash_deterministic() {
        let h1 = compute_content_hash(Some("node"), Some("n1"), "hello world");
        let h2 = compute_content_hash(Some("node"), Some("n1"), "hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_content_hash_normalization() {
        let h1 = compute_content_hash(Some("node"), Some("n1"), "Hello  World");
        let h2 = compute_content_hash(Some("node"), Some("n1"), "hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_dedup_returns_existing_id() {
        let conn = setup_db();
        let r1 = upsert_annotation(
            &conn,
            "a1",
            Some(AnchorType::Node),
            Some("n1"),
            "same note",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        let r2 = upsert_annotation(
            &conn,
            "a2",
            Some(AnchorType::Node),
            Some("n1"),
            "same note",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        assert!(!r1.deduplicated);
        assert!(r2.deduplicated);
        assert_eq!(r2.id, "a1");
    }

    #[test]
    fn test_dedup_different_text_not_deduped() {
        let conn = setup_db();
        let r1 = upsert_annotation(
            &conn,
            "a1",
            Some(AnchorType::Node),
            Some("n1"),
            "note A",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        let r2 = upsert_annotation(
            &conn,
            "a2",
            Some(AnchorType::Node),
            Some("n1"),
            "note B",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        assert!(!r1.deduplicated);
        assert!(!r2.deduplicated);
        let annotations = read_by_anchor(&conn, "node", "n1").unwrap();
        assert_eq!(annotations.len(), 2);
    }

    #[test]
    fn test_dedup_bumps_quality() {
        let conn = setup_db();
        upsert_annotation(
            &conn,
            "a1",
            Some(AnchorType::Node),
            Some("n1"),
            "repeated",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        let before = read_by_anchor(&conn, "node", "n1").unwrap();
        assert!((before[0].quality - 0.5).abs() < f64::EPSILON);

        upsert_annotation(
            &conn,
            "a2",
            Some(AnchorType::Node),
            Some("n1"),
            "repeated",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        let after = read_by_anchor(&conn, "node", "n1").unwrap();
        assert!((after[0].quality - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn test_quality_decay() {
        let conn = setup_db();
        upsert_annotation(
            &conn,
            "a1",
            Some(AnchorType::Node),
            Some("n1"),
            "note",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        decay_quality_for_node(&conn, "n1", 0.9).unwrap();
        let annotations = read_by_anchor(&conn, "node", "n1").unwrap();
        assert!((annotations[0].quality - 0.45).abs() < f64::EPSILON);
    }

    #[test]
    fn test_quality_clamped_at_1() {
        let conn = setup_db();
        upsert_annotation(
            &conn,
            "a1",
            Some(AnchorType::Node),
            Some("n1"),
            "note",
            None,
            AnnotationKind::Fact,
        )
        .unwrap();
        for _ in 0..20 {
            crate::db::queries::update_annotation_quality(&conn, "a1", 0.1).unwrap();
        }
        let annotations = read_by_anchor(&conn, "node", "n1").unwrap();
        assert!(annotations[0].quality <= 1.0);
    }
}
