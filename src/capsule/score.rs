use std::collections::HashSet;

use crate::graph::GraphState;
use crate::graph::types::NodeId;

use super::{CandidateItem, CandidateSource};

/// Context needed by the scorer beyond what's on individual CandidateItems.
pub struct ScoringContext<'a> {
    pub target_file: Option<String>,
    pub one_hop_ids: HashSet<&'a NodeId>,
    pub neighbor_files: HashSet<String>,
}

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

    let ctx = build_scoring_context(graph, target);

    for item in candidates.iter_mut() {
        item.score = match item.source {
            CandidateSource::Target => 1.0,

            CandidateSource::BehavioralSignal => 1.0,

            CandidateSource::Caller | CandidateSource::Callee | CandidateSource::GraphNode => {
                score_graph_node(item, graph, max_centrality, &bm25_map)
            }

            CandidateSource::Annotation => score_annotation(item, target, &ctx),

            CandidateSource::DocChunk => score_doc_chunk(item),

            CandidateSource::NodeHistory => score_node_history(item),

            CandidateSource::SessionActivity => score_session_activity(item),
        };
    }
}

fn build_scoring_context<'a>(
    graph: &'a GraphState,
    target: Option<&'a NodeId>,
) -> ScoringContext<'a> {
    let target_file = target
        .and_then(|t| graph.get_weight(t))
        .map(|w| w.file_path.to_string_lossy().to_string());

    let mut one_hop_ids = HashSet::new();
    let mut neighbor_files = HashSet::new();

    if let Some(t) = target {
        for caller in graph.callers_of(t) {
            one_hop_ids.insert(&caller.id);
            neighbor_files.insert(caller.file_path.to_string_lossy().to_string());
        }
        for callee in graph.callees_of(t) {
            one_hop_ids.insert(&callee.id);
            neighbor_files.insert(callee.file_path.to_string_lossy().to_string());
        }
    }

    ScoringContext {
        target_file,
        one_hop_ids,
        neighbor_files,
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

/// Annotation proximity per design §6.5:
///   1.0  Node(target_id)
///   0.8  File(target's file)
///   0.7  Node(1-hop neighbor of target)
///   0.6  Scope(tag matching target's path)
///   0.5  File(neighbor's file)
///   0.3  None (project-level)
fn compute_proximity(item: &CandidateItem, target: Option<&NodeId>, ctx: &ScoringContext) -> f64 {
    match item.anchor_type.as_deref() {
        Some("node") => {
            if item.node_id.as_ref() == target {
                1.0
            } else if item
                .node_id
                .as_ref()
                .is_some_and(|id| ctx.one_hop_ids.contains(id))
            {
                0.7
            } else {
                0.5
            }
        }
        Some("file") => {
            if let Some(ref fp) = item.file_path {
                if ctx.target_file.as_deref() == Some(fp) {
                    0.8
                } else if ctx.neighbor_files.contains(fp.as_str()) {
                    0.5
                } else {
                    0.4
                }
            } else {
                0.4
            }
        }
        Some("scope") => 0.6,
        _ => 0.3, // project-level (None)
    }
}

/// Annotation: (0.4*bm25 + 0.25*proximity + 0.15*recency + 0.2*quality) * stale * kind
fn score_annotation(item: &CandidateItem, target: Option<&NodeId>, ctx: &ScoringContext) -> f64 {
    let bm25 = item.bm25_score.unwrap_or(0.5);
    let proximity = compute_proximity(item, target, ctx);
    let recency = item.timestamp.map(recency_decay).unwrap_or(0.5);
    let quality = item.quality.unwrap_or(0.5);
    let base = 0.4 * bm25 + 0.25 * proximity + 0.15 * recency + 0.2 * quality;
    let stale_penalty = if item.stale { 0.6 } else { 1.0 };
    let kind_multiplier = match item.annotation_kind.as_deref() {
        Some("pitfall") => 1.2,
        Some("strategy") => 1.1,
        Some("context") => 0.8,
        _ => 1.0,
    };
    (base * stale_penalty * kind_multiplier).clamp(0.0, 1.0)
}

/// DocChunk: 0.7 * bm25_doc + (0.3 if priority else 0.0)
fn score_doc_chunk(item: &CandidateItem) -> f64 {
    let bm25 = item.bm25_score.unwrap_or(0.5);
    let priority_boost = if item.priority_doc { 0.3 } else { 0.0 };
    (0.7 * bm25 + priority_boost).clamp(0.0, 1.0)
}

/// NodeHistory: 0.6 * significance + 0.4 * (1.0 / version_distance)
/// Significance per design §6.5: signature=1.0, edge=0.7, body=0.4, docstring=0.2
fn score_node_history(item: &CandidateItem) -> f64 {
    let significance = item.change_significance.unwrap_or(0.4);
    let distance = item.version_distance.unwrap_or(1).max(1) as f64;
    (0.6 * significance + 0.4 * (1.0 / distance)).clamp(0.0, 1.0)
}

/// SessionActivity: 0.5 * recency + 0.5 * jaccard(activity_nodes, traversal_nodes)
fn score_session_activity(item: &CandidateItem) -> f64 {
    let recency = item.timestamp.map(recency_decay).unwrap_or(0.5);
    // Jaccard computation would need traversal node set — approximate for now
    0.5 * recency + 0.5 * 0.3
}

/// Shared recency decay: e^(-0.01 * hours_elapsed)
fn recency_decay(timestamp_epoch_secs: i64) -> f64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let elapsed_hours = (now - timestamp_epoch_secs).max(0) as f64 / 3600.0;
    (-0.01 * elapsed_hours).exp()
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

    fn make_item(source: CandidateSource, node_id: Option<NodeId>) -> CandidateItem {
        CandidateItem {
            content: "test".to_string(),
            token_count: 3,
            source,
            node_id,
            file_path: None,
            stale: false,
            priority_doc: false,
            score: 0.0,
            pinned: false,
            group: None,
            anchor_type: None,
            version_distance: None,
            change_significance: None,
            bm25_score: None,
            timestamp: None,
            annotation_kind: None,
            quality: None,
            annotation_id: None,
        }
    }

    #[test]
    fn test_target_gets_max_score() {
        let g = make_graph();
        let mut items = vec![make_item(
            CandidateSource::Target,
            Some(NodeId("t1".to_string())),
        )];
        items[0].file_path = Some("src/lib.rs".to_string());
        score(&mut items, &g, Some(&NodeId("t1".to_string())), &[]);
        assert!((items[0].score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_signal_pinned() {
        let g = make_graph();
        let mut items = vec![make_item(
            CandidateSource::BehavioralSignal,
            Some(NodeId("t1".to_string())),
        )];
        score(&mut items, &g, Some(&NodeId("t1".to_string())), &[]);
        assert!((items[0].score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_stale_annotation_penalized() {
        let g = make_graph();
        let target = NodeId("t1".to_string());

        let mut fresh = make_item(CandidateSource::Annotation, Some(target.clone()));
        fresh.anchor_type = Some("node".to_string());
        let mut stale = fresh.clone();
        stale.stale = true;

        score(std::slice::from_mut(&mut fresh), &g, Some(&target), &[]);
        score(std::slice::from_mut(&mut stale), &g, Some(&target), &[]);
        assert!(fresh.score > stale.score);
    }

    #[test]
    fn test_proximity_levels() {
        let mut g = make_graph();
        g.add_node(NodeWeight {
            id: NodeId("n1".to_string()),
            kind: NodeKind::Function,
            name: "neighbor".to_string(),
            file_path: PathBuf::from("src/other.rs"),
            line_start: 1,
            line_end: 10,
            signature: "fn neighbor()".to_string(),
            signature_hash: "cc001122".to_string(),
            docstring: None,
            skeleton: "fn neighbor()".to_string(),
            centrality: 0.1,
            checksum: vec![0xBE, 0xEF],
        });
        g.add_edge(
            &NodeId("t1".to_string()),
            &NodeId("n1".to_string()),
            crate::graph::types::EdgeWeight {
                kind: crate::graph::types::EdgeKind::Calls,
                weight: 1.0,
                confidence: crate::graph::types::Confidence::Precise,
            },
        );

        let target = NodeId("t1".to_string());
        let ctx = build_scoring_context(&g, Some(&target));

        // Node(target) → 1.0
        let mut item = make_item(CandidateSource::Annotation, Some(target.clone()));
        item.anchor_type = Some("node".to_string());
        assert!((compute_proximity(&item, Some(&target), &ctx) - 1.0).abs() < f64::EPSILON);

        // Node(1-hop neighbor) → 0.7
        item.node_id = Some(NodeId("n1".to_string()));
        assert!((compute_proximity(&item, Some(&target), &ctx) - 0.7).abs() < f64::EPSILON);

        // File(target's file) → 0.8
        let mut file_item = make_item(CandidateSource::Annotation, None);
        file_item.anchor_type = Some("file".to_string());
        file_item.file_path = Some("src/lib.rs".to_string());
        assert!((compute_proximity(&file_item, Some(&target), &ctx) - 0.8).abs() < f64::EPSILON);

        // None (project-level) → 0.3
        let mut proj_item = make_item(CandidateSource::Annotation, None);
        proj_item.anchor_type = None;
        assert!((compute_proximity(&proj_item, Some(&target), &ctx) - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_pitfall_scored_higher_than_fact() {
        let g = make_graph();
        let target = NodeId("t1".to_string());

        let mut pitfall = make_item(CandidateSource::Annotation, Some(target.clone()));
        pitfall.anchor_type = Some("node".to_string());
        pitfall.annotation_kind = Some("pitfall".to_string());

        let mut fact = make_item(CandidateSource::Annotation, Some(target.clone()));
        fact.anchor_type = Some("node".to_string());
        fact.annotation_kind = Some("fact".to_string());

        score(std::slice::from_mut(&mut pitfall), &g, Some(&target), &[]);
        score(std::slice::from_mut(&mut fact), &g, Some(&target), &[]);
        assert!(
            pitfall.score > fact.score,
            "pitfall ({}) should rank higher than fact ({})",
            pitfall.score,
            fact.score
        );
    }

    #[test]
    fn test_context_scored_lower_than_fact() {
        let g = make_graph();
        let target = NodeId("t1".to_string());

        let mut context = make_item(CandidateSource::Annotation, Some(target.clone()));
        context.anchor_type = Some("node".to_string());
        context.annotation_kind = Some("context".to_string());

        let mut fact = make_item(CandidateSource::Annotation, Some(target.clone()));
        fact.anchor_type = Some("node".to_string());
        fact.annotation_kind = Some("fact".to_string());

        score(std::slice::from_mut(&mut context), &g, Some(&target), &[]);
        score(std::slice::from_mut(&mut fact), &g, Some(&target), &[]);
        assert!(
            context.score < fact.score,
            "context ({}) should rank lower than fact ({})",
            context.score,
            fact.score
        );
    }

    #[test]
    fn test_quality_affects_score() {
        let g = make_graph();
        let target = NodeId("t1".to_string());

        let mut high_q = make_item(CandidateSource::Annotation, Some(target.clone()));
        high_q.anchor_type = Some("node".to_string());
        high_q.quality = Some(0.9);

        let mut low_q = make_item(CandidateSource::Annotation, Some(target.clone()));
        low_q.anchor_type = Some("node".to_string());
        low_q.quality = Some(0.1);

        score(std::slice::from_mut(&mut high_q), &g, Some(&target), &[]);
        score(std::slice::from_mut(&mut low_q), &g, Some(&target), &[]);
        assert!(
            high_q.score > low_q.score,
            "high quality ({}) should rank higher than low quality ({})",
            high_q.score,
            low_q.score
        );
    }

    #[test]
    fn test_scoring_weights_sum_to_one() {
        let sum: f64 = 0.4 + 0.25 + 0.15 + 0.2;
        assert!(
            (sum - 1.0).abs() < f64::EPSILON,
            "weights should sum to 1.0, got {sum}"
        );
    }

    #[test]
    fn test_node_history_significance() {
        let mut sig_change =
            make_item(CandidateSource::NodeHistory, Some(NodeId("t1".to_string())));
        sig_change.change_significance = Some(1.0);
        sig_change.version_distance = Some(1);

        let mut body_change =
            make_item(CandidateSource::NodeHistory, Some(NodeId("t1".to_string())));
        body_change.change_significance = Some(0.4);
        body_change.version_distance = Some(2);

        let sig_score = score_node_history(&sig_change);
        let body_score = score_node_history(&body_change);
        assert!(
            sig_score > body_score,
            "signature change should rank higher than body change"
        );
    }
}
