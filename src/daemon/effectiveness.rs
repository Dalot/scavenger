use rusqlite::Connection;
use serde_json::{Value, json};

/// Per-capsule effectiveness signals.
/// An effective capsule means the agent used the provided context and did not
/// need to fall back to native tools (Read/Grep) for the same file/symbol.
#[derive(Debug, Clone)]
pub struct CapsuleEffectiveness {
    pub capsule_id: String,
    pub file: String,
    pub tokens_served: u32,
    pub follow_up_reads: u32,
    pub follow_up_greps: u32,
    pub edit_within_5min: bool,
    pub score: f64,
}

/// Compute effectiveness for recent capsules in a session.
///
/// Scoring formula:
///   base = 0.5  (capsule was served)
///   -0.15 per follow-up read of the same file within 5 minutes
///   -0.10 per follow-up grep mentioning the same file within 5 minutes
///   +0.3  if an edit happened on the file within 5 minutes (capsule helped)
///   clamp to [0.0, 1.0]
pub fn score_session_capsules(
    meta_conn: &Connection,
    branch_conn: &Connection,
    session_id: &str,
) -> Vec<CapsuleEffectiveness> {
    let capsules = match meta_conn.prepare(
        "SELECT capsule_id, file, tokens_served, timestamp FROM capsule_log WHERE session_id = ?1 ORDER BY timestamp ASC",
    ) {
        Ok(mut stmt) => {
            stmt.query_map(rusqlite::params![session_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .ok()
            .map(|rows| rows.flatten().collect::<Vec<_>>())
            .unwrap_or_default()
        }
        Err(_) => return Vec::new(),
    };

    let mut results = Vec::new();

    for (capsule_id, file, tokens_served, capsule_ts) in &capsules {
        let window_start = *capsule_ts;
        let window_end = capsule_ts + 300; // 5 minutes

        let follow_up_reads = count_session_events(
            branch_conn,
            session_id,
            "read",
            file,
            window_start,
            window_end,
        );

        let follow_up_greps = count_session_events(
            branch_conn,
            session_id,
            "grep",
            file,
            window_start,
            window_end,
        );

        let edit_within_5min = count_session_events(
            branch_conn,
            session_id,
            "edit",
            file,
            window_start,
            window_end,
        ) > 0;

        let mut score = 0.5;
        score -= 0.15 * follow_up_reads as f64;
        score -= 0.10 * follow_up_greps as f64;
        if edit_within_5min {
            score += 0.3;
        }
        // Empty capsules are ineffective
        if *tokens_served == 0 {
            score = 0.0;
        }
        score = score.clamp(0.0, 1.0);

        results.push(CapsuleEffectiveness {
            capsule_id: capsule_id.clone(),
            file: file.clone(),
            tokens_served: *tokens_served,
            follow_up_reads: follow_up_reads as u32,
            follow_up_greps: follow_up_greps as u32,
            edit_within_5min,
            score,
        });
    }

    results
}

fn count_session_events(
    conn: &Connection,
    session_id: &str,
    event_type: &str,
    file: &str,
    ts_start: i64,
    ts_end: i64,
) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM session_log WHERE session_id = ?1 AND event_type = ?2 AND file_path LIKE ?3 AND timestamp BETWEEN ?4 AND ?5",
        rusqlite::params![session_id, event_type, format!("%{file}%"), ts_start, ts_end],
        |row| row.get(0),
    )
    .unwrap_or(0)
}

/// Compute an aggregate effectiveness score for a session.
pub fn session_effectiveness(
    meta_conn: &Connection,
    branch_conn: &Connection,
    session_id: &str,
) -> Value {
    let capsules = score_session_capsules(meta_conn, branch_conn, session_id);

    if capsules.is_empty() {
        return json!({
            "session_id": session_id,
            "capsule_count": 0,
            "effectiveness_score": 0.0,
            "capsules": [],
        });
    }

    let total_score: f64 = capsules.iter().map(|c| c.score).sum();
    let avg_score = total_score / capsules.len() as f64;

    let empty_count = capsules.iter().filter(|c| c.tokens_served == 0).count();
    let edit_hit_count = capsules.iter().filter(|c| c.edit_within_5min).count();
    let fallback_count = capsules
        .iter()
        .filter(|c| c.follow_up_reads > 0 || c.follow_up_greps > 0)
        .count();

    let capsule_details: Vec<Value> = capsules
        .iter()
        .map(|c| {
            json!({
                "capsule_id": c.capsule_id,
                "file": c.file,
                "tokens": c.tokens_served,
                "follow_up_reads": c.follow_up_reads,
                "follow_up_greps": c.follow_up_greps,
                "edit_hit": c.edit_within_5min,
                "score": (c.score * 100.0).round() / 100.0,
            })
        })
        .collect();

    json!({
        "session_id": session_id,
        "capsule_count": capsules.len(),
        "effectiveness_score": (avg_score * 100.0).round() / 100.0,
        "empty_capsules": empty_count,
        "edit_hit_rate": if capsules.len() > 0 { (edit_hit_count as f64 / capsules.len() as f64 * 100.0).round() / 100.0 } else { 0.0 },
        "fallback_rate": if capsules.len() > 0 { (fallback_count as f64 / capsules.len() as f64 * 100.0).round() / 100.0 } else { 0.0 },
        "capsules": capsule_details,
    })
}
