use rusqlite::Connection;

/// Estimate tokens that would be consumed WITHOUT the index (naive approach).
/// Per-tool estimates based on raw_token_estimate from the files table.
pub fn estimate_without_index(
    conn: &Connection,
    tool_name: &str,
    file_path: Option<&str>,
) -> u32 {
    match tool_name {
        "get_capsule" => estimate_capsule_without_index(conn, file_path),
        "search_docs" => estimate_search_docs_without_index(conn),
        "read_annotations" => estimate_read_annotations_without_index(conn, file_path),
        "write_annotation" | "delete_annotation" => 0,
        _ => 0,
    }
}

/// Capsule without index: seed file + 1-hop neighbor raw_token_estimate
fn estimate_capsule_without_index(conn: &Connection, file_path: Option<&str>) -> u32 {
    let Some(fp) = file_path else { return 0 };

    let seed_estimate: u32 = conn
        .query_row(
            "SELECT raw_token_estimate FROM files WHERE file_path = ?1",
            rusqlite::params![fp],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // Estimate 1-hop neighbors: sum of raw_token_estimate for files that have
    // symbols connected to symbols in the seed file
    let neighbor_estimate: u32 = conn
        .query_row(
            "SELECT COALESCE(SUM(f.raw_token_estimate), 0)
             FROM files f
             WHERE f.file_path IN (
                 SELECT DISTINCT n2.file_path FROM edges e
                 JOIN nodes n1 ON e.from_id = n1.id
                 JOIN nodes n2 ON e.to_id = n2.id
                 WHERE n1.file_path = ?1 AND n2.file_path != ?1
                 UNION
                 SELECT DISTINCT n1.file_path FROM edges e
                 JOIN nodes n1 ON e.from_id = n1.id
                 JOIN nodes n2 ON e.to_id = n2.id
                 WHERE n2.file_path = ?1 AND n1.file_path != ?1
             )",
            rusqlite::params![fp],
            |row| row.get(0),
        )
        .unwrap_or(0);

    seed_estimate + neighbor_estimate
}

/// Search docs without index: sum of all doc file estimates
fn estimate_search_docs_without_index(conn: &Connection) -> u32 {
    conn.query_row(
        "SELECT COALESCE(SUM(raw_token_estimate), 0) FROM files WHERE file_type = 'doc'",
        [],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

/// Read annotations without index: anchor file estimate
fn estimate_read_annotations_without_index(conn: &Connection, file_path: Option<&str>) -> u32 {
    let Some(fp) = file_path else { return 0 };
    conn.query_row(
        "SELECT raw_token_estimate FROM files WHERE file_path = ?1",
        rusqlite::params![fp],
        |row| row.get(0),
    )
    .unwrap_or(0)
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
    fn test_estimate_unknown_tool_returns_zero() {
        let conn = setup_db();
        assert_eq!(estimate_without_index(&conn, "unknown_tool", None), 0);
    }

    #[test]
    fn test_estimate_write_delete_returns_zero() {
        let conn = setup_db();
        assert_eq!(estimate_without_index(&conn, "write_annotation", None), 0);
        assert_eq!(estimate_without_index(&conn, "delete_annotation", None), 0);
    }

    #[test]
    fn test_estimate_capsule_with_file() {
        let conn = setup_db();
        crate::db::queries::upsert_file(&conn, "src/main.rs", "code", 500, 1000).unwrap();
        let est = estimate_without_index(&conn, "get_capsule", Some("src/main.rs"));
        assert!(est >= 500);
    }
}
