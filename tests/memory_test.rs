// T064: Unit tests for memory module — annotations, versions, signals, anti-patterns.

use rusqlite::Connection;
use scavenger::db::schema;
use scavenger::graph::index::ExtractedSymbol;
use scavenger::graph::types::{NodeId, NodeKind};
use scavenger::memory::{annotations, session, signals, versions};

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    schema::ensure_branch_schema(&conn).unwrap();
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

// ── Annotation tests ──

#[test]
fn test_annotation_create_read_update_delete() {
    let conn = setup();

    annotations::upsert_annotation(
        &conn,
        "a1",
        Some(annotations::AnchorType::Node),
        Some("n1"),
        "first version",
        None,
        annotations::AnnotationKind::Fact,
    )
    .unwrap();

    let anns = annotations::read_by_anchor(&conn, "node", "n1").unwrap();
    assert_eq!(anns.len(), 1);
    assert_eq!(anns[0].text, "first version");

    annotations::upsert_annotation(
        &conn,
        "a1",
        Some(annotations::AnchorType::Node),
        Some("n1"),
        "updated version",
        Some("important"),
        annotations::AnnotationKind::Fact,
    )
    .unwrap();

    let anns = annotations::read_by_anchor(&conn, "node", "n1").unwrap();
    assert_eq!(anns.len(), 1);
    assert_eq!(anns[0].text, "updated version");

    assert!(annotations::delete_annotation(&conn, "a1").unwrap());
    let anns = annotations::read_by_anchor(&conn, "node", "n1").unwrap();
    assert_eq!(anns.len(), 0);
}

#[test]
fn test_annotation_anchor_types() {
    let conn = setup();

    annotations::upsert_annotation(
        &conn,
        "f1",
        Some(annotations::AnchorType::File),
        Some("/test.rs"),
        "file note",
        None,
        annotations::AnnotationKind::Fact,
    )
    .unwrap();
    annotations::upsert_annotation(
        &conn,
        "s1",
        Some(annotations::AnchorType::Scope),
        Some("auth"),
        "scope note",
        None,
        annotations::AnnotationKind::Strategy,
    )
    .unwrap();
    annotations::upsert_annotation(
        &conn,
        "p1",
        Some(annotations::AnchorType::Project),
        None,
        "project note",
        None,
        annotations::AnnotationKind::Context,
    )
    .unwrap();

    assert_eq!(
        annotations::read_by_anchor(&conn, "file", "/test.rs")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        annotations::read_by_anchor(&conn, "scope", "auth")
            .unwrap()
            .len(),
        1
    );
}

// ── Version tests ──

#[test]
fn test_version_recording() {
    let conn = setup();
    let sym = make_symbol("foo");
    versions::record_version(&conn, &sym, Some("sess1"), &[]).unwrap();
    let vs = versions::get_recent_versions(&conn, "hash_foo", 5).unwrap();
    assert_eq!(vs.len(), 1);
    assert_eq!(vs[0].signature, "fn foo()");
}

#[test]
fn test_version_ordinal_decay() {
    let conn = setup();
    let sym = make_symbol("bar");
    for _ in 0..8 {
        versions::record_version(&conn, &sym, None, &[]).unwrap();
    }
    let vs = versions::get_recent_versions(&conn, "hash_bar", 10).unwrap();
    assert!(vs.len() <= 5, "should keep at most 5, got {}", vs.len());
}

// ── Signal tests ──

#[test]
fn test_signal_insert_query() {
    let conn = setup();
    signals::insert_signal(
        &conn,
        signals::SignalKind::Thrashing,
        Some("n1"),
        None,
        "s1",
        Some("details"),
    )
    .unwrap();
    let sigs = signals::signals_for_node(&conn, "n1", 10).unwrap();
    assert_eq!(sigs.len(), 1);
    assert_eq!(sigs[0].kind, "THRASHING");
}

#[test]
fn test_signal_active_count() {
    let conn = setup();
    signals::insert_signal(
        &conn,
        signals::SignalKind::FailedSearch,
        Some("n1"),
        None,
        "s1",
        None,
    )
    .unwrap();
    signals::insert_signal(
        &conn,
        signals::SignalKind::Churn,
        Some("n2"),
        None,
        "s1",
        None,
    )
    .unwrap();
    assert_eq!(signals::active_signal_count(&conn).unwrap(), 2);
}

// ── Session tests ──

#[test]
fn test_session_event_recording() {
    let conn = setup();
    session::record_event(&conn, "s1", "read", Some("/a.rs"), Some("foo")).unwrap();
    session::record_event(&conn, "s1", "edit", Some("/b.rs"), None).unwrap();
    let events = session::recent_activity(&conn, "s1", 10).unwrap();
    assert_eq!(events.len(), 2);
}

#[test]
fn test_session_summary() {
    let conn = setup();
    session::record_event(&conn, "s1", "read", Some("/a.rs"), Some("foo")).unwrap();
    session::record_event(&conn, "s1", "read", Some("/b.rs"), Some("bar")).unwrap();
    let summary = session::session_summary(&conn, "s1").unwrap();
    assert_eq!(summary.total_events, 2);
    assert_eq!(summary.unique_files, 2);
    assert_eq!(summary.unique_symbols, 2);
}

// ── Anti-pattern detector tests ──

#[test]
fn test_failed_search_detector() {
    let conn = setup();
    let mut detector = scavenger::memory::antipattern::AntiPatternDetector::new();
    detector.record_search_miss(&conn, "s1", "nonexistent");
    detector.record_search_miss(&conn, "s1", "nonexistent");
    detector.record_search_miss(&conn, "s1", "nonexistent");
    let sigs = signals::signals_for_session(&conn, "s1", 10).unwrap();
    assert!(sigs.iter().any(|s| s.kind == "FAILED_SEARCH"));
}
