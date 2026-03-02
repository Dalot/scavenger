pub mod doc_indexer;
pub mod estimator;
pub mod index;
pub mod similarity;
pub mod traversal;
pub mod types;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::RwLock;
use petgraph::Directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::EdgeRef;
use rusqlite::Connection;

use crate::db;
use crate::db::queries;
use types::{Confidence, EdgeKind, EdgeWeight, NodeId, NodeKind, NodeWeight};

/// Thread-safe handle to the in-memory graph.
pub type SharedGraph = Arc<RwLock<GraphState>>;

pub fn new_shared_graph() -> SharedGraph {
    Arc::new(RwLock::new(GraphState::new()))
}

/// In-memory dependency graph backed by petgraph::StableGraph.
pub struct GraphState {
    pub graph: StableGraph<NodeWeight, EdgeWeight, Directed>,
    node_index_map: HashMap<NodeId, NodeIndex>,
    /// Reverse index: for each node, which files contain symbols that point TO it.
    pub reverse_index: HashMap<NodeId, Vec<PathBuf>>,
}

impl GraphState {
    pub fn new() -> Self {
        Self {
            graph: StableGraph::new(),
            node_index_map: HashMap::new(),
            reverse_index: HashMap::new(),
        }
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn add_node(&mut self, weight: NodeWeight) -> NodeIndex {
        let id = weight.id.clone();
        if let Some(&existing) = self.node_index_map.get(&id) {
            *self.graph.node_weight_mut(existing).expect("stale index") = weight;
            existing
        } else {
            let idx = self.graph.add_node(weight);
            self.node_index_map.insert(id, idx);
            idx
        }
    }

    pub fn remove_node(&mut self, id: &NodeId) -> Option<NodeWeight> {
        if let Some(idx) = self.node_index_map.remove(id) {
            self.reverse_index.remove(id);
            self.graph.remove_node(idx)
        } else {
            None
        }
    }

    pub fn get_index(&self, id: &NodeId) -> Option<NodeIndex> {
        self.node_index_map.get(id).copied()
    }

    pub fn get_weight(&self, id: &NodeId) -> Option<&NodeWeight> {
        self.get_index(id)
            .and_then(|idx| self.graph.node_weight(idx))
    }

    pub fn add_edge(
        &mut self,
        from: &NodeId,
        to: &NodeId,
        weight: EdgeWeight,
    ) -> Option<petgraph::stable_graph::EdgeIndex> {
        let from_idx = self.node_index_map.get(from)?;
        let to_idx = self.node_index_map.get(to)?;
        Some(self.graph.add_edge(*from_idx, *to_idx, weight))
    }

    /// Remove all edges originating from the given node.
    pub fn remove_edges_from(&mut self, id: &NodeId) {
        if let Some(&idx) = self.node_index_map.get(id) {
            let outgoing: Vec<_> = self
                .graph
                .edges_directed(idx, petgraph::Direction::Outgoing)
                .map(|e| e.id())
                .collect();
            for edge_id in outgoing {
                self.graph.remove_edge(edge_id);
            }
        }
    }

    /// Rebuild the reverse index from the current edge set.
    pub fn rebuild_reverse_index(&mut self) {
        self.reverse_index.clear();
        for edge in self.graph.edge_indices() {
            if let Some((source, target)) = self.graph.edge_endpoints(edge) {
                if let (Some(src_w), Some(tgt_w)) = (
                    self.graph.node_weight(source),
                    self.graph.node_weight(target),
                ) {
                    self.reverse_index
                        .entry(tgt_w.id.clone())
                        .or_default()
                        .push(src_w.file_path.clone());
                }
            }
        }
        for paths in self.reverse_index.values_mut() {
            paths.sort();
            paths.dedup();
        }
    }

    /// Run PageRank and update centrality scores on all nodes.
    pub fn compute_pagerank(&mut self, damping: f64, iterations: usize) {
        let scores = petgraph::algo::page_rank(&self.graph, damping as f32, iterations);
        let indices: Vec<NodeIndex> = self.graph.node_indices().collect();
        for (i, idx) in indices.into_iter().enumerate() {
            if let Some(w) = self.graph.node_weight_mut(idx) {
                w.centrality = scores[i];
            }
        }
    }

    /// Load the full graph from a SQLite branch database.
    pub fn load_from_db(&mut self, conn: &Connection) -> db::DbResult<()> {
        self.graph.clear();
        self.node_index_map.clear();
        self.reverse_index.clear();

        let node_rows = queries::load_all_nodes(conn)?;
        for row in &node_rows {
            let kind = NodeKind::from_str(&row.kind).unwrap_or(NodeKind::Function);
            let weight = NodeWeight {
                id: NodeId(row.id.clone()),
                kind,
                name: row.name.clone(),
                file_path: PathBuf::from(&row.file_path),
                line_start: row.line_start,
                line_end: row.line_end,
                signature: row.signature.clone(),
                signature_hash: row.signature_hash.clone(),
                docstring: row.docstring.clone(),
                skeleton: row.skeleton.clone(),
                centrality: row.centrality as f32,
                checksum: row.checksum.clone(),
            };
            self.add_node(weight);
        }

        let edge_rows = queries::get_all_edges(conn)?;
        for row in &edge_rows {
            let kind = EdgeKind::from_str(&row.kind).unwrap_or(EdgeKind::Calls);
            let confidence = Confidence::from_str(&row.confidence).unwrap_or(Confidence::Precise);
            let weight = EdgeWeight {
                kind,
                weight: row.weight as f32,
                confidence,
            };
            self.add_edge(
                &NodeId(row.from_id.clone()),
                &NodeId(row.to_id.clone()),
                weight,
            );
        }

        self.rebuild_reverse_index();
        Ok(())
    }

    /// Persist centrality scores back to SQLite.
    pub fn save_centrality(&self, conn: &Connection) -> db::DbResult<()> {
        for idx in self.graph.node_indices() {
            if let Some(w) = self.graph.node_weight(idx) {
                queries::update_centrality(conn, &w.id.0, w.centrality as f64)?;
            }
        }
        Ok(())
    }

    /// Get callers (nodes that have edges pointing TO the given node).
    pub fn callers_of(&self, id: &NodeId) -> Vec<&NodeWeight> {
        let Some(&idx) = self.node_index_map.get(id) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(idx, petgraph::Direction::Incoming)
            .filter_map(|e| self.graph.node_weight(e.source()))
            .collect()
    }

    /// Get callees (nodes that the given node has edges pointing TO).
    pub fn callees_of(&self, id: &NodeId) -> Vec<&NodeWeight> {
        let Some(&idx) = self.node_index_map.get(id) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(idx, petgraph::Direction::Outgoing)
            .filter_map(|e| self.graph.node_weight(e.target()))
            .collect()
    }
}

impl Default for GraphState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_node(id_str: &str, name: &str, file: &str) -> NodeWeight {
        NodeWeight {
            id: NodeId(id_str.to_string()),
            kind: NodeKind::Function,
            name: name.to_string(),
            file_path: PathBuf::from(file),
            line_start: 1,
            line_end: 10,
            signature: format!("fn {name}()"),
            signature_hash: "abcdef01".to_string(),
            docstring: None,
            skeleton: format!("fn {name}()"),
            centrality: 0.0,
            checksum: vec![0xDE, 0xAD],
        }
    }

    #[test]
    fn test_add_and_get_node() {
        let mut g = GraphState::new();
        let w = sample_node("n1", "foo", "src/lib.rs");
        g.add_node(w);
        assert_eq!(g.node_count(), 1);
        assert!(g.get_weight(&NodeId("n1".into())).is_some());
    }

    #[test]
    fn test_add_edge_and_callers_callees() {
        let mut g = GraphState::new();
        g.add_node(sample_node("a", "alpha", "a.rs"));
        g.add_node(sample_node("b", "beta", "b.rs"));
        g.add_edge(
            &NodeId("a".into()),
            &NodeId("b".into()),
            EdgeWeight {
                kind: EdgeKind::Calls,
                weight: 1.0,
                confidence: Confidence::Precise,
            },
        );
        assert_eq!(g.edge_count(), 1);

        let callees = g.callees_of(&NodeId("a".into()));
        assert_eq!(callees.len(), 1);
        assert_eq!(callees[0].name, "beta");

        let callers = g.callers_of(&NodeId("b".into()));
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].name, "alpha");
    }

    #[test]
    fn test_remove_node() {
        let mut g = GraphState::new();
        g.add_node(sample_node("x", "xfn", "x.rs"));
        assert_eq!(g.node_count(), 1);
        g.remove_node(&NodeId("x".into()));
        assert_eq!(g.node_count(), 0);
    }

    #[test]
    fn test_rebuild_reverse_index() {
        let mut g = GraphState::new();
        g.add_node(sample_node("a", "alpha", "a.rs"));
        g.add_node(sample_node("b", "beta", "b.rs"));
        g.add_edge(
            &NodeId("a".into()),
            &NodeId("b".into()),
            EdgeWeight {
                kind: EdgeKind::Calls,
                weight: 1.0,
                confidence: Confidence::Precise,
            },
        );
        g.rebuild_reverse_index();
        let paths = g.reverse_index.get(&NodeId("b".into())).unwrap();
        assert_eq!(paths, &[PathBuf::from("a.rs")]);
    }

    #[test]
    fn test_pagerank() {
        let mut g = GraphState::new();
        g.add_node(sample_node("a", "alpha", "a.rs"));
        g.add_node(sample_node("b", "beta", "b.rs"));
        g.add_node(sample_node("c", "gamma", "c.rs"));
        g.add_edge(
            &NodeId("a".into()),
            &NodeId("b".into()),
            EdgeWeight {
                kind: EdgeKind::Calls,
                weight: 1.0,
                confidence: Confidence::Precise,
            },
        );
        g.add_edge(
            &NodeId("c".into()),
            &NodeId("b".into()),
            EdgeWeight {
                kind: EdgeKind::Calls,
                weight: 1.0,
                confidence: Confidence::Precise,
            },
        );
        g.compute_pagerank(0.85, 30);

        let b_centrality = g.get_weight(&NodeId("b".into())).unwrap().centrality;
        let a_centrality = g.get_weight(&NodeId("a".into())).unwrap().centrality;
        assert!(
            b_centrality > a_centrality,
            "b should have higher centrality"
        );
    }

    #[test]
    fn test_load_from_db() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum)
             VALUES ('n1', 'Function', 'hello', 'src/lib.rs', 1, 5, 'fn hello()', 'aabb0011', 'fn hello()', X'CAFE')",
            [],
        ).unwrap();

        let mut g = GraphState::new();
        g.load_from_db(&conn).unwrap();
        assert_eq!(g.node_count(), 1);
        let w = g.get_weight(&NodeId("n1".into())).unwrap();
        assert_eq!(w.name, "hello");
    }
}
