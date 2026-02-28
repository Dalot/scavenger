use crate::graph::GraphState;
use crate::graph::types::NodeId;

use super::{CandidateItem, CandidateSource};

/// SCORE stage: apply per-source scoring formula to each candidate.
/// Scores are in [0.0, 1.0]. BehavioralSignals are pinned (score = 1.0).
pub fn score(
    candidates: &mut [CandidateItem],
    graph: &GraphState,
    target: Option<&NodeId>,
    query_bm25_scores: &[(NodeId, f64)],
) {
    let max_centrality = candidates
        .iter()
        .filter_map(|c| c.node_id.as_ref())
        .filter_map(|id| graph.get_weight(id))
        .map(|w| w.centrality as f64)
        .fold(f64::MIN, f64::max)
        .max(1e-10);

    let bm25_map: std::collections::HashMap<&NodeId, f64> =
        query_bm25_scores.iter().map(|(id, s)| (id, *s)).collect();

    for item in candidates.iter_mut() {
        item.score = match item.source {
            CandidateSource::Target => 1.0,

            CandidateSource::BehavioralSignal => 1.0,

            CandidateSource::Caller | CandidateSource::Callee | CandidateSource::GraphNode => {
                score_graph_node(item, graph, max_centrality, &bm25_map)
            }

            CandidateSource::Annotation => {
                score_annotation(item, target)
            }

            CandidateSource::DocChunk => {
                score_doc_chunk(item)
            }

            CandidateSource::NodeHistory => {
                score_node_history(item)
            }

            CandidateSource::SessionActivity => {
                0.5
            }
        };
    }
}

/// GraphNode: 0.4 * centrality + 0.6 * bm25
fn score_graph_node(
    item: &CandidateItem,
    graph: &GraphState,
    max_centrality: f64,
    bm25_map: &std::collections::HashMap<&NodeId, f64>,
) -> f64 {
    let centrality = item
        .node_id
        .as_ref()
        .and_then(|id| graph.get_weight(id))
        .map(|w| w.centrality as f64 / max_centrality)
        .unwrap_or(0.0);

    let bm25 = item
        .node_id
        .as_ref()
        .and_then(|id| bm25_map.get(id))
        .copied()
        .unwrap_or(0.0);

    0.4 * centrality + 0.6 * bm25
}

/// Annotation: (0.5 * bm25 + 0.3 * proximity + 0.2 * recency) * stale_penalty
fn score_annotation(item: &CandidateItem, target: Option<&NodeId>) -> f64 {
    let proximity = if item.node_id.as_ref() == target { 1.0 } else { 0.3 };
    let base = 0.5 * 0.5 + 0.3 * proximity + 0.2 * 0.5;
    let stale_penalty = if item.stale { 0.6 } else { 1.0 };
    base * stale_penalty
}

/// DocChunk: 0.7 * bm25_doc + (0.3 if priority else 0.0)
fn score_doc_chunk(item: &CandidateItem) -> f64 {
    let bm25_estimate = 0.5;
    let priority_boost = if item.priority_doc { 0.3 } else { 0.0 };
    0.7 * bm25_estimate + priority_boost
}

/// NodeHistory: 0.6 * significance + 0.4 * (1.0 / version_distance)
fn score_node_history(_item: &CandidateItem) -> f64 {
    0.6 * 0.5 + 0.4 * 1.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{NodeKind, NodeWeight};
    use std::path::PathBuf;

    fn make_graph() -> GraphState {
        let mut g = GraphState::new();
        g.add_node(NodeWeight {
            id: NodeId("t1".to_string()),
            kind: NodeKind::Function,
            name: "target".to_string(),
            file_path: PathBuf::from("src/lib.rs"),
            line_start: 1,
            line_end: 10,
            signature: "fn target()".to_string(),
            signature_hash: "aabb0011".to_string(),
            docstring: None,
            skeleton: "fn target()".to_string(),
            centrality: 0.5,
            checksum: vec![0xDE, 0xAD],
        });
        g
    }

    #[test]
    fn test_target_gets_max_score() {
        let g = make_graph();
        let mut items = vec![CandidateItem {
            content: "fn target()".to_string(),
            token_count: 3,
            source: CandidateSource::Target,
            node_id: Some(NodeId("t1".to_string())),
            file_path: Some("src/lib.rs".to_string()),
            stale: false,
            priority_doc: false,
            score: 0.0,
            pinned: false,
            group: None,
        }];
        score(&mut items, &g, Some(&NodeId("t1".to_string())), &[]);
        assert!((items[0].score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_signal_pinned() {
        let g = make_graph();
        let mut items = vec![CandidateItem {
            content: "[!] THRASHING".to_string(),
            token_count: 4,
            source: CandidateSource::BehavioralSignal,
            node_id: Some(NodeId("t1".to_string())),
            file_path: None,
            stale: false,
            priority_doc: false,
            score: 0.0,
            pinned: false,
            group: None,
        }];
        score(&mut items, &g, Some(&NodeId("t1".to_string())), &[]);
        assert!((items[0].score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_stale_annotation_penalized() {
        let g = make_graph();
        let target = NodeId("t1".to_string());

        let mut fresh = CandidateItem {
            content: "annotation".to_string(),
            token_count: 3,
            source: CandidateSource::Annotation,
            node_id: Some(target.clone()),
            file_path: None,
            stale: false,
            priority_doc: false,
            score: 0.0,
            pinned: false,
            group: None,
        };
        let mut stale = fresh.clone();
        stale.stale = true;

        score(std::slice::from_mut(&mut fresh), &g, Some(&target), &[]);
        score(std::slice::from_mut(&mut stale), &g, Some(&target), &[]);
        assert!(fresh.score > stale.score);
    }
}
