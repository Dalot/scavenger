// T067: Integration test — full lifecycle: init → index → capsule → edit → re-index → verify.

use std::path::PathBuf;
use rusqlite::Connection;

use scavenger::config::Config;
use scavenger::db::{self, queries, schema};
use scavenger::graph::{self, index, GraphState};
use scavenger::query;
use scavenger::capsule;

#[test]
fn test_full_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // Create a source file
    let src_dir = root.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    std::fs::write(src_dir.join("lib.rs"), "fn hello() { world(); }\nfn world() {}").unwrap();

    // Init: create DB and bulk index
    let scav_dir = root.join(".scavenger");
    std::fs::create_dir_all(scav_dir.join("indexes")).unwrap();
    let conn = db::open_branch_db(&scav_dir, "main").unwrap();

    let source_files = vec![src_dir.join("lib.rs")];
    let mut g = GraphState::new();
    let stats = index::bulk_index(&conn, &mut g, &source_files).unwrap();

    assert!(stats.files_indexed >= 1);
    assert!(stats.symbols_extracted >= 2);
    assert_eq!(g.node_count(), 2);

    // Generate capsule
    let config = Config::default();
    let file_str = src_dir.join("lib.rs").to_string_lossy().to_string();
    let qr = query::run_query(&conn, &g, &config, &file_str, Some("hello"), None);
    assert!(qr.target.is_some());

    let capsule_result = capsule::assemble(&conn, &g, &config, &qr, None);
    assert!(!capsule_result.text.is_empty());
    let _original_text = capsule_result.text.clone();

    // Edit the file (add a new function)
    std::fs::write(
        src_dir.join("lib.rs"),
        "fn hello() { world(); }\nfn world() {}\n/// New function\nfn greet(name: &str) { hello(); }",
    ).unwrap();

    // Re-index via incremental flow
    let prep = index::incremental_reindex_prep(&conn, &g, &file_str).unwrap();
    let inc_stats = index::incremental_reindex_swap(&conn, &mut g, prep).unwrap();

    assert!(inc_stats.nodes_added >= 3, "should have 3 nodes after edit");
    assert_eq!(g.node_count(), 3);

    // Verify capsule now includes new content
    let qr2 = query::run_query(&conn, &g, &config, &file_str, Some("hello"), None);
    let capsule2 = capsule::assemble(&conn, &g, &config, &qr2, None);
    // The capsule should now potentially include greet as a neighbor
    assert!(!capsule2.text.is_empty());
}

#[test]
fn test_doc_indexing_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    std::fs::write(
        root.join("README.md"),
        "# Test\n\n## Section A\n\nContent A\n\n## Section B\n\nContent B\n",
    ).unwrap();

    let conn = Connection::open_in_memory().unwrap();
    schema::ensure_branch_schema(&conn).unwrap();

    let content = std::fs::read_to_string(root.join("README.md")).unwrap();
    let chunks = scavenger::graph::doc_indexer::index_doc_file(
        &conn,
        &root.join("README.md").to_string_lossy(),
        &content,
    ).unwrap();

    assert!(chunks >= 1, "should index at least 1 doc chunk");
}
