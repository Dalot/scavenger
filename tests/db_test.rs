// T060: Unit tests for db module — schema, migration, CRUD.

use rusqlite::Connection;
use scavenger::db::{queries, schema};

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    schema::ensure_branch_schema(&conn).unwrap();
    conn
}

#[test]
fn test_schema_version_set() {
    let conn = setup();
    let ver: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(ver, 1);
}

#[test]
fn test_schema_idempotent() {
    let conn = setup();
    schema::ensure_branch_schema(&conn).unwrap();
    let ver: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
    assert_eq!(ver, 1);
}

#[test]
fn test_upsert_and_query_node() {
    let conn = setup();
    queries::upsert_node(
        &conn, "n1", scavenger::graph::types::NodeKind::Function,
        "hello", "src/lib.rs", 1, 10, "fn hello()", "aabb0011",
        Some("doc"), "fn hello()", &[0xDE, 0xAD],
    ).unwrap();

    let nodes = queries::load_all_nodes(&conn).unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name, "hello");
}

#[test]
fn test_upsert_node_updates_on_conflict() {
    let conn = setup();
    queries::upsert_node(
        &conn, "n1", scavenger::graph::types::NodeKind::Function,
        "hello", "src/lib.rs", 1, 10, "fn hello()", "aabb0011",
        None, "fn hello()", &[0x01],
    ).unwrap();
    queries::upsert_node(
        &conn, "n1", scavenger::graph::types::NodeKind::Function,
        "hello_v2", "src/lib.rs", 1, 15, "fn hello_v2()", "aabb0012",
        None, "fn hello_v2()", &[0x02],
    ).unwrap();

    let nodes = queries::load_all_nodes(&conn).unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name, "hello_v2");
}

#[test]
fn test_edge_crud() {
    let conn = setup();
    queries::upsert_node(
        &conn, "a", scavenger::graph::types::NodeKind::Function,
        "alpha", "a.rs", 1, 5, "fn alpha()", "aa", None, "fn alpha()", &[],
    ).unwrap();
    queries::upsert_node(
        &conn, "b", scavenger::graph::types::NodeKind::Function,
        "beta", "b.rs", 1, 5, "fn beta()", "bb", None, "fn beta()", &[],
    ).unwrap();
    queries::upsert_edge(
        &conn, "a", "b",
        scavenger::graph::types::EdgeKind::Calls, 1.0,
        scavenger::graph::types::Confidence::Precise,
    ).unwrap();

    let edges = queries::get_all_edges(&conn).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].from_id, "a");
    assert_eq!(edges[0].to_id, "b");
}

#[test]
fn test_delete_nodes_by_file() {
    let conn = setup();
    queries::upsert_node(
        &conn, "n1", scavenger::graph::types::NodeKind::Function,
        "foo", "src/a.rs", 1, 5, "fn foo()", "ff", None, "fn foo()", &[],
    ).unwrap();
    queries::upsert_node(
        &conn, "n2", scavenger::graph::types::NodeKind::Function,
        "bar", "src/a.rs", 6, 10, "fn bar()", "bb", None, "fn bar()", &[],
    ).unwrap();

    let deleted = queries::delete_nodes_by_file(&conn, "src/a.rs").unwrap();
    assert_eq!(deleted.len(), 2);

    let nodes = queries::load_all_nodes(&conn).unwrap();
    assert_eq!(nodes.len(), 0);
}

#[test]
fn test_file_last_indexed() {
    let conn = setup();
    queries::upsert_file(&conn, "test.rs", "code", 100, 1000).unwrap();
    let ts = queries::get_file_last_indexed(&conn, "test.rs").unwrap();
    assert_eq!(ts, Some(1000));
}

#[test]
fn test_annotation_crud() {
    let conn = setup();
    queries::insert_annotation(&conn, "a1", Some("node"), Some("n1"), "note text", None, 1000).unwrap();
    let anns = queries::get_annotations_for_anchor(&conn, "node", "n1").unwrap();
    assert_eq!(anns.len(), 1);
    assert_eq!(anns[0].text, "note text");
}

#[test]
fn test_behavioral_signal_insert_and_prune() {
    let conn = setup();
    queries::insert_behavioral_signal(&conn, "THRASHING", Some("n1"), None, "s1", 100, None).unwrap();
    let pruned = queries::prune_old_signals(&conn, 200).unwrap();
    assert_eq!(pruned, 1);
}

#[test]
fn test_daemon_meta_schema() {
    let conn = Connection::open_in_memory().unwrap();
    schema::ensure_daemon_meta_schema(&conn).unwrap();
    queries::set_meta(&conn, "key1", "val1").unwrap();
    assert_eq!(queries::get_meta(&conn, "key1").unwrap(), Some("val1".to_string()));
}
