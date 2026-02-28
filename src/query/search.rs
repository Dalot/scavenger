use rusqlite::Connection;

use crate::db::DbResult;
use crate::graph::GraphState;
use crate::graph::types::NodeId;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub node_id: NodeId,
    pub bm25_score: f64,
    pub centrality: f64,
    pub combined_score: f64,
}

/// Search nodes via FTS5 BM25 and compose with centrality from the in-memory graph.
/// Combined score: 0.6 * normalize(bm25) + 0.4 * normalize(centrality)
pub fn search(
    conn: &Connection,
    graph: &GraphState,
    query: &str,
    limit: u32,
) -> DbResult<Vec<SearchResult>> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let sanitized = sanitize_fts_query(query);
    if sanitized.is_empty() {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT n.id, bm25(nodes_fts) AS bm25_score
         FROM nodes_fts
         JOIN nodes n ON nodes_fts.rowid = n._rowid
         WHERE nodes_fts MATCH ?1
         ORDER BY bm25(nodes_fts)
         LIMIT ?2",
    )?;

    let raw_results: Vec<(String, f64)> = stmt
        .query_map(rusqlite::params![sanitized, limit], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if raw_results.is_empty() {
        return Ok(Vec::new());
    }

    // Sign-flip normalization: FTS5 bm25() returns negative scores (lower = better)
    let max_magnitude = raw_results
        .iter()
        .map(|(_, s)| s.abs())
        .fold(f64::MIN, f64::max)
        .max(1e-10);

    let mut results: Vec<SearchResult> = raw_results
        .into_iter()
        .map(|(id, bm25_raw)| {
            let node_id = NodeId(id);
            let bm25_normalized = -bm25_raw / max_magnitude;
            let centrality = graph
                .get_weight(&node_id)
                .map(|w| w.centrality as f64)
                .unwrap_or(0.0);
            SearchResult {
                node_id,
                bm25_score: bm25_normalized,
                centrality,
                combined_score: 0.0,
            }
        })
        .collect();

    // Normalize centrality across result set
    let max_centrality = results
        .iter()
        .map(|r| r.centrality)
        .fold(f64::MIN, f64::max)
        .max(1e-10);

    for r in &mut results {
        let norm_centrality = r.centrality / max_centrality;
        r.combined_score = 0.6 * r.bm25_score + 0.4 * norm_centrality;
    }

    results.sort_by(|a, b| {
        b.combined_score
            .partial_cmp(&a.combined_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(results)
}

/// Sanitize a query for FTS5 MATCH: strip special characters that break FTS5 syntax.
fn sanitize_fts_query(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for ch in query.chars() {
        match ch {
            '"' | '\'' | '(' | ')' | '*' | '+' | '-' | ':' | '^' | '{' | '}' => {
                out.push(' ');
            }
            _ => out.push(ch),
        }
    }
    let trimmed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_fts_query() {
        assert_eq!(sanitize_fts_query("hello world"), "hello world");
        assert_eq!(sanitize_fts_query("fn(x: i32)"), "fn x i32");
        assert_eq!(sanitize_fts_query("***"), "");
    }

    #[test]
    fn test_search_empty_query() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();
        let graph = GraphState::new();
        let results = search(&conn, &graph, "", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_with_results() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum)
             VALUES ('n1', 'Function', 'getUserById', 'src/users.rs', 10, 20, 'fn get_user_by_id(id: u32)', 'abcdef01', 'fn get_user_by_id(id: u32)', X'DEADBEEF')",
            [],
        ).unwrap();

        let mut graph = GraphState::new();
        graph.load_from_db(&conn).unwrap();

        let results = search(&conn, &graph, "getUserById", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].node_id.0, "n1");
    }
}
