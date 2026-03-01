pub mod annotations;
pub mod antipattern;
pub mod session;
pub mod signals;
pub mod versions;

use rusqlite::Connection;

use crate::graph::index::ExtractedSymbol;
use crate::graph::GraphState;

/// Three-layer memory orchestrator.
///
/// Layer 1: Node version history (versions.rs)
/// Layer 2: Annotations with anchoring (annotations.rs)
/// Layer 3: Behavioral signals + session activity (signals.rs, session.rs)
pub struct MemoryManager {
    #[allow(dead_code)]
    pub detector: antipattern::AntiPatternDetector,
}

#[allow(dead_code)]
impl MemoryManager {
    pub fn new() -> Self {
        Self {
            detector: antipattern::AntiPatternDetector::new(),
        }
    }

    /// Called after a successful re-index of a file.
    /// Records new versions and checks annotation staleness.
    pub fn on_reindex(
        &mut self,
        conn: &Connection,
        graph: &GraphState,
        session_id: &str,
        symbols: &[ExtractedSymbol],
    ) {
        // Layer 1: Record versions
        let _ = versions::record_versions_batch(conn, symbols, Some(session_id));

        // Layer 2: Check staleness for each symbol
        for sym in symbols {
            let _ = annotations::detect_staleness_for_node(conn, &sym.id.0, &sym.checksum);
        }

        // Layer 3: Run anti-pattern checks for each symbol
        for sym in symbols {
            let context = antipattern::DetectorContext {
                node_id: Some(sym.id.0.clone()),
                file_path: Some(sym.file_path.clone()),
                edge_from: None,
                edge_to: None,
                new_checksum: Some(sym.checksum.clone()),
                action_count: 0,
            };
            self.detector.check_all(conn, graph, session_id, &context);
        }
    }

    /// Periodic maintenance: prune expired signals and sessions, clean orphan annotations.
    pub fn maintenance(&self, conn: &Connection) {
        let _ = signals::prune_expired(conn);
        let _ = session::prune_expired(conn);
        let _ = annotations::cleanup_orphans(conn);
    }

    /// Fork annotations from a parent branch DB for cold start.
    pub fn fork_annotations(
        parent_conn: &Connection,
        child_conn: &Connection,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let mut stmt = parent_conn.prepare(
            "SELECT id, anchor_type, anchor_value, text, tags, created_at, updated_at, stale
             FROM annotations",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, bool>(7)?,
            ))
        })?;

        let mut count = 0u64;
        for row in rows {
            let (id, at, av, text, tags, created, updated, stale) = row?;
            child_conn.execute(
                "INSERT OR IGNORE INTO annotations
                 (id, anchor_type, anchor_value, text, tags, created_at, updated_at, stale)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![id, at, av, text, tags, created, updated, stale],
            )?;
            count += 1;
        }
        Ok(count)
    }

    /// Union-merge annotations from a source branch into the target.
    /// Same anchor+text = dedup, different text = keep both.
    pub fn merge_annotations(
        source_conn: &Connection,
        target_conn: &Connection,
    ) -> Result<MergeResult, Box<dyn std::error::Error>> {
        let mut stmt = source_conn.prepare(
            "SELECT id, anchor_type, anchor_value, text, tags, created_at, updated_at, stale
             FROM annotations",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, bool>(7)?,
            ))
        })?;

        let mut imported = 0u64;
        let mut deduped = 0u64;

        for row in rows {
            let (id, at, av, text, tags, created, updated, stale) = row?;

            // Check for duplicate (same anchor + text)
            let exists: bool = target_conn
                .query_row(
                    "SELECT COUNT(*) FROM annotations
                     WHERE anchor_type IS ?1 AND anchor_value IS ?2 AND text = ?3",
                    rusqlite::params![at, av, text],
                    |row| row.get::<_, i64>(0).map(|c| c > 0),
                )
                .unwrap_or(false);

            if exists {
                // Rule 1: same anchor + same text → skip
                deduped += 1;
            } else {
                // Rule 3: only import node-anchored annotations if the NodeId exists in target
                if at.as_deref() == Some("node") {
                    if let Some(ref node_id) = av {
                        let node_exists: bool = target_conn
                            .query_row(
                                "SELECT COUNT(*) FROM nodes WHERE id = ?1",
                                rusqlite::params![node_id],
                                |row| row.get::<_, i64>(0).map(|c| c > 0),
                            )
                            .unwrap_or(false);
                        if !node_exists {
                            continue;
                        }
                    }
                }

                // Rule 2: same anchor + different text → keep both (new ID if clash)
                let new_id = if target_conn
                    .query_row(
                        "SELECT COUNT(*) FROM annotations WHERE id = ?1",
                        rusqlite::params![id],
                        |row| row.get::<_, i64>(0).map(|c| c > 0),
                    )
                    .unwrap_or(false)
                {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    id
                };

                target_conn.execute(
                    "INSERT INTO annotations
                     (id, anchor_type, anchor_value, text, tags, created_at, updated_at, stale)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![new_id, at, av, text, tags, created, updated, stale],
                )?;
                imported += 1;
            }
        }

        Ok(MergeResult { imported, deduped })
    }
}

#[derive(Debug)]
pub struct MergeResult {
    pub imported: u64,
    pub deduped: u64,
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
    fn test_fork_annotations() {
        let parent = setup_db();
        let child = setup_db();

        crate::db::queries::insert_annotation(
            &parent, "a1", Some("node"), Some("n1"), "note", None, 1000,
        ).unwrap();

        let count = MemoryManager::fork_annotations(&parent, &child).unwrap();
        assert_eq!(count, 1);

        let annotations = annotations::read_by_anchor(&child, "node", "n1").unwrap();
        assert_eq!(annotations.len(), 1);
    }

    #[test]
    fn test_merge_annotations_dedup() {
        let source = setup_db();
        let target = setup_db();

        crate::db::queries::insert_annotation(
            &source, "a1", Some("node"), Some("n1"), "same note", None, 1000,
        ).unwrap();
        crate::db::queries::insert_annotation(
            &target, "a2", Some("node"), Some("n1"), "same note", None, 1000,
        ).unwrap();

        let result = MemoryManager::merge_annotations(&source, &target).unwrap();
        assert_eq!(result.deduped, 1);
        assert_eq!(result.imported, 0);
    }

    #[test]
    fn test_merge_annotations_different_text() {
        let source = setup_db();
        let target = setup_db();

        // Node must exist in target for Rule 3 (node-anchored annotations import only if NodeId exists)
        target.execute(
            "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum)
             VALUES ('n1', 'Function', 'hello', 'src/lib.rs', 1, 5, 'fn hello()', 'aabb0011', 'fn hello()', X'CAFE')",
            [],
        ).unwrap();

        crate::db::queries::insert_annotation(
            &source, "a1", Some("node"), Some("n1"), "source note", None, 1000,
        ).unwrap();
        crate::db::queries::insert_annotation(
            &target, "a2", Some("node"), Some("n1"), "target note", None, 1000,
        ).unwrap();

        let result = MemoryManager::merge_annotations(&source, &target).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.deduped, 0);

        let annotations = annotations::read_by_anchor(&target, "node", "n1").unwrap();
        assert_eq!(annotations.len(), 2);
    }

    #[test]
    fn test_merge_skips_annotations_for_missing_nodes() {
        let source = setup_db();
        let target = setup_db();

        // Source has an annotation for "n_missing" which does NOT exist in target's nodes table
        crate::db::queries::insert_annotation(
            &source, "a1", Some("node"), Some("n_missing"), "orphan note", None, 1000,
        ).unwrap();

        let result = MemoryManager::merge_annotations(&source, &target).unwrap();
        assert_eq!(result.imported, 0, "should skip annotations for missing nodes");
        assert_eq!(result.deduped, 0);
    }
}
