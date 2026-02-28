use std::sync::Arc;

use serde_json::{json, Value};

use super::DaemonState;
use crate::capsule;
use crate::db::queries;
use crate::query;

/// Dispatch an incoming JSON request to the appropriate handler.
pub async fn dispatch(state: &Arc<DaemonState>, request: Value) -> Value {
    let method = request
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    match method {
        "status" => handle_status(state).await,
        "capsule" => handle_capsule(state, &request).await,
        "hook_pre" => handle_hook_pre(state, &request).await,
        "hook_post" => handle_hook_post(state, &request).await,
        "annotation_read" => handle_annotation_read(state, &request).await,
        "annotation_write" => handle_annotation_write(state, &request).await,
        "annotation_delete" => handle_annotation_delete(state, &request).await,
        "search_docs" => handle_search_docs(state, &request).await,
        _ => json!({ "error": format!("unknown method: {method}") }),
    }
}

async fn handle_status(state: &Arc<DaemonState>) -> Value {
    let graph = state.graph.read();
    let branch = state.current_branch.read().clone();
    let reindex_state = *state.reindex_state.read();
    json!({
        "status": "ok",
        "branch": branch,
        "reindex_state": reindex_state.to_string(),
        "node_count": graph.node_count(),
        "edge_count": graph.edge_count(),
        "pid": std::process::id(),
    })
}

async fn handle_capsule(state: &Arc<DaemonState>, request: &Value) -> Value {
    let file = request.get("file").and_then(|v| v.as_str()).unwrap_or("");
    let symbol = request.get("symbol").and_then(|v| v.as_str());
    let query_str = request.get("query").and_then(|v| v.as_str());
    let budget = request
        .get("budget")
        .and_then(|v| v.as_u64())
        .map(|b| b as u32);

    let graph = state.graph.read();

    let conn_guard = state.branch_db.lock();
    let Some(ref conn) = *conn_guard else {
        return json!({ "capsule": "", "token_count": 0 });
    };

    let query_result = query::run_query(conn, &graph, &state.config, file, symbol, query_str);

    let result = capsule::assemble(conn, &graph, &state.config, &query_result, budget);

    // Token logging
    let estimated = crate::graph::estimator::estimate_without_index(conn, "get_capsule", Some(file));
    {
        let session = state.session_id.read().clone();
        let branch = state.current_branch.read().clone();
        let intent_str = format!("{:?}", query_result.intent.primary);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let meta = state.meta_db.lock();
        let _ = queries::insert_token_log(
            &meta,
            now,
            &session,
            &branch,
            "get_capsule",
            query_str,
            Some(&intent_str),
            result.token_count,
            estimated,
            Some(file),
        );
    }

    json!({
        "capsule": result.text,
        "token_count": result.token_count,
        "items_included": result.items_included,
    })
}

async fn handle_hook_pre(state: &Arc<DaemonState>, request: &Value) -> Value {
    // PreToolUse (Read): generate capsule as additional context
    let file = request.get("file").and_then(|v| v.as_str()).unwrap_or("");
    if file.is_empty() {
        return json!({});
    }

    let capsule_req = json!({
        "method": "capsule",
        "file": file,
    });
    let result = handle_capsule(state, &capsule_req).await;
    let capsule_text = result.get("capsule").and_then(|v| v.as_str()).unwrap_or("");

    if capsule_text.is_empty() {
        json!({})
    } else {
        json!({ "additionalContext": capsule_text })
    }
}

async fn handle_hook_post(state: &Arc<DaemonState>, request: &Value) -> Value {
    if let Some(file) = request.get("file").and_then(|v| v.as_str()) {
        let session = state.session_id.read().clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Log session event
        if let Some(ref conn) = *state.branch_db.lock() {
            let _ = queries::insert_session_event(conn, &session, "edit", Some(file), None, now);
        }

        // Trigger immediate incremental re-index (Phase 1: prep without lock)
        let prep = {
            let db_guard = state.branch_db.lock();
            let Some(ref conn) = *db_guard else {
                return json!({});
            };
            let graph = state.graph.read();
            match crate::graph::index::incremental_reindex_prep(conn, &graph, file) {
                Ok(p) => p,
                Err(_) => return json!({}),
            }
        };

        // Phase 2: swap under write lock
        {
            let db_guard = state.branch_db.lock();
            if let Some(ref conn) = *db_guard {
                let mut graph = state.graph.write();
                let _ = crate::graph::index::incremental_reindex_swap(conn, &mut graph, prep);
                graph.compute_pagerank(0.85, 30);
                let _ = graph.save_centrality(conn);
            }
        }
    }
    json!({})
}

async fn handle_annotation_read(state: &Arc<DaemonState>, request: &Value) -> Value {
    let anchor_type = request.get("anchor_type").and_then(|v| v.as_str());
    let anchor_value = request.get("anchor_value").and_then(|v| v.as_str());
    let query = request.get("query").and_then(|v| v.as_str());
    let tags_filter = request.get("tags").and_then(|v| v.as_str());
    let session_summary_mode = request
        .get("session_summary")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let limit = request
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as u32;

    let conn_guard = state.branch_db.lock();
    let Some(ref conn) = *conn_guard else {
        return json!({ "annotations": [] });
    };

    // Session summary mode
    if session_summary_mode {
        let session_id = state.session_id.read().clone();
        let summary = crate::memory::session::session_summary(conn, &session_id)
            .unwrap_or_else(|_| crate::memory::session::SessionSummary {
                session_id: session_id.clone(),
                total_events: 0,
                unique_files: 0,
                unique_symbols: 0,
                recent_events: vec![],
                files_touched: vec![],
            });

        let active_signals = crate::memory::signals::active_signal_count(conn).unwrap_or(0);

        // Gather stale annotations
        let stale: Vec<Value> = conn
            .prepare("SELECT id, anchor_type, anchor_value, text FROM annotations WHERE stale = TRUE LIMIT 20")
            .ok()
            .map(|mut stmt| {
                stmt.query_map([], |row| {
                    Ok(json!({
                        "id": row.get::<_, String>(0)?,
                        "anchor_type": row.get::<_, Option<String>>(1)?,
                        "anchor_value": row.get::<_, Option<String>>(2)?,
                        "text": row.get::<_, String>(3)?,
                    }))
                })
                .ok()
                .map(|rows| rows.flatten().collect())
                .unwrap_or_default()
            })
            .unwrap_or_default();

        let events: Vec<Value> = summary
            .recent_events
            .iter()
            .map(|e| {
                json!({
                    "event_type": e.event_type,
                    "file_path": e.file_path,
                    "symbol": e.symbol,
                    "timestamp": e.timestamp,
                })
            })
            .collect();

        return json!({
            "session_id": summary.session_id,
            "total_events": summary.total_events,
            "unique_files": summary.unique_files,
            "unique_symbols": summary.unique_symbols,
            "recent_events": events,
            "files_touched": summary.files_touched,
            "active_signals": active_signals,
            "stale_annotations": stale,
        });
    }

    // FTS query mode
    if let Some(q) = query {
        match queries::search_annotations_fts(conn, q, limit) {
            Ok(matches) => {
                let results: Vec<Value> = matches
                    .iter()
                    .map(|m| json!({ "id": m.id, "rank": m.rank }))
                    .collect();
                json!({ "annotations": results })
            }
            Err(_) => json!({ "annotations": [] }),
        }
    } else if let (Some(at), Some(av)) = (anchor_type, anchor_value) {
        match queries::get_annotations_for_anchor(conn, at, av) {
            Ok(rows) => {
                let mut results: Vec<Value> = rows
                    .iter()
                    .map(|r| {
                        json!({
                            "id": r.id,
                            "text": r.text,
                            "tags": r.tags,
                            "stale": r.stale,
                        })
                    })
                    .collect();

                // Apply tags filter if provided
                if let Some(tag_filter) = tags_filter {
                    results.retain(|r| {
                        r.get("tags")
                            .and_then(|t| t.as_str())
                            .is_some_and(|t| t.contains(tag_filter))
                    });
                }

                json!({ "annotations": results })
            }
            Err(_) => json!({ "annotations": [] }),
        }
    } else {
        json!({ "annotations": [] })
    }
}

async fn handle_annotation_write(state: &Arc<DaemonState>, request: &Value) -> Value {
    let text = request.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if text.is_empty() {
        return json!({ "error": "text is required" });
    }

    let id = request
        .get("id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let mut anchor_type = request.get("anchor_type").and_then(|v| v.as_str()).map(String::from);
    let mut anchor_value = request.get("anchor_value").and_then(|v| v.as_str()).map(String::from);
    let tags = request.get("tags").and_then(|v| v.as_str());

    // Anchor resolution cascade: if anchor_type is "node" and anchor_value
    // looks like a symbol name (not a NodeId hash), resolve via FTS5
    let conn_guard = state.branch_db.lock();
    let Some(ref conn) = *conn_guard else {
        return json!({ "error": "no database" });
    };

    let mut note: Option<String> = None;

    if anchor_type.as_deref() == Some("node") {
        if let Some(ref av) = anchor_value {
            if !av.chars().all(|c| c.is_ascii_hexdigit()) || av.len() != 32 {
                // Looks like a symbol name, resolve via FTS5
                match queries::search_nodes_fts(conn, av, 5) {
                    Ok(matches) if !matches.is_empty() => {
                        anchor_value = Some(matches[0].id.clone());

                        // Disambiguation: if multiple matches within 20% of top score
                        if matches.len() > 1 {
                            let top_rank = matches[0].rank.abs();
                            let threshold = top_rank * 1.2;
                            let close_matches: Vec<&str> = matches
                                .iter()
                                .filter(|m| m.rank.abs() <= threshold)
                                .map(|m| m.id.as_str())
                                .collect();
                            if close_matches.len() > 1 {
                                note = Some(format!(
                                    "Disambiguated: {} candidates within 20%. Selected: {}. Alternatives: {}",
                                    close_matches.len(),
                                    close_matches[0],
                                    close_matches[1..].join(", ")
                                ));
                            }
                        }
                    }
                    _ => {
                        // Symbol not found via FTS5, fall through to file/scope/None
                        anchor_type = None;
                        anchor_value = None;
                    }
                }
            }
        }
    }

    let at_str = anchor_type.as_deref();
    let av_str = anchor_value.as_deref();

    match crate::memory::annotations::upsert_annotation(conn, &id, at_str.and_then(crate::memory::annotations::AnchorType::from_str), av_str, text, tags) {
        Ok(()) => {
            let mut response = json!({ "id": id, "status": "created" });
            if let Some(n) = note {
                response["note"] = json!(n);
            }
            response
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn handle_annotation_delete(state: &Arc<DaemonState>, request: &Value) -> Value {
    let id = request.get("id").and_then(|v| v.as_str()).unwrap_or("");
    if id.is_empty() {
        return json!({ "error": "id is required" });
    }

    let conn_guard = state.branch_db.lock();
    let Some(ref conn) = *conn_guard else {
        return json!({ "error": "no database" });
    };

    match conn.execute("DELETE FROM annotations WHERE id = ?1", rusqlite::params![id]) {
        Ok(0) => json!({ "error": "not found" }),
        Ok(_) => json!({ "status": "deleted" }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn handle_search_docs(state: &Arc<DaemonState>, request: &Value) -> Value {
    let query = request.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let limit = request
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as u32;

    if query.is_empty() {
        return json!({ "results": [] });
    }

    let conn_guard = state.branch_db.lock();
    let Some(ref conn) = *conn_guard else {
        return json!({ "results": [] });
    };

    match queries::search_doc_chunks_fts(conn, query, limit) {
        Ok(matches) => {
            let results: Vec<Value> = matches
                .iter()
                .map(|m| {
                    json!({
                        "file_path": m.file_path,
                        "chunk_index": m.chunk_index,
                        "heading": m.heading,
                        "content": m.content,
                        "token_estimate": m.token_estimate,
                        "rank": m.rank,
                    })
                })
                .collect();
            json!({ "results": results })
        }
        Err(_) => json!({ "results": [] }),
    }
}
