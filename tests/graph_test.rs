// T061: Unit tests for graph module — node/edge CRUD, PageRank, reverse index.

use std::path::PathBuf;
use scavenger::graph::types::*;
use scavenger::graph::GraphState;

fn node(id: &str, name: &str, file: &str) -> NodeWeight {
    NodeWeight {
        id: NodeId(id.to_string()),
        kind: NodeKind::Function,
        name: name.to_string(),
        file_path: PathBuf::from(file),
        line_start: 1,
        line_end: 10,
        signature: format!("fn {name}()"),
        signature_hash: "aabb0011".to_string(),
        docstring: None,
        skeleton: format!("fn {name}()"),
        centrality: 0.0,
        checksum: vec![0xDE, 0xAD],
    }
}

fn edge() -> EdgeWeight {
    EdgeWeight { kind: EdgeKind::Calls, weight: 1.0, confidence: Confidence::Precise }
}

#[test]
fn test_add_remove_node() {
    let mut g = GraphState::new();
    g.add_node(node("a", "alpha", "a.rs"));
    assert_eq!(g.node_count(), 1);
    g.remove_node(&NodeId("a".into()));
    assert_eq!(g.node_count(), 0);
}

#[test]
fn test_add_duplicate_node_updates() {
    let mut g = GraphState::new();
    g.add_node(node("a", "alpha_v1", "a.rs"));
    g.add_node(node("a", "alpha_v2", "a.rs"));
    assert_eq!(g.node_count(), 1);
    assert_eq!(g.get_weight(&NodeId("a".into())).unwrap().name, "alpha_v2");
}

#[test]
fn test_edges_and_callers_callees() {
    let mut g = GraphState::new();
    g.add_node(node("a", "alpha", "a.rs"));
    g.add_node(node("b", "beta", "b.rs"));
    g.add_node(node("c", "gamma", "c.rs"));
    g.add_edge(&NodeId("a".into()), &NodeId("b".into()), edge());
    g.add_edge(&NodeId("a".into()), &NodeId("c".into()), edge());

    assert_eq!(g.edge_count(), 2);
    assert_eq!(g.callees_of(&NodeId("a".into())).len(), 2);
    assert_eq!(g.callers_of(&NodeId("b".into())).len(), 1);
    assert_eq!(g.callers_of(&NodeId("a".into())).len(), 0);
}

#[test]
fn test_remove_edges_from() {
    let mut g = GraphState::new();
    g.add_node(node("a", "alpha", "a.rs"));
    g.add_node(node("b", "beta", "b.rs"));
    g.add_edge(&NodeId("a".into()), &NodeId("b".into()), edge());
    assert_eq!(g.edge_count(), 1);
    g.remove_edges_from(&NodeId("a".into()));
    assert_eq!(g.edge_count(), 0);
}

#[test]
fn test_pagerank_gives_higher_centrality_to_popular_nodes() {
    let mut g = GraphState::new();
    g.add_node(node("a", "alpha", "a.rs"));
    g.add_node(node("b", "beta", "b.rs"));
    g.add_node(node("c", "gamma", "c.rs"));
    g.add_edge(&NodeId("a".into()), &NodeId("c".into()), edge());
    g.add_edge(&NodeId("b".into()), &NodeId("c".into()), edge());
    g.compute_pagerank(0.85, 30);

    let c_rank = g.get_weight(&NodeId("c".into())).unwrap().centrality;
    let a_rank = g.get_weight(&NodeId("a".into())).unwrap().centrality;
    assert!(c_rank > a_rank, "c should be more central than a");
}

#[test]
fn test_reverse_index_correct() {
    let mut g = GraphState::new();
    g.add_node(node("a", "alpha", "a.rs"));
    g.add_node(node("b", "beta", "b.rs"));
    g.add_node(node("c", "gamma", "c.rs"));
    g.add_edge(&NodeId("a".into()), &NodeId("c".into()), edge());
    g.add_edge(&NodeId("b".into()), &NodeId("c".into()), edge());
    g.rebuild_reverse_index();

    let paths = g.reverse_index.get(&NodeId("c".into())).unwrap();
    assert_eq!(paths.len(), 2);
    assert!(paths.contains(&PathBuf::from("a.rs")));
    assert!(paths.contains(&PathBuf::from("b.rs")));
}

#[test]
fn test_load_save_roundtrip() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    scavenger::db::schema::ensure_branch_schema(&conn).unwrap();

    let mut g = GraphState::new();
    g.add_node(node("n1", "hello", "src/lib.rs"));
    g.add_node(node("n2", "world", "src/lib.rs"));
    g.add_edge(&NodeId("n1".into()), &NodeId("n2".into()), edge());
    g.compute_pagerank(0.85, 30);

    // Save
    let files = vec![PathBuf::from("src/lib.rs")];
    scavenger::graph::index::bulk_index(&conn, &mut g, &[]).unwrap();
    // Manual save for this test
    for idx in g.graph.node_indices() {
        if let Some(w) = g.graph.node_weight(idx) {
            scavenger::db::queries::upsert_node(
                &conn, &w.id.0, w.kind, &w.name,
                &w.file_path.to_string_lossy(), w.line_start, w.line_end,
                &w.signature, &w.signature_hash, w.docstring.as_deref(),
                &w.skeleton, &w.checksum,
            ).unwrap();
        }
    }

    // Load into fresh graph
    let mut g2 = GraphState::new();
    g2.load_from_db(&conn).unwrap();
    assert_eq!(g2.node_count(), 2);
}
