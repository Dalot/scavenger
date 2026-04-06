pub mod budget;
pub mod gather;
pub mod render;
pub mod score;

use budget::CapsuleConstraints;
use rusqlite::Connection;

use crate::config::Config;
use crate::graph::GraphState;
use crate::graph::types::NodeId;
use crate::query::QueryResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputGroup {
    Signal,
    Target,
    Callers,
    Callees,
    Context,
    Documentation,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum CandidateSource {
    Target,
    Caller,
    Callee,
    GraphNode,
    Annotation,
    DocChunk,
    BehavioralSignal,
    NodeHistory,
    SessionActivity,
}

#[derive(Debug, Clone)]
pub struct CandidateItem {
    pub content: String,
    pub token_count: u32,
    pub source: CandidateSource,
    pub node_id: Option<NodeId>,
    pub file_path: Option<String>,
    pub stale: bool,
    pub priority_doc: bool,
    pub score: f64,
    pub pinned: bool,
    pub group: Option<OutputGroup>,
    /// For annotations: the anchor type ('node', 'file', 'scope', or None for project-level)
    pub anchor_type: Option<String>,
    /// For NodeHistory: distance from current version (1 = most recent)
    pub version_distance: Option<u32>,
    /// For NodeHistory: what type of change (signature=1.0, edge=0.7, body=0.4, docstring=0.2)
    pub change_significance: Option<f64>,
    /// For annotations/session: BM25 score from FTS5 search (pre-computed)
    pub bm25_score: Option<f64>,
    /// For annotations/session: creation/update timestamp (epoch seconds)
    pub timestamp: Option<i64>,
    /// For annotations: kind ('fact', 'strategy', 'pitfall', 'context')
    pub annotation_kind: Option<String>,
    /// For annotations: quality score [0.0, 1.0]
    pub quality: Option<f64>,
    /// For annotations: the annotation ID (for retrieval count bumping and edge following)
    pub annotation_id: Option<String>,
}

#[derive(Debug)]
pub struct CapsuleResult {
    pub text: String,
    pub token_count: u32,
    pub items_included: usize,
}

/// Run the full 6-stage capsule assembly pipeline.
/// Stages: GATHER → SCORE → PIN → TRIM → GROUP → RENDER
///
/// Token budget: configurable, default 8000, with 10% headroom.
pub fn assemble(
    conn: &Connection,
    graph: &GraphState,
    config: &Config,
    query_result: &QueryResult,
    budget_override: Option<u32>,
    constraints: &CapsuleConstraints,
) -> CapsuleResult {
    use std::time::Instant;

    let raw_budget = budget_override.unwrap_or(config.budget.default);
    let effective_budget = (raw_budget as f64 * 0.9) as u32;

    // Stage 1: GATHER
    let t0 = Instant::now();
    let mut candidates = gather::gather(conn, graph, config, query_result, constraints);
    let gather_us = t0.elapsed().as_micros() as u64;

    if candidates.is_empty() {
        tracing::debug!(gather_us, "capsule pipeline: empty after gather");
        return CapsuleResult {
            text: String::new(),
            token_count: 0,
            items_included: 0,
        };
    }

    let gathered_count = candidates.len();

    // Stage 2: SCORE
    let t1 = Instant::now();
    let bm25_scores: Vec<(NodeId, f64)> = query_result
        .search_results
        .iter()
        .map(|r| (r.node_id.clone(), r.combined_score))
        .collect();

    score::score(
        &mut candidates,
        graph,
        query_result.target.as_ref(),
        &bm25_scores,
    );
    let score_us = t1.elapsed().as_micros() as u64;

    // Stage 3: PIN
    let t2 = Instant::now();
    render::pin(&mut candidates);
    let pinned_count = candidates.iter().filter(|c| c.pinned).count();
    let pin_us = t2.elapsed().as_micros() as u64;

    // Stage 4: TRIM
    let t3 = Instant::now();
    let pre_trim = candidates.len();
    render::trim(&mut candidates, effective_budget);
    let dropped_count = pre_trim - candidates.len();
    let trim_us = t3.elapsed().as_micros() as u64;

    let annotation_ids: Vec<String> = candidates
        .iter()
        .filter(|c| c.source == CandidateSource::Annotation)
        .filter_map(|c| c.annotation_id.clone())
        .collect();
    if !annotation_ids.is_empty() {
        let _ = crate::db::queries::increment_retrieval_count(conn, &annotation_ids);
    }

    let items_included = candidates.len();

    // Stage 5: GROUP
    let t4 = Instant::now();
    render::group(&mut candidates);
    let group_us = t4.elapsed().as_micros() as u64;

    // Stage 6: RENDER
    let t5 = Instant::now();

    let target_body = query_result.target.as_ref().and_then(|tid| {
        let node = graph.get_weight(tid)?;
        Some(render::TargetBody {
            file_path: node.file_path.to_string_lossy().to_string(),
            line_start: node.line_start,
            line_end: node.line_end,
            name: node.name.clone(),
        })
    });

    let text = render::render(
        &candidates,
        target_body.as_ref(),
        constraints.include_body,
        effective_budget,
    );
    let token_count = (text.len() / 4) as u32;
    let render_us = t5.elapsed().as_micros() as u64;

    let budget_utilization = if effective_budget > 0 {
        token_count as f64 / effective_budget as f64
    } else {
        0.0
    };

    tracing::debug!(
        gathered_count,
        pinned_count,
        dropped_count,
        items_included,
        token_count,
        budget_utilization = format!("{:.2}", budget_utilization),
        gather_us,
        score_us,
        pin_us,
        trim_us,
        group_us,
        render_us,
        "capsule pipeline complete"
    );

    CapsuleResult {
        text,
        token_count,
        items_included,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{NodeKind, NodeWeight};
    use crate::query::intent::IntentResult;
    use std::path::PathBuf;

    #[test]
    fn test_assemble_empty_query() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();

        let graph = GraphState::new();
        let config = Config::default();
        let qr = QueryResult {
            target: None,
            intent: IntentResult::single(crate::query::intent::Intent::Understand),
            neighbor_ids: Vec::new(),
            search_results: Vec::new(),
        };
        let constraints = budget::CapsuleConstraints::from_detail(budget::DetailLevel::Standard);

        let result = assemble(&conn, &graph, &config, &qr, None, &constraints);
        assert!(result.text.is_empty());
    }

    #[test]
    fn test_assemble_with_target() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum)
             VALUES ('n1', 'Function', 'hello', 'src/lib.rs', 1, 5, 'fn hello()', 'aabb0011', 'fn hello()', X'CAFE')",
            [],
        ).unwrap();

        let mut graph = GraphState::new();
        graph.load_from_db(&conn).unwrap();

        let config = Config::default();
        let qr = QueryResult {
            target: Some(NodeId("n1".to_string())),
            intent: IntentResult::single(crate::query::intent::Intent::Understand),
            neighbor_ids: Vec::new(),
            search_results: Vec::new(),
        };
        let constraints = budget::CapsuleConstraints::from_detail(budget::DetailLevel::Standard);

        let result = assemble(&conn, &graph, &config, &qr, None, &constraints);
        assert!(result.text.contains("[TARGET]"));
        assert!(result.text.contains("fn hello()"));
        assert!(result.items_included >= 1);
    }
}
