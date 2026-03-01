use std::collections::HashSet;

use strsim::jaro_winkler;

use super::types::{NodeId, NodeWeight};
use crate::graph::index::ExtractedSymbol;

/// Result of matching an orphaned old node to a candidate new node.
#[derive(Debug, Clone)]
pub struct SimilarityMatch {
    pub old_id: NodeId,
    pub new_id: NodeId,
    #[allow(dead_code)]
    pub score: f64,
}

/// 5-component weighted similarity score for identity migration.
///
/// Weights: name 0.3, signature 0.25, body 0.25, edge neighborhood 0.15, file proximity 0.05
/// Threshold: >0.6 triggers migration of annotations and version history.
pub fn compute_similarity(
    old: &NodeWeight,
    new: &ExtractedSymbol,
    old_neighbor_ids: &HashSet<NodeId>,
    new_neighbor_ids: &HashSet<NodeId>,
    same_file: bool,
) -> f64 {
    let name_sim = jaro_winkler(&old.name, &new.name) * 0.3;
    let sig_sim = signature_similarity(&old.signature, &new.signature) * 0.25;
    let body_sim = body_similarity(&old.checksum, &new.checksum) * 0.25;
    let edge_sim = edge_neighborhood_jaccard(old_neighbor_ids, new_neighbor_ids) * 0.15;
    let file_prox = if same_file { 1.0 } else { 0.0 } * 0.05;

    name_sim + sig_sim + body_sim + edge_sim + file_prox
}

/// Match orphaned old nodes against new candidate nodes.
/// Returns pairs where score > threshold (0.6).
pub fn find_matches(
    orphans: &[&NodeWeight],
    candidates: &[ExtractedSymbol],
    old_neighbors: &dyn Fn(&NodeId) -> HashSet<NodeId>,
    new_neighbors: &dyn Fn(&NodeId) -> HashSet<NodeId>,
    same_file: bool,
) -> Vec<SimilarityMatch> {
    let threshold = 0.6;
    let mut matches = Vec::new();
    let mut claimed_new: HashSet<NodeId> = HashSet::new();

    // Compute all scores, then greedily assign best matches
    let mut all_scores: Vec<(usize, usize, f64)> = Vec::new();
    for (oi, old) in orphans.iter().enumerate() {
        let old_n = old_neighbors(&old.id);
        for (ni, new) in candidates.iter().enumerate() {
            let new_n = new_neighbors(&new.id);
            let score = compute_similarity(old, new, &old_n, &new_n, same_file);
            if score > threshold {
                all_scores.push((oi, ni, score));
            }
        }
    }

    all_scores.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    let mut claimed_old: HashSet<usize> = HashSet::new();

    for (oi, ni, score) in all_scores {
        if claimed_old.contains(&oi) || claimed_new.contains(&candidates[ni].id) {
            continue;
        }
        matches.push(SimilarityMatch {
            old_id: orphans[oi].id.clone(),
            new_id: candidates[ni].id.clone(),
            score,
        });
        claimed_old.insert(oi);
        claimed_new.insert(candidates[ni].id.clone());
    }

    matches
}

fn signature_similarity(old_sig: &str, new_sig: &str) -> f64 {
    let old_params = extract_param_names(old_sig);
    let new_params = extract_param_names(new_sig);

    if old_params.is_empty() && new_params.is_empty() {
        return jaro_winkler(old_sig, new_sig);
    }

    let param_jaccard = jaccard_index(&old_params, &new_params);
    let return_sim = return_type_similarity(old_sig, new_sig);

    param_jaccard * 0.6 + return_sim * 0.4
}

fn body_similarity(old_checksum: &[u8], new_checksum: &[u8]) -> f64 {
    if old_checksum == new_checksum {
        1.0
    } else {
        0.0
    }
}

fn edge_neighborhood_jaccard(old_set: &HashSet<NodeId>, new_set: &HashSet<NodeId>) -> f64 {
    jaccard_index_set(old_set, new_set)
}

fn extract_param_names(sig: &str) -> HashSet<String> {
    let mut params = HashSet::new();
    // Extract content between parentheses
    if let Some(start) = sig.find('(') {
        if let Some(end) = sig.rfind(')') {
            let inner = &sig[start + 1..end];
            for param in inner.split(',') {
                let trimmed = param.trim();
                if let Some(name) = trimmed.split(':').next() {
                    let name = name.split_whitespace().last().unwrap_or("").trim();
                    if !name.is_empty() && name != "self" && name != "&self" && name != "&mut self" {
                        params.insert(name.to_string());
                    }
                }
            }
        }
    }
    params
}

fn return_type_similarity(old_sig: &str, new_sig: &str) -> f64 {
    let old_ret = extract_return_type(old_sig);
    let new_ret = extract_return_type(new_sig);
    match (old_ret, new_ret) {
        (Some(a), Some(b)) => jaro_winkler(&a, &b),
        (None, None) => 1.0,
        _ => 0.0,
    }
}

fn extract_return_type(sig: &str) -> Option<String> {
    // Look for `->` pattern (Rust, Python type hints)
    if let Some(pos) = sig.rfind("->") {
        let ret = sig[pos + 2..].trim();
        if !ret.is_empty() {
            return Some(ret.to_string());
        }
    }
    // Look for `: type` after closing paren (TypeScript)
    if let Some(paren_pos) = sig.rfind(')') {
        let after = sig[paren_pos + 1..].trim();
        if let Some(stripped) = after.strip_prefix(':') {
            let ret = stripped.trim();
            if !ret.is_empty() {
                return Some(ret.to_string());
            }
        }
    }
    None
}

fn jaccard_index(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 { 0.0 } else { intersection / union }
}

fn jaccard_index_set(a: &HashSet<NodeId>, b: &HashSet<NodeId>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 { 0.0 } else { intersection / union }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use crate::graph::types::NodeKind;

    fn make_node(name: &str, sig: &str, checksum: &[u8]) -> NodeWeight {
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

    fn make_extracted(name: &str, sig: &str, checksum: &[u8]) -> ExtractedSymbol {
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
    fn test_identical_nodes_high_score() {
        let old = make_node("foo", "fn foo(x: i32) -> bool", &[0xDE, 0xAD]);
        let new = make_extracted("foo", "fn foo(x: i32) -> bool", &[0xDE, 0xAD]);
        let score = compute_similarity(&old, &new, &HashSet::new(), &HashSet::new(), true);
        assert!(score > 0.8, "identical nodes should score > 0.8, got {score}");
    }

    #[test]
    fn test_renamed_function_moderate_score() {
        let old = make_node("get_user", "fn get_user(id: i32) -> User", &[0xDE, 0xAD]);
        let new = make_extracted("fetch_user", "fn fetch_user(id: i32) -> User", &[0xDE, 0xAD]);
        let score = compute_similarity(&old, &new, &HashSet::new(), &HashSet::new(), true);
        assert!(score > 0.6, "renamed with same body should score > 0.6, got {score}");
    }

    #[test]
    fn test_completely_different_low_score() {
        let old = make_node("parse_json", "fn parse_json(s: &str) -> Value", &[0x01]);
        let new = make_extracted("render_html", "fn render_html(ctx: Context) -> String", &[0xFF]);
        let score = compute_similarity(&old, &new, &HashSet::new(), &HashSet::new(), true);
        assert!(score < 0.6, "completely different should score < 0.6, got {score}");
    }

    #[test]
    fn test_find_matches_greedy() {
        let old1 = make_node("foo", "fn foo()", &[1]);
        let old2 = make_node("bar", "fn bar()", &[2]);
        let new1 = make_extracted("foo", "fn foo()", &[1]);
        let new2 = make_extracted("baz", "fn baz()", &[3]);

        let orphans = vec![&old1, &old2];
        let candidates = vec![new1, new2];
        let empty = |_: &NodeId| -> HashSet<NodeId> { HashSet::new() };

        let matches = find_matches(&orphans, &candidates, &empty, &empty, true);
        assert_eq!(matches.len(), 1, "only foo→foo should match");
        assert_eq!(matches[0].old_id.0, "old_foo");
        assert_eq!(matches[0].new_id.0, "new_foo");
    }

    #[test]
    fn test_extract_param_names() {
        let params = extract_param_names("fn foo(x: i32, y: String) -> bool");
        assert!(params.contains("x"));
        assert!(params.contains("y"));
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_return_type_similarity_match() {
        let sim = return_type_similarity("fn a() -> bool", "fn b() -> bool");
        assert!((sim - 1.0).abs() < 0.001);
    }
}
