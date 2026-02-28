use rusqlite::Connection;

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

/// Create or update an annotation.
/// If `id` exists, updates text/tags. Otherwise creates a new one.
pub fn upsert_annotation(
    conn: &Connection,
    id: &str,
    anchor_type: Option<AnchorType>,
    anchor_value: Option<&str>,
    text: &str,
    tags: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = now_secs();
    let at_str = anchor_type.map(|a| a.as_str());

    // Try update first
    let updated = conn.execute(
        "UPDATE annotations SET text = ?1, tags = ?2, stale = FALSE, updated_at = ?3 WHERE id = ?4",
        rusqlite::params![text, tags, now, id],
    )?;

    if updated == 0 {
        queries::insert_annotation(conn, id, at_str, anchor_value, text, tags, now)?;
    }

    Ok(())
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
        })
        .collect())
}

/// Delete an annotation by ID.
pub fn delete_annotation(conn: &Connection, id: &str) -> Result<bool, Box<dyn std::error::Error>> {
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
        upsert_annotation(&conn, "a1", Some(AnchorType::Node), Some("nid"), "test note", None).unwrap();
        let annotations = read_by_anchor(&conn, "node", "nid").unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].text, "test note");
        assert!(!annotations[0].stale);
    }

    #[test]
    fn test_update_annotation() {
        let conn = setup_db();
        upsert_annotation(&conn, "a1", Some(AnchorType::File), Some("/test.rs"), "v1", None).unwrap();
        upsert_annotation(&conn, "a1", Some(AnchorType::File), Some("/test.rs"), "v2", None).unwrap();
        let annotations = read_by_anchor(&conn, "file", "/test.rs").unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].text, "v2");
    }

    #[test]
    fn test_delete_annotation() {
        let conn = setup_db();
        upsert_annotation(&conn, "a1", Some(AnchorType::Project), None, "note", None).unwrap();
        assert!(delete_annotation(&conn, "a1").unwrap());
        assert!(!delete_annotation(&conn, "a1").unwrap());
    }
}
