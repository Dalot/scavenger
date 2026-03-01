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

fn gather_annotations(
    conn: &Connection,
    graph: &GraphState,
    target_id: &NodeId,
    candidates: &mut Vec<CandidateItem>,
) {
    // Node-anchored annotations for the target
    if let Ok(annotations) = crate::db::queries::get_annotations_for_anchor(
        conn, "node", &target_id.0,
    ) {
        for ann in annotations {
            let prefix = if ann.stale { "[STALE \u{26A0}]" } else { "[NOTE]" };
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
            candidates.push(item);
        }
    }

    // File-anchored annotations for the target's file
    if let Some(w) = graph.get_weight(target_id) {
        let file_str = w.file_path.to_string_lossy().to_string();
        if let Ok(annotations) = crate::db::queries::get_annotations_for_anchor(
            conn, "file", &file_str,
        ) {
            for ann in annotations {
                let prefix = if ann.stale { "[STALE \u{26A0}]" } else { "[NOTE]" };
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
                candidates.push(item);
            }
        }
    }

    // Project-level annotations (anchor_type IS NULL)
    if let Ok(annotations) = crate::db::queries::get_project_level_annotations(conn) {
        for ann in annotations {
            let prefix = if ann.stale { "[STALE \u{26A0}]" } else { "[NOTE]" };
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
            item.anchor_type = None; // project-level
            item.timestamp = Some(ann.updated_at);
            candidates.push(item);
        }
    }
}

fn gather_node_history(
    conn: &Connection,
    target_id: &NodeId,
    candidates: &mut Vec<CandidateItem>,
) {
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
        format!("Signature changed: {} → {}", older.signature, newer.signature)
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
    let search_query = query_result.search_results
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
