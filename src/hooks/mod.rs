pub mod metrics;
pub mod register;

use std::path::Path;

use serde_json::{json, Value};

/// Handle PreToolUse hook invocation.
/// Reads stdin JSON, connects to daemon UDS, requests capsule, writes stdout.
/// Exit 0 always (fail open). Partial fallback at 100ms.
pub async fn handle_pre_tool_use(
    scavenger_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = read_stdin_json()?;

    let tool_name = input
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if tool_name != "Read" {
        print_json(&json!({}));
        return Ok(());
    }

    let file = input
        .get("tool_input")
        .and_then(|v| v.get("file_path").or_else(|| v.get("path")))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if file.is_empty() {
        print_json(&json!({}));
        return Ok(());
    }

    let socket_path = scavenger_dir.join("daemon.sock");
    if !socket_path.exists() {
        print_json(&json!({}));
        return Ok(());
    }

    let timeout = tokio::time::Duration::from_millis(100);

    let capsule_req = json!({
        "method": "capsule",
        "file": file,
    });

    match tokio::time::timeout(
        timeout,
        crate::daemon::socket::send_request(&socket_path, &capsule_req),
    )
    .await
    {
        Ok(Ok(response)) => {
            let capsule_text = response
                .get("capsule")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if capsule_text.is_empty() {
                print_json(&json!({}));
            } else {
                print_json(&json!({ "additionalContext": capsule_text }));
            }
        }
        _ => {
            print_json(&json!({}));
        }
    }

    Ok(())
}

/// Handle PostToolUse hook invocation.
/// Reads stdin JSON, enqueues re-index for Write/Edit/MultiEdit.
/// Exit 0 always.
pub async fn handle_post_tool_use(
    scavenger_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = read_stdin_json()?;

    let tool_name = input
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match tool_name {
        "Write" | "Edit" | "MultiEdit" => {}
        _ => {
            print_json(&json!({}));
            return Ok(());
        }
    }

    let file = input
        .get("tool_input")
        .and_then(|v| v.get("file_path").or_else(|| v.get("path")))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let socket_path = scavenger_dir.join("daemon.sock");
    if !socket_path.exists() || file.is_empty() {
        print_json(&json!({}));
        return Ok(());
    }

    let request = json!({
        "method": "hook_post",
        "file": file,
    });

    let timeout = tokio::time::Duration::from_millis(100);
    let _ = tokio::time::timeout(
        timeout,
        crate::daemon::socket::send_request(&socket_path, &request),
    )
    .await;

    print_json(&json!({}));
    Ok(())
}

/// Handle Cursor's `afterFileEdit` hook.
/// Reads `{ file_path, edits }` from stdin, triggers re-index on the daemon.
/// Exit 0 always (fail open).
pub async fn handle_after_file_edit(
    scavenger_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let input = read_stdin_json()?;

    let file = input
        .get("file_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let socket_path = scavenger_dir.join("daemon.sock");
    if !socket_path.exists() || file.is_empty() {
        print_json(&json!({}));
        return Ok(());
    }

    let request = json!({
        "method": "hook_post",
        "file": file,
    });

    let timeout = tokio::time::Duration::from_millis(100);
    let _ = tokio::time::timeout(
        timeout,
        crate::daemon::socket::send_request(&socket_path, &request),
    )
    .await;

    print_json(&json!({}));
    Ok(())
}

/// Handle SessionStart hook. Returns `additional_context` with a decision tree
/// that positions Scavenger tools as replacements for native navigation.
pub fn handle_session_start_with_context() {
    print_json(&json!({
        "additional_context": concat!(
            "## Scavenger — AST dependency graph active\n\n",
            "This project has a pre-built dependency graph of all source files. ",
            "Use it INSTEAD OF native tools when possible:\n\n",
            "1. \"What calls X / what does X call / what breaks if X changes?\" → ",
            "`get_capsule(symbol=\"X\")`. DO NOT grep for usages — the graph already has this.\n",
            "2. \"Understand a file\" → `get_capsule(file=\"path\")` returns focused context ",
            "(signatures, dependencies) within a token budget. Only Read if you need exact ",
            "line content the capsule didn't include.\n",
            "3. \"Find code related to a concept\" → `search_docs(query=\"...\")` searches ",
            "indexed docs and code semantically.\n",
            "4. \"Resume prior session\" → `read_annotations(session_summary=true)`.\n\n",
            "Use native Grep/Glob/Read only for: exact string matches in non-code files, ",
            "file listing, or raw content after a capsule confirmed relevance.\n\n",
            "Trust the graph — it is complete for all source files in this project."
        )
    }));
}

fn read_stdin_json() -> Result<Value, Box<dyn std::error::Error>> {
    let mut buf = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
    if buf.trim().is_empty() {
        eprintln!("scavenger: hook received empty stdin");
        return Ok(json!({}));
    }
    match serde_json::from_str(&buf) {
        Ok(val) => Ok(val),
        Err(e) => {
            eprintln!(
                "scavenger: hook stdin JSON parse error: {e} (first 200 chars: {:?})",
                &buf[..buf.len().min(200)]
            );
            Ok(json!({}))
        }
    }
}

fn print_json(value: &Value) {
    if let Ok(s) = serde_json::to_string(value) {
        println!("{s}");
    }
}
