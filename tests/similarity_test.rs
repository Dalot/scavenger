// T066: Unit tests for similarity heuristic — scoring, threshold, matching.

use scavenger::graph::index::ExtractedSymbol;
use scavenger::graph::similarity;
use scavenger::graph::types::*;
use std::collections::HashSet;
use std::path::PathBuf;

fn make_old(name: &str, sig: &str, checksum: &[u8]) -> NodeWeight {
    NodeWeight {
        id: NodeId(format!("old_{name}")),
        kind: NodeKind::Function,
        name: name.to_string(),
        file_path: PathBuf::from("test.rs"),
        line_start: 1,
        line_end: 10,
        signature: sig.to_string(),
        signature_hash: "aabb0011".to_string(),
        docstring: None,
        skeleton: sig.to_string(),
        centrality: 0.0,
        checksum: checksum.to_vec(),
    }
}

fn make_new(name: &str, sig: &str, checksum: &[u8]) -> ExtractedSymbol {
    ExtractedSymbol {
        id: NodeId(format!("new_{name}")),
        kind: NodeKind::Function,
        name: name.to_string(),
        file_path: "test.rs".to_string(),
        line_start: 1,
        line_end: 10,
        signature: sig.to_string(),
        signature_hash: "aabb0011".to_string(),
        docstring: None,
        skeleton: sig.to_string(),
        checksum: checksum.to_vec(),
    }
}

#[test]
fn test_identical_symbols_score_above_threshold() {
    let old = make_old("foo", "fn foo(x: i32) -> bool", &[0xDE, 0xAD]);
    let new = make_new("foo", "fn foo(x: i32) -> bool", &[0xDE, 0xAD]);
    let score = similarity::compute_similarity(&old, &new, &HashSet::new(), &HashSet::new(), true);
    assert!(score > 0.6, "identical should be > 0.6, got {score}");
}

#[test]
fn test_different_symbols_below_threshold() {
    let old = make_old("parse_json", "fn parse_json(s: &str) -> Value", &[0x01]);
    let new = make_new(
        "render_html",
        "fn render_html(ctx: Context) -> String",
        &[0xFF],
    );
    let score = similarity::compute_similarity(&old, &new, &HashSet::new(), &HashSet::new(), true);
    assert!(score < 0.6, "different should be < 0.6, got {score}");
}

#[test]
fn test_threshold_boundary_059_no_match() {
    // Craft a scenario that lands just below 0.6
    let old = make_old("process", "fn process()", &[0x01]);
    let new = make_new("handle", "fn handle()", &[0xFF]);
    let score = similarity::compute_similarity(&old, &new, &HashSet::new(), &HashSet::new(), false);
    // Different name, different body, different file = low score
    assert!(score < 0.6, "boundary case should be < 0.6, got {score}");
}

#[test]
fn test_renamed_with_same_body_matches() {
    let old = make_old("get_user", "fn get_user(id: i32) -> User", &[0xDE, 0xAD]);
    let new = make_new(
        "fetch_user",
        "fn fetch_user(id: i32) -> User",
        &[0xDE, 0xAD],
    );
    let score = similarity::compute_similarity(&old, &new, &HashSet::new(), &HashSet::new(), true);
    assert!(
        score > 0.6,
        "renamed with same body should match, got {score}"
    );
}

#[test]
fn test_find_matches_greedy_assignment() {
    let old1 = make_old("alpha", "fn alpha()", &[1]);
    let old2 = make_old("beta", "fn beta()", &[2]);
    let new1 = make_new("alpha", "fn alpha()", &[1]);

    let orphans = vec![&old1, &old2];
    let candidates = vec![new1];
    let empty = |_: &NodeId| -> HashSet<NodeId> { HashSet::new() };

    let matches = similarity::find_matches(&orphans, &candidates, &empty, &empty, true);
    // Only alpha→alpha should match; beta has no candidate
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].old_id.0, "old_alpha");
    assert_eq!(matches[0].new_id.0, "new_alpha");
}

#[test]
fn test_edge_neighborhood_affects_score() {
    let old = make_old("foo", "fn foo()", &[0xDE]);
    let new = make_new("foo", "fn foo()", &[0xDE]);

    let shared_neighbor = NodeId("shared".to_string());
    let old_neighbors: HashSet<NodeId> = [shared_neighbor.clone()].into();
    let new_neighbors: HashSet<NodeId> = [shared_neighbor].into();

    let score_with =
        similarity::compute_similarity(&old, &new, &old_neighbors, &new_neighbors, true);
    let score_without =
        similarity::compute_similarity(&old, &new, &HashSet::new(), &HashSet::new(), true);

    assert!(
        score_with > score_without,
        "shared neighbors should increase score"
    );
}
