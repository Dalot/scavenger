use rusqlite::Connection;

use crate::config::Config;
use crate::graph::GraphState;
use crate::graph::traversal;
use crate::graph::types::NodeId;
use crate::query::QueryResult;

use super::{CandidateItem, CandidateSource};

fn new_candidate(
    content: String,
    token_count: u32,
    source: CandidateSource,
    node_id: Option<crate::graph::types::NodeId>,
    file_path: Option<String>,
    stale: bool,
    priority_doc: bool,
) -> super::CandidateItem {
    super::CandidateItem {
        content,
        token_count,
        source,
        node_id,
        file_path,
        stale,
        priority_doc,
        score: 0.0,
        pinned: false,
        group: None,
    }
}

/// GATHER stage: collect candidate items from all sources.
pub fn gather(
    conn: &Connection,
    graph: &GraphState,
    config: &Config,
    query_result: &QueryResult,
) -> Vec<CandidateItem> {
    let mut candidates = Vec::new();

    // 1. Target node (always included if present)
    if let Some(ref target_id) = query_result.target {
        if let Some(w) = graph.get_weight(target_id) {
            candidates.push(new_candidate(
                w.skeleton.clone(),
                estimate_tokens(&w.skeleton),
                CandidateSource::Target,
                Some(target_id.clone()),
                Some(w.file_path.to_string_lossy().to_string()),
                false,
                false,
            ));
        }
    }

    // 2. Graph neighbors via traversal
    if let Some(ref target_id) = query_result.target {
        let one_hop = traversal::one_hop_neighbors(graph, target_id);
        for neighbor_id in &one_hop {
            if let Some(w) = graph.get_weight(neighbor_id) {
                let is_caller = graph.callers_of(target_id).iter().any(|c| c.id == *neighbor_id);
                let source = if is_caller {
                    CandidateSource::Caller
                } else {
                    CandidateSource::Callee
                };
                candidates.push(new_candidate(
                    w.skeleton.clone(),
                    estimate_tokens(&w.skeleton),
                    source,
                    Some(neighbor_id.clone()),
                    Some(w.file_path.to_string_lossy().to_string()),
                    false,
                    false,
                ));
            }
        }

        // Extended neighbors from query traversal
        for neighbor_id in &query_result.neighbor_ids {
            if candidates.iter().any(|c| c.node_id.as_ref() == Some(neighbor_id)) {
                continue;
            }
            if let Some(w) = graph.get_weight(neighbor_id) {
                candidates.push(new_candidate(
                    w.skeleton.clone(),
                    estimate_tokens(&w.skeleton),
                    CandidateSource::GraphNode,
                    Some(neighbor_id.clone()),
                    Some(w.file_path.to_string_lossy().to_string()),
                    false,
                    false,
                ));
            }
        }
    }

    // 3. Annotations via FTS5
    if let Some(ref target_id) = query_result.target {
        if let Ok(annotations) = crate::db::queries::get_annotations_for_anchor(
            conn, "node", &target_id.0,
        ) {
            for ann in annotations {
                let stale_marker = if ann.stale { " [STALE \u{26A0}]" } else { "" };
                let content = format!("[annotation{}] {}", stale_marker, ann.text);
                candidates.push(new_candidate(
                    content.clone(),
                    estimate_tokens(&content),
                    CandidateSource::Annotation,
                    Some(target_id.clone()),
                    None,
                    ann.stale,
                    false,
                ));
            }
        }
    }

    // 4. Behavioral signals
    if let Some(ref target_id) = query_result.target {
        gather_signals(conn, target_id, &mut candidates);
    }

    // 5. Doc chunks via FTS5 search
    let search_query = query_result.search_results
        .first()
        .map(|r| r.node_id.0.clone())
        .unwrap_or_default();
    if !search_query.is_empty() {
        if let Ok(doc_matches) = crate::db::queries::search_doc_chunks_fts(conn, &search_query, 5) {
            for m in doc_matches {
                let heading = m.heading.as_deref().unwrap_or("(untitled)");
                let is_priority = config.docs.priority.iter().any(|p| m.file_path.contains(p));
                let content = format!("[doc: {} > {}]\n{}", m.file_path, heading, m.content);
                candidates.push(new_candidate(
                    content.clone(),
                    estimate_tokens(&content),
                    CandidateSource::DocChunk,
                    None,
                    Some(m.file_path),
                    false,
                    is_priority,
                ));
            }
        }
    }

    candidates
}

fn gather_signals(conn: &Connection, target_id: &NodeId, candidates: &mut Vec<CandidateItem>) {
    let mut stmt = match conn.prepare(
        "SELECT kind, detail FROM behavioral_signals WHERE node_id = ?1 ORDER BY timestamp DESC LIMIT 5",
    ) {
        Ok(s) => s,
        Err(_) => return,
    };

    let signals: Vec<(String, Option<String>)> = stmt
        .query_map(rusqlite::params![target_id.0], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .ok()
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default();

    for (kind, detail) in signals {
        let detail_str = detail.as_deref().unwrap_or("");
        let content = format!("[!] {kind}: {detail_str}");
        candidates.push(new_candidate(
            content.clone(),
            estimate_tokens(&content),
            CandidateSource::BehavioralSignal,
            Some(target_id.clone()),
            None,
            false,
            false,
        ));
    }
}

fn estimate_tokens(text: &str) -> u32 {
    (text.len() / 4).max(1) as u32
}
