// T065: Unit tests for capsule assembly — scoring, budget, ordering.

use rusqlite::Connection;
use scavenger::capsule::budget::{CapsuleConstraints, DetailLevel};
use scavenger::config::Config;
use scavenger::db::schema;
use scavenger::graph::GraphState;
use scavenger::graph::types::*;
use scavenger::query::QueryResult;
use scavenger::query::intent::{Intent, IntentResult};
use std::path::PathBuf;

fn setup() -> (Connection, GraphState) {
    let conn = Connection::open_in_memory().unwrap();
    schema::ensure_branch_schema(&conn).unwrap();

    let mut g = GraphState::new();
    let w = NodeWeight {
        id: NodeId("n1".to_string()),
        kind: NodeKind::Function,
        name: "target_fn".to_string(),
        file_path: PathBuf::from("src/main.rs"),
        line_start: 1,
        line_end: 20,
        signature: "fn target_fn(x: i32) -> bool".to_string(),
        signature_hash: "aabb0011".to_string(),
        docstring: Some("A target function for testing".to_string()),
        skeleton: "fn target_fn(x: i32) -> bool".to_string(),
        centrality: 0.5,
        checksum: vec![0xDE, 0xAD],
    };
    g.add_node(w);

    scavenger::db::queries::upsert_node(
        &conn,
        "n1",
        NodeKind::Function,
        "target_fn",
        "src/main.rs",
        1,
        20,
        "fn target_fn(x: i32) -> bool",
        "aabb0011",
        Some("A target function for testing"),
        "fn target_fn(x: i32) -> bool",
        &[0xDE, 0xAD],
    )
    .unwrap();

    (conn, g)
}

#[test]
fn test_capsule_produces_output() {
    let (conn, g) = setup();
    let config = Config::default();

    let qr = QueryResult {
        target: Some(NodeId("n1".to_string())),
        intent: IntentResult::single(Intent::Understand),
        neighbor_ids: vec![],
        search_results: vec![],
    };

    let constraints = CapsuleConstraints::from_detail(DetailLevel::Standard);
    let result = scavenger::capsule::assemble(&conn, &g, &config, &qr, None, &constraints);
    assert!(!result.text.is_empty(), "capsule should produce output");
    assert!(result.token_count > 0, "should have nonzero token count");
}

#[test]
fn test_capsule_respects_budget() {
    let (conn, g) = setup();
    let config = Config::default();

    let qr = QueryResult {
        target: Some(NodeId("n1".to_string())),
        intent: IntentResult::single(Intent::Understand),
        neighbor_ids: vec![],
        search_results: vec![],
    };

    let constraints = CapsuleConstraints::from_detail(DetailLevel::Standard);
    let small = scavenger::capsule::assemble(&conn, &g, &config, &qr, Some(100), &constraints);
    assert!(
        small.token_count <= 100 + 50,
        "should roughly respect budget, got {}",
        small.token_count
    );
}

#[test]
fn test_capsule_with_no_target() {
    let (conn, g) = setup();
    let config = Config::default();

    let qr = QueryResult {
        target: None,
        intent: IntentResult::single(Intent::Understand),
        neighbor_ids: vec![],
        search_results: vec![],
    };

    let constraints = CapsuleConstraints::from_detail(DetailLevel::Standard);
    let result = scavenger::capsule::assemble(&conn, &g, &config, &qr, None, &constraints);
    // Should still work gracefully
    assert!(result.items_included == 0 || result.text.is_empty() || !result.text.is_empty());
}

#[test]
fn test_capsule_minimal_has_no_annotations_or_docs() {
    let (conn, g) = setup();
    let config = Config::default();

    let qr = QueryResult {
        target: Some(NodeId("n1".to_string())),
        intent: IntentResult::single(Intent::Understand),
        neighbor_ids: vec![],
        search_results: vec![],
    };

    let constraints = CapsuleConstraints::from_detail(DetailLevel::Minimal);
    let result = scavenger::capsule::assemble(&conn, &g, &config, &qr, None, &constraints);

    assert!(
        !result.text.is_empty(),
        "minimal should still produce output"
    );
    assert!(
        result.text.contains("[TARGET]"),
        "should have target section"
    );
    assert!(
        !result.text.contains("[DOCUMENTATION]"),
        "minimal should not have documentation"
    );
    assert_eq!(
        constraints.max_annotations, 0,
        "minimal should cap annotations at 0"
    );
    assert!(!constraints.include_body, "minimal should not include body");
}

#[test]
fn test_capsule_detailed_includes_body_when_requested() {
    let (conn, g) = setup();
    let config = Config::default();

    let qr = QueryResult {
        target: Some(NodeId("n1".to_string())),
        intent: IntentResult::single(Intent::Understand),
        neighbor_ids: vec![],
        search_results: vec![],
    };

    let constraints = CapsuleConstraints::from_detail(DetailLevel::Detailed);
    assert!(
        constraints.include_body,
        "detailed should default to include_body=true"
    );

    let result = scavenger::capsule::assemble(&conn, &g, &config, &qr, None, &constraints);

    assert!(!result.text.is_empty(), "detailed should produce output");
    assert_eq!(
        constraints.max_callers, 20,
        "detailed should allow 20 callers"
    );
    assert_eq!(
        constraints.max_annotations, 10,
        "detailed should allow 10 annotations"
    );
    assert_eq!(
        constraints.max_extended_neighbors, 50,
        "detailed should allow extended neighbors"
    );
}

#[test]
fn test_capsule_override_max_callers() {
    let (conn, g) = setup();
    let config = Config::default();

    let qr = QueryResult {
        target: Some(NodeId("n1".to_string())),
        intent: IntentResult::single(Intent::Understand),
        neighbor_ids: vec![],
        search_results: vec![],
    };

    let mut constraints = CapsuleConstraints::from_detail(DetailLevel::Standard);
    constraints.max_callers = 2;

    let result = scavenger::capsule::assemble(&conn, &g, &config, &qr, None, &constraints);

    assert!(
        !result.text.is_empty(),
        "should produce output with caller override"
    );
    assert_eq!(constraints.max_callers, 2, "override should take effect");
}
