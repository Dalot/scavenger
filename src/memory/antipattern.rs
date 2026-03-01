#![allow(dead_code)]
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::Instant;

use rusqlite::Connection;

use petgraph::visit::EdgeRef;

use crate::graph::types::NodeId;
use crate::graph::GraphState;
use crate::memory::signals::{self, SignalKind};

/// Dedup key: (signal_kind_str, entity_key)
type DeduplicationSet = HashSet<(String, String)>;

/// Thrashing ring buffer entry.
struct EditEntry {
    timestamp: Instant,
    checksum: Vec<u8>,
}

/// Anti-pattern detector state. Lives in the daemon for the duration of a session.
pub struct AntiPatternDetector {
    fired: DeduplicationSet,
    thrashing_buffer: HashMap<String, VecDeque<EditEntry>>,
    failed_search_counts: HashMap<String, u32>,
}

impl AntiPatternDetector {
    pub fn new() -> Self {
        Self {
            fired: HashSet::new(),
            thrashing_buffer: HashMap::new(),
            failed_search_counts: HashMap::new(),
        }
    }

    /// Run all detectors. Called after a re-index or session event.
    pub fn check_all(
        &mut self,
        conn: &Connection,
        graph: &GraphState,
        session_id: &str,
        context: &DetectorContext,
    ) {
        if let Some(ref node_id) = context.node_id {
            self.check_thrashing(conn, session_id, node_id, context);
            self.check_dead_end(conn, graph, session_id, node_id, context);
            self.check_large_blast_radius(conn, graph, session_id, node_id);
            self.check_untested(conn, graph, session_id, node_id);
        }

        if let Some(ref from) = context.edge_from {
            if let Some(ref to) = context.edge_to {
                self.check_cycle_introduced(conn, graph, session_id, from, to);
            }
        }

        if let Some(ref file_path) = context.file_path {
            self.check_index_blind_spot(conn, graph, session_id, file_path);
        }
    }

    /// THRASHING: >=3 edits in 5min with Levenshtein similarity >0.9.
    fn check_thrashing(
        &mut self,
        conn: &Connection,
        session_id: &str,
        node_id: &str,
        context: &DetectorContext,
    ) {
        let key = ("THRASHING".to_string(), node_id.to_string());
        if self.fired.contains(&key) {
            return;
        }

        let buffer = self.thrashing_buffer.entry(node_id.to_string()).or_default();

        if let Some(ref checksum) = context.new_checksum {
            buffer.push_back(EditEntry {
                timestamp: Instant::now(),
                checksum: checksum.clone(),
            });
        }

        // Evict entries older than 5 minutes
        let five_min_ago = Instant::now() - std::time::Duration::from_secs(300);
        while buffer.front().is_some_and(|e| e.timestamp < five_min_ago) {
            buffer.pop_front();
        }

        if buffer.len() >= 3 {
            // Check if consecutive edits are highly similar (>0.9 normalized Levenshtein)
            let checksums: Vec<&[u8]> = buffer.iter().map(|e| e.checksum.as_slice()).collect();
            let mut similar_count = 0;
            for w in checksums.windows(2) {
                let sim = strsim::normalized_levenshtein(
                    &hex_encode(w[0]),
                    &hex_encode(w[1]),
                );
                if sim > 0.9 {
                    similar_count += 1;
                }
            }
            if similar_count >= 2 {
                self.fired.insert(key);
                let _ = signals::insert_signal(
                    conn,
                    SignalKind::Thrashing,
                    Some(node_id),
                    None,
                    session_id,
                    Some(&format!("{} similar edits in 5min", buffer.len())),
                );
            }
        }
    }

    /// DEAD_END: zero incoming non-test edges after >=10 actions or 15min.
    fn check_dead_end(
        &mut self,
        conn: &Connection,
        graph: &GraphState,
        session_id: &str,
        node_id: &str,
        context: &DetectorContext,
    ) {
        let key = ("DEAD_END".to_string(), node_id.to_string());
        if self.fired.contains(&key) {
            return;
        }

        if context.action_count < 10 {
            return;
        }

        let nid = NodeId(node_id.to_string());
        let callers = graph.callers_of(&nid);
        let non_test_callers: Vec<_> = callers
            .iter()
            .filter(|c| !is_test_path(&c.file_path.to_string_lossy()))
            .collect();

        if non_test_callers.is_empty() {
            self.fired.insert(key);
            let _ = signals::insert_signal(
                conn,
                SignalKind::DeadEnd,
                Some(node_id),
                None,
                session_id,
                Some("Zero non-test incoming edges after sustained editing"),
            );
        }
    }

    /// CYCLE_INTRODUCED: check if adding from→to would create a cycle.
    fn check_cycle_introduced(
        &mut self,
        conn: &Connection,
        graph: &GraphState,
        session_id: &str,
        from: &str,
        to: &str,
    ) {
        let key = ("CYCLE_INTRODUCED".to_string(), format!("{from}::{to}"));
        if self.fired.contains(&key) {
            return;
        }

        let from_nid = NodeId(from.to_string());
        let to_nid = NodeId(to.to_string());

        if let (Some(from_idx), Some(to_idx)) = (graph.get_index(&from_nid), graph.get_index(&to_nid)) {
            if petgraph::algo::has_path_connecting(&graph.graph, to_idx, from_idx, None) {
                self.fired.insert(key);
                let _ = signals::insert_signal(
                    conn,
                    SignalKind::CycleIntroduced,
                    Some(from),
                    None,
                    session_id,
                    Some(&format!("Cycle: {from} → {to} creates back-path")),
                );
            }
        }
    }

    /// LARGE_BLAST_RADIUS: >20 direct or >50 transitive dependents.
    fn check_large_blast_radius(
        &mut self,
        conn: &Connection,
        graph: &GraphState,
        session_id: &str,
        node_id: &str,
    ) {
        let key = ("LARGE_BLAST_RADIUS".to_string(), node_id.to_string());
        if self.fired.contains(&key) {
            return;
        }

        let nid = NodeId(node_id.to_string());
        let direct = graph.callers_of(&nid).len();

        if direct > 20 {
            self.fired.insert(key.clone());
            let _ = signals::insert_signal(
                conn,
                SignalKind::LargeBlastRadius,
                Some(node_id),
                None,
                session_id,
                Some(&format!("{direct} direct dependents")),
            );
            return;
        }

        // Transitive count via BFS on incoming edges
        if let Some(start_idx) = graph.get_index(&nid) {
            let mut visited = HashSet::new();
            let mut queue = VecDeque::new();
            visited.insert(start_idx);
            queue.push_back(start_idx);

            while let Some(idx) = queue.pop_front() {
                for edge in graph.graph.edges_directed(idx, petgraph::Direction::Incoming) {
                    let src = edge.source();
                    if visited.insert(src) {
                        queue.push_back(src);
                    }
                }
                if visited.len() > 50 {
                    break;
                }
            }

            if visited.len() > 50 {
                self.fired.insert(key);
                let _ = signals::insert_signal(
                    conn,
                    SignalKind::LargeBlastRadius,
                    Some(node_id),
                    None,
                    session_id,
                    Some(&format!(">50 transitive dependents")),
                );
            }
        }
    }

    /// UNTESTED: zero edges from test files.
    fn check_untested(
        &mut self,
        conn: &Connection,
        graph: &GraphState,
        session_id: &str,
        node_id: &str,
    ) {
        let key = ("UNTESTED".to_string(), node_id.to_string());
        if self.fired.contains(&key) {
            return;
        }

        let nid = NodeId(node_id.to_string());
        let callers = graph.callers_of(&nid);
        let has_test_caller = callers
            .iter()
            .any(|c| is_test_path(&c.file_path.to_string_lossy()));

        if !has_test_caller {
            self.fired.insert(key);
            let _ = signals::insert_signal(
                conn,
                SignalKind::Untested,
                Some(node_id),
                None,
                session_id,
                Some("No test file references this symbol"),
            );
        }
    }

    /// INDEX_BLIND_SPOT: file exists on disk but has zero indexed nodes.
    fn check_index_blind_spot(
        &mut self,
        conn: &Connection,
        graph: &GraphState,
        session_id: &str,
        file_path: &str,
    ) {
        let key = ("INDEX_BLIND_SPOT".to_string(), file_path.to_string());
        if self.fired.contains(&key) {
            return;
        }

        let path = std::path::PathBuf::from(file_path);
        if !path.exists() {
            return;
        }

        let has_nodes = graph.graph.node_indices().any(|idx| {
            graph
                .graph
                .node_weight(idx)
                .is_some_and(|w| w.file_path == path)
        });

        if !has_nodes {
            self.fired.insert(key);
            let _ = signals::insert_signal(
                conn,
                SignalKind::IndexBlindSpot,
                None,
                Some(file_path),
                session_id,
                Some("File exists but has zero indexed symbols"),
            );
        }
    }

    /// FAILED_SEARCH: same normalized query with 0 results >= 3 times.
    pub fn record_search_miss(&mut self, conn: &Connection, session_id: &str, query: &str) {
        let normalized = query.trim().to_lowercase();
        let key = ("FAILED_SEARCH".to_string(), normalized.clone());
        if self.fired.contains(&key) {
            return;
        }

        let count = self.failed_search_counts.entry(normalized.clone()).or_insert(0);
        *count += 1;

        if *count >= 3 {
            self.fired.insert(key);
            let _ = signals::insert_signal(
                conn,
                SignalKind::FailedSearch,
                None,
                None,
                session_id,
                Some(&format!("Query \"{normalized}\" returned 0 results {count} times")),
            );
        }
    }
}

/// Context passed to detectors for a given event.
pub struct DetectorContext {
    pub node_id: Option<String>,
    pub file_path: Option<String>,
    pub edge_from: Option<String>,
    pub edge_to: Option<String>,
    pub new_checksum: Option<Vec<u8>>,
    pub action_count: u64,
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("test") || lower.contains("spec") || lower.contains("__tests__")
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detector_new() {
        let detector = AntiPatternDetector::new();
        assert!(detector.fired.is_empty());
    }

    #[test]
    fn test_is_test_path() {
        assert!(is_test_path("src/test_utils.rs"));
        assert!(is_test_path("tests/integration.rs"));
        assert!(is_test_path("__tests__/foo.ts"));
        assert!(!is_test_path("src/main.rs"));
    }

    #[test]
    fn test_failed_search_dedup() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();

        let mut detector = AntiPatternDetector::new();
        detector.record_search_miss(&conn, "s1", "  FooBar  ");
        detector.record_search_miss(&conn, "s1", "foobar");
        detector.record_search_miss(&conn, "s1", "FOOBAR");

        // Should have fired after 3rd miss
        assert!(detector.fired.contains(&("FAILED_SEARCH".to_string(), "foobar".to_string())));

        // 4th call should be no-op (dedup)
        detector.record_search_miss(&conn, "s1", "foobar");
    }
}
