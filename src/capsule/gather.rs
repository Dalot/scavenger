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

fn annotation_prefix(kind: &str, stale: bool) -> &'static str {
    if stale {
        return "[STALE]";
    }
    match kind {
        "strategy" => "[STRATEGY]",
        "pitfall" => "[PITFALL]",
        "context" => "[CONTEXT NOTE]",
        _ => "[NOTE]",
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
                let is_caller = graph
                    .callers_of(target_id)
                    .iter()
                    .any(|c| c.id == *neighbor_id);
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
            if candidates
                .iter()
                .any(|c| c.node_id.as_ref() == Some(neighbor_id))
            {
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

    // 3. Annotations — node-anchored, file-anchored, and project-level
    if let Some(ref target_id) = query_result.target {
        gather_annotations(conn, graph, target_id, &mut candidates);
    }

    // 4. Node version history for target
    if let Some(ref target_id) = query_result.target {
        gather_node_history(conn, target_id, &mut candidates);
    }

    // 5. Behavioral signals
    if let Some(ref target_id) = query_result.target {
        gather_signals(conn, target_id, &mut candidates);
    }

    // 6. Doc chunks via FTS5 search + unconditional priority docs
    gather_doc_chunks(conn, config, query_result, &mut candidates);

    candidates
}

const DISTILL_THRESHOLD: usize = 5;
const DISTILL_KEEP: usize = 3;
const PROJECT_LEVEL_CAP: usize = 3;

fn gather_annotations(
    conn: &Connection,
    graph: &GraphState,
    target_id: &NodeId,
    candidates: &mut Vec<CandidateItem>,
) {
    // Node-anchored annotations for the target
    if let Ok(mut annotations) =
        crate::db::queries::get_annotations_for_anchor(conn, "node", &target_id.0)
    {
        if annotations.len() > DISTILL_THRESHOLD {
            annotations.sort_by(|a, b| {
                b.quality
                    .partial_cmp(&a.quality)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let overflow = annotations.len() - DISTILL_KEEP;
            let summary_text = format!("[+{overflow} more notes on {}]", target_id.0);
            let mut summary = new_candidate(
                summary_text.clone(),
                estimate_tokens(&summary_text),
                CandidateSource::Annotation,
                Some(target_id.clone()),
                None,
                false,
                false,
            );
            summary.anchor_type = Some("node".to_string());
            candidates.push(summary);
            annotations.truncate(DISTILL_KEEP);
        }
        for ann in annotations {
            let prefix = annotation_prefix(&ann.kind, ann.stale);
            let content = format!("{prefix} {}", ann.text);
            let mut item = new_candidate(
                content.clone(),
                estimate_tokens(&content),
                CandidateSource::Annotation,
                Some(target_id.clone()),
                None,
                ann.stale,
                false,
            );
            item.anchor_type = Some("node".to_string());
            item.timestamp = Some(ann.updated_at);
            item.annotation_kind = Some(ann.kind.clone());
            item.quality = Some(ann.quality);
            item.annotation_id = Some(ann.id.clone());
            candidates.push(item);
        }
    }

    // File-anchored annotations for the target's file
    if let Some(w) = graph.get_weight(target_id) {
        let file_str = w.file_path.to_string_lossy().to_string();
        if let Ok(mut annotations) =
            crate::db::queries::get_annotations_for_anchor(conn, "file", &file_str)
        {
            if annotations.len() > DISTILL_THRESHOLD {
                annotations.sort_by(|a, b| {
                    b.quality
                        .partial_cmp(&a.quality)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                let overflow = annotations.len() - DISTILL_KEEP;
                let summary_text = format!("[+{overflow} more notes on {file_str}]");
                let mut summary = new_candidate(
                    summary_text.clone(),
                    estimate_tokens(&summary_text),
                    CandidateSource::Annotation,
                    None,
                    Some(file_str.clone()),
                    false,
                    false,
                );
                summary.anchor_type = Some("file".to_string());
                candidates.push(summary);
                annotations.truncate(DISTILL_KEEP);
            }
            for ann in annotations {
                let prefix = annotation_prefix(&ann.kind, ann.stale);
                let content = format!("{prefix} {}", ann.text);
                let mut item = new_candidate(
                    content.clone(),
                    estimate_tokens(&content),
                    CandidateSource::Annotation,
                    None,
                    Some(file_str.clone()),
                    ann.stale,
                    false,
                );
                item.anchor_type = Some("file".to_string());
                item.timestamp = Some(ann.updated_at);
                item.annotation_kind = Some(ann.kind.clone());
                item.quality = Some(ann.quality);
                item.annotation_id = Some(ann.id.clone());
                candidates.push(item);
            }
        }
    }

    // Project-level annotations (anchor_type IS NULL), capped at PROJECT_LEVEL_CAP
    if let Ok(mut annotations) = crate::db::queries::get_project_level_annotations(conn) {
        annotations.sort_by(|a, b| {
            b.quality
                .partial_cmp(&a.quality)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        annotations.truncate(PROJECT_LEVEL_CAP);
        for ann in annotations {
            let prefix = annotation_prefix(&ann.kind, ann.stale);
            let content = format!("{prefix} {}", ann.text);
            let mut item = new_candidate(
                content.clone(),
                estimate_tokens(&content),
                CandidateSource::Annotation,
                None,
                None,
                ann.stale,
                false,
            );
            item.anchor_type = None;
            item.timestamp = Some(ann.updated_at);
            item.annotation_kind = Some(ann.kind.clone());
            item.quality = Some(ann.quality);
            item.annotation_id = Some(ann.id.clone());
            candidates.push(item);
        }
    }
}

fn gather_node_history(conn: &Connection, target_id: &NodeId, candidates: &mut Vec<CandidateItem>) {
    if let Some(sig_hash) = crate::db::queries::get_node_signature_hash(conn, &target_id.0) {
        if let Ok(versions) = crate::memory::versions::get_recent_versions(conn, &sig_hash, 5) {
            if versions.len() < 2 {
                return;
            }
            // Compare consecutive versions to determine change type
            for (i, ver) in versions.iter().enumerate().skip(1) {
                let prev = &versions[i - 1];
                let significance = compute_change_significance(prev, ver);
                let change_desc = describe_change(prev, ver);
                let content = format!("[CHANGED] {change_desc}");
                let mut item = new_candidate(
                    content.clone(),
                    estimate_tokens(&content),
                    CandidateSource::NodeHistory,
                    Some(target_id.clone()),
                    None,
                    false,
                    false,
                );
                item.version_distance = Some(i as u32);
                item.change_significance = Some(significance);
                item.timestamp = Some(ver.created_at);
                candidates.push(item);
            }
        }
    }
}

fn compute_change_significance(
    newer: &crate::memory::versions::VersionInfo,
    older: &crate::memory::versions::VersionInfo,
) -> f64 {
    if newer.signature != older.signature {
        1.0
    } else if newer.edges_json != older.edges_json {
        0.7
    } else {
        0.4 // body change (default — we don't have docstring-only detection here)
    }
}

fn describe_change(
    newer: &crate::memory::versions::VersionInfo,
    older: &crate::memory::versions::VersionInfo,
) -> String {
    if newer.signature != older.signature {
        format!(
            "Signature changed: {} → {}",
            older.signature, newer.signature
        )
    } else if newer.edges_json != older.edges_json {
        "Dependencies changed".to_string()
    } else {
        "Body modified".to_string()
    }
}

fn gather_doc_chunks(
    conn: &Connection,
    config: &Config,
    query_result: &QueryResult,
    candidates: &mut Vec<CandidateItem>,
) {
    let mut seen_files = std::collections::HashSet::new();

    // Priority docs are unconditionally included (design §6.5)
    for priority_name in &config.docs.priority {
        if let Ok(chunks) = crate::db::queries::get_doc_chunks_for_file(conn, priority_name) {
            for m in chunks {
                let heading = m.heading.as_deref().unwrap_or("(untitled)");
                let content = format!("[doc: {} > {}]\n{}", m.file_path, heading, m.content);
                candidates.push(new_candidate(
                    content.clone(),
                    estimate_tokens(&content),
                    CandidateSource::DocChunk,
                    None,
                    Some(m.file_path.clone()),
                    false,
                    true,
                ));
                seen_files.insert(m.file_path);
            }
        }
    }

    // FTS5 search for additional doc chunks
    let search_query = query_result
        .search_results
        .first()
        .map(|r| r.node_id.0.clone())
        .unwrap_or_default();
    if !search_query.is_empty() {
        if let Ok(doc_matches) = crate::db::queries::search_doc_chunks_fts(conn, &search_query, 5) {
            for m in doc_matches {
                if seen_files.contains(&m.file_path) {
                    continue;
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{NodeKind, NodeWeight};
    use std::path::PathBuf;

    fn setup() -> (rusqlite::Connection, GraphState) {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();

        let mut graph = GraphState::new();
        conn.execute(
            "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum)
             VALUES ('n1', 'Function', 'target_fn', 'src/lib.rs', 1, 10, 'fn target_fn()', 'aabb0011', 'fn target_fn()', X'CAFE')",
            [],
        ).unwrap();
        graph.load_from_db(&conn).unwrap();
        (conn, graph)
    }

    #[test]
    fn test_distill_under_threshold_no_distillation() {
        let (conn, graph) = setup();
        for i in 0..4 {
            crate::db::queries::insert_annotation(
                &conn,
                &format!("a{i}"),
                Some("node"),
                Some("n1"),
                &format!("note {i}"),
                None,
                "fact",
                None,
                1000 + i,
            )
            .unwrap();
        }

        let mut candidates = Vec::new();
        gather_annotations(&conn, &graph, &NodeId("n1".to_string()), &mut candidates);
        let ann_count = candidates
            .iter()
            .filter(|c| c.source == CandidateSource::Annotation)
            .count();
        assert_eq!(
            ann_count, 4,
            "4 annotations should pass through without distillation"
        );
    }

    #[test]
    fn test_distill_over_threshold() {
        let (conn, graph) = setup();
        for i in 0..8 {
            crate::db::queries::insert_annotation(
                &conn,
                &format!("a{i}"),
                Some("node"),
                Some("n1"),
                &format!("note {i}"),
                None,
                "fact",
                None,
                1000 + i,
            )
            .unwrap();
        }

        let mut candidates = Vec::new();
        gather_annotations(&conn, &graph, &NodeId("n1".to_string()), &mut candidates);
        let ann_items: Vec<_> = candidates
            .iter()
            .filter(|c| {
                c.source == CandidateSource::Annotation && c.anchor_type.as_deref() == Some("node")
            })
            .collect();

        // 3 full items + 1 summary = 4
        assert_eq!(
            ann_items.len(),
            4,
            "should have 3 distilled + 1 summary, got {}",
            ann_items.len()
        );

        let summary = ann_items.iter().find(|c| c.content.contains("[+")).unwrap();
        assert!(
            summary.content.contains("[+5 more notes on n1]"),
            "summary should mention overflow count: {}",
            summary.content
        );
    }

    #[test]
    fn test_project_level_capped_at_3() {
        let (conn, graph) = setup();
        for i in 0..6 {
            crate::db::queries::insert_annotation(
                &conn,
                &format!("p{i}"),
                None,
                None,
                &format!("project note {i}"),
                None,
                "fact",
                None,
                1000 + i,
            )
            .unwrap();
        }

        let mut candidates = Vec::new();
        gather_annotations(&conn, &graph, &NodeId("n1".to_string()), &mut candidates);
        let project_items: Vec<_> = candidates
            .iter()
            .filter(|c| c.source == CandidateSource::Annotation && c.anchor_type.is_none())
            .collect();
        assert_eq!(
            project_items.len(),
            3,
            "project-level should be capped at 3, got {}",
            project_items.len()
        );
    }

    #[test]
    fn test_annotation_prefix_by_kind() {
        assert_eq!(annotation_prefix("fact", false), "[NOTE]");
        assert_eq!(annotation_prefix("strategy", false), "[STRATEGY]");
        assert_eq!(annotation_prefix("pitfall", false), "[PITFALL]");
        assert_eq!(annotation_prefix("context", false), "[CONTEXT NOTE]");
        assert_eq!(annotation_prefix("fact", true), "[STALE]");
        assert_eq!(annotation_prefix("pitfall", true), "[STALE]");
    }
}
