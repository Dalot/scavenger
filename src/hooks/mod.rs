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

    match tokio::time::timeout(
        timeout,
        crate::bridge::get_capsule(&socket_path, file, None, None),
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
            // Timeout or error: fail open
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

fn read_stdin_json() -> Result<Value, Box<dyn std::error::Error>> {
    let mut buf = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
    let val: Value = serde_json::from_str(&buf).unwrap_or_else(|_| json!({}));
    Ok(val)
}

fn print_json(value: &Value) {
    if let Ok(s) = serde_json::to_string(value) {
        println!("{s}");
    }
}
