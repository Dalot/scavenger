// T068: Integration test — branch handling: separate DBs per branch.

use rusqlite::Connection;
use scavenger::db::{self, queries, schema};
use scavenger::graph::{index, GraphState};
use scavenger::memory;

#[test]
fn test_independent_branch_state() {
    let tmp = tempfile::tempdir().unwrap();
    let scav_dir = tmp.path().join(".scavenger");
    std::fs::create_dir_all(scav_dir.join("indexes")).unwrap();

    // Create source file
    let src = tmp.path().join("test.rs");
    std::fs::write(&src, "fn main_branch_fn() {}").unwrap();

    // Index on "main"
    let main_conn = db::open_branch_db(&scav_dir, "main").unwrap();
    let mut main_graph = GraphState::new();
    index::bulk_index(&main_conn, &mut main_graph, &[src.clone()]).unwrap();
    assert_eq!(main_graph.node_count(), 1);

    // Index on "feature" with a different file
    std::fs::write(&src, "fn feature_fn() {}\nfn another_fn() {}").unwrap();
    let feat_conn = db::open_branch_db(&scav_dir, "feature").unwrap();
    let mut feat_graph = GraphState::new();
    index::bulk_index(&feat_conn, &mut feat_graph, &[src.clone()]).unwrap();
    assert_eq!(feat_graph.node_count(), 2);

    // Verify main branch still has 1 node
    let mut main_graph2 = GraphState::new();
    main_graph2.load_from_db(&main_conn).unwrap();
    assert_eq!(main_graph2.node_count(), 1);
}

#[test]
fn test_annotation_fork_between_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let scav_dir = tmp.path().join(".scavenger");
    std::fs::create_dir_all(scav_dir.join("indexes")).unwrap();

    let parent_conn = db::open_branch_db(&scav_dir, "main").unwrap();
    queries::insert_annotation(&parent_conn, "a1", Some("node"), Some("n1"), "parent note", None, "fact", None, 1000).unwrap();

    let child_conn = db::open_branch_db(&scav_dir, "feature-x").unwrap();
    let count = memory::MemoryManager::fork_annotations(&parent_conn, &child_conn).unwrap();
    assert_eq!(count, 1);

    // Verify child has the annotation
    let anns = queries::get_annotations_for_anchor(&child_conn, "node", "n1").unwrap();
    assert_eq!(anns.len(), 1);
    assert_eq!(anns[0].text, "parent note");

    // Modify child annotation
    child_conn.execute(
        "UPDATE annotations SET text = 'child modified' WHERE id = 'a1'",
        [],
    ).unwrap();

    // Verify parent is unaffected
    let parent_anns = queries::get_annotations_for_anchor(&parent_conn, "node", "n1").unwrap();
    assert_eq!(parent_anns[0].text, "parent note");
}

#[test]
fn test_annotation_merge_dedup() {
    let tmp = tempfile::tempdir().unwrap();
    let scav_dir = tmp.path().join(".scavenger");
    std::fs::create_dir_all(scav_dir.join("indexes")).unwrap();

    let source = db::open_branch_db(&scav_dir, "source").unwrap();
    let target = db::open_branch_db(&scav_dir, "target").unwrap();

    let hash = scavenger::memory::annotations::compute_content_hash(Some("node"), Some("n1"), "shared note");
    queries::insert_annotation(&source, "a1", Some("node"), Some("n1"), "shared note", None, "fact", Some(&hash), 1000).unwrap();
    queries::insert_annotation(&target, "a2", Some("node"), Some("n1"), "shared note", None, "fact", Some(&hash), 1000).unwrap();

    let result = memory::MemoryManager::merge_annotations(&source, &target).unwrap();
    assert_eq!(result.deduped, 1, "same anchor+text should dedup");
    assert_eq!(result.imported, 0);
}
