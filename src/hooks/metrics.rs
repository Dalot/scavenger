use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::json;

/// A single metrics event logged by the audit hook.
#[derive(Debug, Serialize, Deserialize)]
pub struct MetricsEvent {
    pub timestamp: String,
    pub event: String,
    pub conversation_id: String,
    pub generation_id: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_bytes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_usage_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

/// Handle the generic audit hook. Reads stdin JSON from any Cursor hook event,
/// extracts key metrics, and appends to `.scavenger/metrics/<conversation_id>.jsonl`.
pub fn handle_audit(scavenger_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let input = match super::read_stdin_json() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("scavenger audit: failed to read stdin: {e}");
            super::print_json(&json!({}));
            return Ok(());
        }
    };

    // Claude Code sends "session_id", Cursor sends "conversation_id".
    let event_name = input
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let conversation_id = input
        .get("conversation_id")
        .or_else(|| input.get("session_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let generation_id = input
        .get("generation_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let model = input
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut evt = MetricsEvent {
        timestamp: chrono::Utc::now().to_rfc3339(),
        event: event_name.clone(),
        conversation_id: conversation_id.clone(),
        generation_id,
        model,
        tool_name: None,
        input_bytes: None,
        output_bytes: None,
        duration_ms: None,
        context_tokens: None,
        context_window_size: None,
        context_usage_pct: None,
        status: None,
        loop_count: None,
        file_path: None,
    };

    match event_name.as_str() {
        // Cursor camelCase events
        "postToolUse" => {
            evt.tool_name = input.get("tool_name").and_then(|v| v.as_str()).map(String::from);
            evt.input_bytes = input
                .get("tool_input")
                .map(|v| serde_json::to_string(v).unwrap_or_default().len());
            evt.output_bytes = input
                .get("tool_output")
                .and_then(|v| v.as_str())
                .map(|s| s.len());
            evt.duration_ms = input.get("duration").and_then(|v| v.as_u64());
        }
        "afterMCPExecution" => {
            evt.tool_name = input.get("tool_name").and_then(|v| v.as_str()).map(String::from);
            evt.input_bytes = input
                .get("tool_input")
                .and_then(|v| v.as_str())
                .map(|s| s.len());
            evt.output_bytes = input
                .get("result_json")
                .and_then(|v| v.as_str())
                .map(|s| s.len());
            evt.duration_ms = input.get("duration").and_then(|v| v.as_u64());
        }
        // Claude Code PascalCase events
        "PreToolUse" => {
            evt.tool_name = input.get("tool_name").and_then(|v| v.as_str()).map(String::from);
            evt.input_bytes = input
                .get("tool_input")
                .map(|v| serde_json::to_string(v).unwrap_or_default().len());
        }
        "PostToolUse" | "PostToolUseFailure" => {
            evt.tool_name = input.get("tool_name").and_then(|v| v.as_str()).map(String::from);
            evt.input_bytes = input
                .get("tool_input")
                .map(|v| serde_json::to_string(v).unwrap_or_default().len());
            evt.output_bytes = input
                .get("tool_response")
                .map(|v| serde_json::to_string(v).unwrap_or_default().len());
            if let Some(path) = input.get("tool_input")
                .and_then(|v| v.get("file_path"))
                .and_then(|v| v.as_str())
            {
                evt.file_path = Some(path.to_string());
            }
        }
        "SessionStart" => {
            evt.status = input.get("source").and_then(|v| v.as_str()).map(String::from);
            if evt.model.is_empty() {
                evt.model = input.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
            }
        }
        "Stop" => {
            evt.status = input.get("stop_hook_active").map(|_| "stop".to_string());
        }
        "PreCompact" | "preCompact" => {
            evt.context_tokens = input.get("context_tokens").and_then(|v| v.as_u64());
            evt.context_window_size = input.get("context_window_size").and_then(|v| v.as_u64());
            evt.context_usage_pct = input.get("context_usage_percent").and_then(|v| v.as_f64());
        }
        "SessionEnd" | "sessionEnd" => {
            evt.status = input.get("status").or_else(|| input.get("reason"))
                .and_then(|v| v.as_str()).map(String::from);
            evt.loop_count = input.get("loop_count").and_then(|v| v.as_u64()).map(|v| v as u32);
            evt.duration_ms = input.get("duration_ms").and_then(|v| v.as_u64());
        }
        "stop" => {
            evt.status = input.get("status").or_else(|| input.get("reason"))
                .and_then(|v| v.as_str()).map(String::from);
            evt.loop_count = input.get("loop_count").and_then(|v| v.as_u64()).map(|v| v as u32);
            evt.duration_ms = input.get("duration_ms").and_then(|v| v.as_u64());
        }
        "afterFileEdit" => {
            evt.file_path = input.get("file_path").and_then(|v| v.as_str()).map(String::from);
        }
        _ => {}
    }

    let metrics_dir = scavenger_dir.join("metrics");
    if let Err(e) = std::fs::create_dir_all(&metrics_dir) {
        eprintln!("scavenger audit: failed to create metrics dir {}: {e}", metrics_dir.display());
        super::print_json(&json!({}));
        return Ok(());
    }

    let log_path = metrics_dir.join(format!("{conversation_id}.jsonl"));
    let line = serde_json::to_string(&evt)? + "\n";

    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(mut file) => {
            if let Err(e) = file.write_all(line.as_bytes()) {
                eprintln!("scavenger audit: write error: {e}");
            }
        }
        Err(e) => {
            eprintln!("scavenger audit: failed to open {}: {e}", log_path.display());
        }
    }

    super::print_json(&json!({}));
    Ok(())
}

// ── Session labeling ────────────────────────────────────────────────

/// Save a user label for a session (e.g. "with-scavenger", "baseline").
pub fn tag_session(
    scavenger_dir: &Path,
    conversation_id: &str,
    label: &str,
) -> Result<(), std::io::Error> {
    let metrics_dir = scavenger_dir.join("metrics");
    std::fs::create_dir_all(&metrics_dir)?;
    let label_path = metrics_dir.join(format!("{conversation_id}.label"));
    std::fs::write(label_path, label)
}

/// Read the user label for a session, if any.
pub fn read_label(scavenger_dir: &Path, conversation_id: &str) -> Option<String> {
    let label_path = scavenger_dir
        .join("metrics")
        .join(format!("{conversation_id}.label"));
    std::fs::read_to_string(label_path).ok().map(|s| s.trim().to_string())
}

// ── Metrics analysis ────────────────────────────────────────────────

/// Summary of a single session's metrics.
#[derive(Debug, Default)]
pub struct SessionSummary {
    pub conversation_id: String,
    pub label: Option<String>,
    pub model: String,
    pub scavenger_active: bool,
    pub total_tool_calls: u32,
    pub tool_breakdown: std::collections::HashMap<String, u32>,
    pub mcp_calls: u32,
    pub mcp_breakdown: std::collections::HashMap<String, u32>,
    pub file_reads: u32,
    pub file_edits: u32,
    pub grep_calls: u32,
    pub search_calls: u32,
    pub total_input_bytes: usize,
    pub total_output_bytes: usize,
    pub total_duration_ms: u64,
    pub context_tokens_peak: Option<u64>,
    pub context_window_size: Option<u64>,
    pub compaction_count: u32,
    pub status: Option<String>,
    pub session_duration_ms: Option<u64>,
    pub first_event: Option<String>,
    pub last_event: Option<String>,
    pub unique_files_read: Vec<String>,
}

impl SessionSummary {
    pub fn estimated_input_tokens(&self) -> usize {
        self.total_input_bytes / 4
    }

    pub fn estimated_output_tokens(&self) -> usize {
        self.total_output_bytes / 4
    }

    /// "with-scavenger" or "without-scavenger" based on label or auto-detection.
    pub fn condition_label(&self) -> &str {
        if let Some(ref label) = self.label {
            return label;
        }
        if self.scavenger_active {
            "WITH scavenger"
        } else {
            "WITHOUT scavenger"
        }
    }

    pub fn capsule_calls(&self) -> u32 {
        self.mcp_breakdown.get("get_capsule").copied().unwrap_or(0)
    }

    /// Total navigation calls: file reads + greps + semantic searches.
    /// This is what Scavenger capsules are meant to replace/reduce.
    pub fn navigation_calls(&self) -> u32 {
        self.file_reads + self.grep_calls + self.search_calls
    }
}

/// Load and analyze a session's metrics log.
pub fn analyze_session(scavenger_dir: &Path, conversation_id: &str) -> Option<SessionSummary> {
    let log_path = scavenger_dir
        .join("metrics")
        .join(format!("{conversation_id}.jsonl"));

    if !log_path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&log_path).ok()?;
    let events: Vec<MetricsEvent> = content
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    if events.is_empty() {
        return None;
    }

    let label = read_label(scavenger_dir, conversation_id);

    let mut summary = SessionSummary {
        conversation_id: conversation_id.to_string(),
        label,
        model: events.first().map(|e| e.model.clone()).unwrap_or_default(),
        first_event: events.first().map(|e| e.timestamp.clone()),
        last_event: events.last().map(|e| e.timestamp.clone()),
        ..Default::default()
    };

    let mut files_seen = std::collections::HashSet::new();

    for evt in &events {
        match evt.event.as_str() {
            "postToolUse" | "PostToolUse" => {
                summary.total_tool_calls += 1;
                if let Some(name) = &evt.tool_name {
                    *summary.tool_breakdown.entry(name.clone()).or_insert(0) += 1;
                    match name.as_str() {
                        "Read" => {
                            summary.file_reads += 1;
                            if let Some(ref path) = evt.file_path {
                                files_seen.insert(path.clone());
                            }
                        }
                        "Write" | "Edit" | "MultiEdit" | "StrReplace" => {
                            summary.file_edits += 1;
                        }
                        "Grep" => summary.grep_calls += 1,
                        "SemanticSearch" => summary.search_calls += 1,
                        _ => {}
                    }
                }
                summary.total_input_bytes += evt.input_bytes.unwrap_or(0);
                summary.total_output_bytes += evt.output_bytes.unwrap_or(0);
                summary.total_duration_ms += evt.duration_ms.unwrap_or(0);
                if evt.event == "PostToolUse" {
                    summary.scavenger_active = true;
                }
            }
            "PreToolUse" => {
                summary.scavenger_active = true;
            }
            "afterMCPExecution" => {
                summary.mcp_calls += 1;
                if let Some(name) = &evt.tool_name {
                    *summary.mcp_breakdown.entry(name.clone()).or_insert(0) += 1;
                    if name == "get_capsule" {
                        summary.scavenger_active = true;
                    }
                }
                summary.total_input_bytes += evt.input_bytes.unwrap_or(0);
                summary.total_output_bytes += evt.output_bytes.unwrap_or(0);
                summary.total_duration_ms += evt.duration_ms.unwrap_or(0);
            }
            "preCompact" => {
                summary.compaction_count += 1;
                if let Some(tokens) = evt.context_tokens {
                    summary.context_tokens_peak = Some(
                        summary.context_tokens_peak.map_or(tokens, |prev: u64| prev.max(tokens)),
                    );
                }
                if summary.context_window_size.is_none() {
                    summary.context_window_size = evt.context_window_size;
                }
            }
            "afterFileEdit" => {
                summary.file_edits += 1;
                if let Some(ref path) = evt.file_path {
                    files_seen.insert(path.clone());
                }
            }
            "stop" | "sessionEnd" => {
                if summary.status.is_none() {
                    summary.status = evt.status.clone();
                }
                if let Some(d) = evt.duration_ms {
                    summary.session_duration_ms = Some(d);
                }
            }
            _ => {}
        }
    }

    summary.unique_files_read = files_seen.into_iter().collect();
    summary.unique_files_read.sort();

    Some(summary)
}

/// List all session IDs that have metrics logs.
pub fn list_sessions(scavenger_dir: &Path) -> Vec<String> {
    let metrics_dir = scavenger_dir.join("metrics");
    if !metrics_dir.exists() {
        return Vec::new();
    }

    let mut sessions: Vec<(String, std::time::SystemTime)> = std::fs::read_dir(&metrics_dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".jsonl") {
                let id = name.trim_end_matches(".jsonl").to_string();
                let modified = entry.metadata().ok()?.modified().ok()?;
                Some((id, modified))
            } else {
                None
            }
        })
        .collect();

    sessions.sort_by(|a, b| b.1.cmp(&a.1));
    sessions.into_iter().map(|(id, _)| id).collect()
}

// ── Formatting ──────────────────────────────────────────────────────

/// Format the session list with auto-detected scavenger status.
pub fn format_list(scavenger_dir: &Path, sessions: &[String]) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "{:<10} {:<38} {:>6} {:>6} {:>6} {:>6} {:>10}\n",
        "Condition", "Session", "Tools", "Caps.", "Reads", "Greps", "Est.OutTok"
    ));
    out.push_str(&"─".repeat(86));
    out.push('\n');

    for id in sessions {
        if let Some(s) = analyze_session(scavenger_dir, id) {
            let condition = if s.scavenger_active { "[WITH]" } else { "[WITHOUT]" };
            out.push_str(&format!(
                "{:<10} {:<38} {:>6} {:>6} {:>6} {:>6} {:>10}\n",
                condition,
                id,
                s.total_tool_calls,
                s.capsule_calls(),
                s.file_reads,
                s.grep_calls,
                s.estimated_output_tokens(),
            ));
        }
    }

    out
}

/// Format a comparison of two sessions, framed around the Scavenger value proposition.
pub fn format_comparison(a: &SessionSummary, b: &SessionSummary) -> String {
    let mut out = String::new();

    let label_a = a.condition_label();
    let label_b = b.condition_label();

    out.push_str(&format!(
        "{:<34} {:>18} {:>18}\n",
        "", label_a, label_b
    ));
    let short_a = &a.conversation_id[..8.min(a.conversation_id.len())];
    let short_b = &b.conversation_id[..8.min(b.conversation_id.len())];
    out.push_str(&format!(
        "{:<34} {:>18} {:>18}\n",
        "Metric", short_a, short_b
    ));
    out.push_str(&"─".repeat(72));
    out.push('\n');

    out.push_str(&format!(
        "{:<34} {:>18} {:>18}\n",
        "Model", a.model, b.model
    ));
    out.push('\n');

    out.push_str("  NAVIGATION (how the agent explores)\n");
    row(&mut out, "  File reads (Read tool)", a.file_reads, b.file_reads);
    row(&mut out, "  Grep calls", a.grep_calls, b.grep_calls);
    row(&mut out, "  Semantic searches", a.search_calls, b.search_calls);
    row(
        &mut out,
        "  Total navigation calls",
        a.navigation_calls(),
        b.navigation_calls(),
    );
    row(&mut out, "  Capsule calls (get_capsule)", a.capsule_calls(), b.capsule_calls());
    out.push('\n');

    out.push_str("  WORK (what the agent produces)\n");
    row(&mut out, "  Total tool calls", a.total_tool_calls, b.total_tool_calls);
    row(&mut out, "  File edits", a.file_edits, b.file_edits);
    row(&mut out, "  MCP calls (all)", a.mcp_calls, b.mcp_calls);
    out.push('\n');

    out.push_str("  TOKEN USAGE (estimated from tool I/O)\n");
    row_usize(
        &mut out,
        "  Tool input tokens",
        a.estimated_input_tokens(),
        b.estimated_input_tokens(),
    );
    row_usize(
        &mut out,
        "  Tool output tokens",
        a.estimated_output_tokens(),
        b.estimated_output_tokens(),
    );
    row_usize(
        &mut out,
        "  Total tool bytes",
        a.total_input_bytes + a.total_output_bytes,
        b.total_input_bytes + b.total_output_bytes,
    );
    out.push('\n');

    out.push_str("  CONTEXT WINDOW\n");
    row(&mut out, "  Compactions", a.compaction_count, b.compaction_count);
    if a.context_tokens_peak.is_some() || b.context_tokens_peak.is_some() {
        row_u64(
            &mut out,
            "  Peak context tokens",
            a.context_tokens_peak.unwrap_or(0),
            b.context_tokens_peak.unwrap_or(0),
        );
    }
    out.push('\n');

    out.push_str("  TIMING\n");
    row_u64(&mut out, "  Tool execution (ms)", a.total_duration_ms, b.total_duration_ms);
    if a.session_duration_ms.is_some() || b.session_duration_ms.is_some() {
        row_u64(
            &mut out,
            "  Session duration (ms)",
            a.session_duration_ms.unwrap_or(0),
            b.session_duration_ms.unwrap_or(0),
        );
    }

    // Tool breakdown detail
    let mut all_tools: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    all_tools.extend(a.tool_breakdown.keys().cloned());
    all_tools.extend(b.tool_breakdown.keys().cloned());

    if !all_tools.is_empty() {
        out.push_str("\n  TOOL BREAKDOWN\n");
        for tool in &all_tools {
            let va = a.tool_breakdown.get(tool).copied().unwrap_or(0);
            let vb = b.tool_breakdown.get(tool).copied().unwrap_or(0);
            row(&mut out, &format!("  {tool}"), va, vb);
        }
    }

    let mut all_mcp: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    all_mcp.extend(a.mcp_breakdown.keys().cloned());
    all_mcp.extend(b.mcp_breakdown.keys().cloned());

    if !all_mcp.is_empty() {
        out.push_str("\n  MCP TOOL BREAKDOWN\n");
        for tool in &all_mcp {
            let va = a.mcp_breakdown.get(tool).copied().unwrap_or(0);
            let vb = b.mcp_breakdown.get(tool).copied().unwrap_or(0);
            row(&mut out, &format!("  {tool}"), va, vb);
        }
    }

    out
}

fn delta_str(a: f64, b: f64) -> String {
    if a == 0.0 && b == 0.0 {
        return String::new();
    }
    if a == 0.0 {
        return " (new)".to_string();
    }
    let pct = ((b - a) / a) * 100.0;
    if pct.abs() < 0.5 {
        String::new()
    } else {
        format!(" ({:+.0}%)", pct)
    }
}

fn row(out: &mut String, label: &str, a: u32, b: u32) {
    let d = delta_str(a as f64, b as f64);
    out.push_str(&format!(
        "{:<34} {:>18} {:>18}{}\n",
        label, a, b, d
    ));
}

fn row_usize(out: &mut String, label: &str, a: usize, b: usize) {
    let d = delta_str(a as f64, b as f64);
    out.push_str(&format!(
        "{:<34} {:>18} {:>18}{}\n",
        label, a, b, d
    ));
}

fn row_u64(out: &mut String, label: &str, a: u64, b: u64) {
    let d = delta_str(a as f64, b as f64);
    out.push_str(&format!(
        "{:<34} {:>18} {:>18}{}\n",
        label, a, b, d
    ));
}

/// Format a single session summary for display.
pub fn format_summary(s: &SessionSummary) -> String {
    let mut out = String::new();

    out.push_str(&format!("Session:   {}\n", s.conversation_id));
    out.push_str(&format!("Condition: {}\n", s.condition_label()));
    out.push_str(&format!("Model:     {}\n", s.model));
    if let Some(ref ts) = s.first_event {
        out.push_str(&format!("Started:   {}\n", ts));
    }
    if let Some(ref status) = s.status {
        out.push_str(&format!("Status:    {}\n", status));
    }
    out.push('\n');

    out.push_str("Navigation:\n");
    out.push_str(&format!("  File reads:         {}\n", s.file_reads));
    out.push_str(&format!("  Grep calls:         {}\n", s.grep_calls));
    out.push_str(&format!("  Semantic searches:  {}\n", s.search_calls));
    out.push_str(&format!("  Capsule calls:      {}\n", s.capsule_calls()));
    out.push_str(&format!(
        "  Total navigation:   {}\n",
        s.navigation_calls()
    ));

    out.push_str("\nWork:\n");
    out.push_str(&format!("  Total tool calls:   {}\n", s.total_tool_calls));
    out.push_str(&format!("  File edits:         {}\n", s.file_edits));
    out.push_str(&format!("  MCP calls:          {}\n", s.mcp_calls));

    let mut mcp_keys: Vec<_> = s.mcp_breakdown.keys().collect();
    mcp_keys.sort();
    for key in mcp_keys {
        out.push_str(&format!("    {:<18}  {}\n", key, s.mcp_breakdown[key]));
    }

    out.push_str("\nTokens (estimated from tool I/O):\n");
    out.push_str(&format!(
        "  Input tokens:       {}\n",
        s.estimated_input_tokens()
    ));
    out.push_str(&format!(
        "  Output tokens:      {}\n",
        s.estimated_output_tokens()
    ));
    out.push_str(&format!("  Tool time:          {} ms\n", s.total_duration_ms));

    if s.compaction_count > 0 {
        out.push_str(&format!("\nContext window:\n"));
        out.push_str(&format!("  Compactions:        {}\n", s.compaction_count));
        if let Some(peak) = s.context_tokens_peak {
            out.push_str(&format!("  Peak context:       {} tokens\n", peak));
        }
    }

    let mut tool_keys: Vec<_> = s.tool_breakdown.keys().collect();
    tool_keys.sort();
    if !tool_keys.is_empty() {
        out.push_str("\nTool breakdown:\n");
        for key in tool_keys {
            out.push_str(&format!("  {:<20}{}\n", key, s.tool_breakdown[key]));
        }
    }

    out
}
