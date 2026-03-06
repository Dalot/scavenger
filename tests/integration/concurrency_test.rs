// T069: Integration test — concurrency: multi-threaded re-index safety.

use parking_lot::RwLock;
use scavenger::db::schema;
use scavenger::graph::types::*;
use scavenger::graph::{GraphState, index};
use std::path::PathBuf;
use std::sync::Arc;

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

#[test]
fn test_concurrent_reads_during_write() {
    let graph = Arc::new(RwLock::new(GraphState::new()));

    // Seed with initial data
    {
        let mut g = graph.write();
        for i in 0..50 {
            g.add_node(node(&format!("n{i}"), &format!("fn_{i}"), "src/lib.rs"));
        }
    }

    let readers: Vec<_> = (0..4)
        .map(|_| {
            let g = Arc::clone(&graph);
            std::thread::spawn(move || {
                for _ in 0..100 {
                    let guard = g.read();
                    let count = guard.node_count();
                    assert!(count >= 50, "got {count}");
                    drop(guard);
                    std::thread::yield_now();
                }
            })
        })
        .collect();

    let writer = {
        let g = Arc::clone(&graph);
        std::thread::spawn(move || {
            for i in 50..100 {
                let mut guard = g.write();
                guard.add_node(node(&format!("n{i}"), &format!("fn_{i}"), "src/lib.rs"));
                drop(guard);
                std::thread::yield_now();
            }
        })
    };

    writer.join().unwrap();
    for r in readers {
        r.join().unwrap();
    }

    let g = graph.read();
    assert_eq!(g.node_count(), 100);
}

#[test]
fn test_split_phase_reindex_under_contention() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("test.rs");
    std::fs::write(&src, "fn original() {}").unwrap();

    let scav_dir = tmp.path().join(".scavenger");
    std::fs::create_dir_all(scav_dir.join("indexes")).unwrap();

    let conn = scavenger::db::open_branch_db(&scav_dir, "main").unwrap();
    let mut g = GraphState::new();
    index::bulk_index(&conn, &mut g, &[src.clone()]).unwrap();
    assert_eq!(g.node_count(), 1);

    let file_str = src.to_string_lossy().to_string();

    // Prep phase (no lock needed)
    std::fs::write(&src, "fn original() {}\nfn added() {}").unwrap();
    let prep = index::incremental_reindex_prep(&conn, &g, &file_str)
        .unwrap()
        .expect("file was modified, prep should not be None");

    // Readers can still work during prep
    assert_eq!(g.node_count(), 1);

    // Swap phase (needs write access)
    let stats = index::incremental_reindex_swap(&conn, &mut g, prep).unwrap();
    assert!(
        stats.nodes_added >= 2,
        "should have >= 2 after swap, got {}",
        stats.nodes_added
    );
}
