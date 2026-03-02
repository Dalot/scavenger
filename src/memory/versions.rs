#![allow(dead_code)]
use rusqlite::Connection;
use serde_json;

use crate::db::queries;
use crate::graph::index::ExtractedSymbol;
use crate::graph::types::NodeId;

/// Record a new version snapshot for a symbol after re-index.
/// Captures kind, signature, signature_hash, edges as JSON, body hash.
/// Retains last 5 per symbol (ordinal decay).
pub fn record_version(
    conn: &Connection,
    symbol: &ExtractedSymbol,
    session_id: Option<&str>,
    connected_edges: &[(NodeId, String)],
) -> Result<(), Box<dyn std::error::Error>> {
    let symbol_hash = &symbol.signature_hash;

    let next_version = queries::get_latest_version_num(conn, symbol_hash)?
        .map(|v| v + 1)
        .unwrap_or(1);

    let edges_json = serde_json::to_string(
        &connected_edges
            .iter()
            .map(|(id, kind)| serde_json::json!({"target": id.0, "kind": kind}))
            .collect::<Vec<_>>(),
    )?;

    queries::insert_node_version(
        conn,
        symbol_hash,
        next_version,
        &symbol.file_path,
        session_id,
        symbol.kind.as_str(),
        &symbol.signature,
        &symbol.signature_hash,
        &edges_json,
        Some(&symbol.checksum),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
    )?;

    // Ordinal decay: keep last 5
    queries::prune_old_versions(conn, symbol_hash, 5)?;

    Ok(())
}

/// Record versions for all symbols in a batch (after bulk re-index).
pub fn record_versions_batch(
    conn: &Connection,
    symbols: &[ExtractedSymbol],
    session_id: Option<&str>,
) -> Result<u64, Box<dyn std::error::Error>> {
    let mut count = 0u64;
    for sym in symbols {
        record_version(conn, sym, session_id, &[])?;
        count += 1;
    }
    Ok(count)
}

/// Lookup recent versions for a symbol (by signature_hash).
/// Returns up to `limit` most recent versions.
pub fn get_recent_versions(
    conn: &Connection,
    signature_hash: &str,
    limit: u32,
) -> Result<Vec<VersionInfo>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT version_num, file_path, session_id, node_kind, signature, edges_json, created_at
         FROM node_versions WHERE symbol_hash = ?1
         ORDER BY version_num DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![signature_hash, limit], |row| {
            Ok(VersionInfo {
                version_num: row.get(0)?,
                file_path: row.get(1)?,
                session_id: row.get(2)?,
                node_kind: row.get(3)?,
                signature: row.get(4)?,
                edges_json: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Debug, Clone)]
pub struct VersionInfo {
    pub version_num: u32,
    pub file_path: String,
    pub session_id: Option<String>,
    pub node_kind: String,
    pub signature: String,
    pub edges_json: String,
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::NodeKind;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();
        conn
    }

    fn make_symbol(name: &str) -> ExtractedSymbol {
        ExtractedSymbol {
            id: NodeId(format!("id_{name}")),
            kind: NodeKind::Function,
            name: name.to_string(),
            file_path: "test.rs".to_string(),
            line_start: 1,
            line_end: 10,
            signature: format!("fn {name}()"),
            signature_hash: format!("hash_{name}"),
            docstring: None,
            skeleton: format!("fn {name}()"),
            checksum: vec![0xDE, 0xAD],
        }
    }

    #[test]
    fn test_record_and_retrieve_version() {
        let conn = setup_db();
        let sym = make_symbol("foo");
        record_version(&conn, &sym, Some("sess1"), &[]).unwrap();

        let versions = get_recent_versions(&conn, "hash_foo", 5).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version_num, 1);
        assert_eq!(versions[0].signature, "fn foo()");
    }

    #[test]
    fn test_ordinal_decay_keeps_last_5() {
        let conn = setup_db();
        let sym = make_symbol("bar");
        for _ in 0..8 {
            record_version(&conn, &sym, None, &[]).unwrap();
        }
        let versions = get_recent_versions(&conn, "hash_bar", 10).unwrap();
        assert!(
            versions.len() <= 5,
            "should keep at most 5, got {}",
            versions.len()
        );
    }
}
