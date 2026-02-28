use std::path::Path;

use fs2::FileExt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum HookRegistrationError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Register scavenger hooks and MCP bridge in `.claude/settings.local.json`.
/// Uses fs2 exclusive lock + temp file + rename for atomic updates.
pub fn register_hooks(project_root: &Path) -> Result<(), HookRegistrationError> {
    let claude_dir = project_root.join(".claude");
    std::fs::create_dir_all(&claude_dir)?;

    let settings_path = claude_dir.join("settings.local.json");
    let lock_path = claude_dir.join("settings.local.json.lock");

    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    lock_file.lock_exclusive()?;

    let _guard = LockGuard(&lock_file);

    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    deep_merge_hooks(&mut settings);

    let tmp_path = claude_dir.join("settings.local.json.tmp");
    let serialized = serde_json::to_string_pretty(&settings)?;
    std::fs::write(&tmp_path, &serialized)?;
    std::fs::rename(&tmp_path, &settings_path)?;

    Ok(())
}

fn deep_merge_hooks(settings: &mut serde_json::Value) {
    let obj = settings.as_object_mut().unwrap();

    // Hooks configuration
    if !obj.contains_key("hooks") {
        obj.insert("hooks".to_string(), serde_json::json!({}));
    }
    let hooks = obj.get_mut("hooks").unwrap().as_object_mut().unwrap();

    if !hooks.contains_key("PreToolUse") {
        hooks.insert("PreToolUse".to_string(), serde_json::json!([]));
    }
    let pre = hooks.get_mut("PreToolUse").unwrap().as_array_mut().unwrap();
    let pre_hook = serde_json::json!({
        "matcher": "Read",
        "hook": "scavenger hook pre-tool-use"
    });
    if !pre.iter().any(|h| h.get("hook").and_then(|v| v.as_str()) == Some("scavenger hook pre-tool-use")) {
        pre.push(pre_hook);
    }

    if !hooks.contains_key("PostToolUse") {
        hooks.insert("PostToolUse".to_string(), serde_json::json!([]));
    }
    let post = hooks.get_mut("PostToolUse").unwrap().as_array_mut().unwrap();
    let post_hook = serde_json::json!({
        "matcher": "Write|Edit|MultiEdit",
        "hook": "scavenger hook post-tool-use"
    });
    if !post.iter().any(|h| h.get("hook").and_then(|v| v.as_str()) == Some("scavenger hook post-tool-use")) {
        post.push(post_hook);
    }

    // MCP configuration
    if !obj.contains_key("mcpServers") {
        obj.insert("mcpServers".to_string(), serde_json::json!({}));
    }
    let mcp = obj.get_mut("mcpServers").unwrap().as_object_mut().unwrap();
    mcp.insert(
        "scavenger".to_string(),
        serde_json::json!({
            "command": "scavenger",
            "args": ["daemon", "--mcp-bridge"]
        }),
    );
}

struct LockGuard<'a>(&'a std::fs::File);

impl Drop for LockGuard<'_> {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_hooks_creates_settings() {
        let tmp = tempfile::tempdir().unwrap();
        register_hooks(tmp.path()).unwrap();

        let settings_path = tmp.path().join(".claude/settings.local.json");
        assert!(settings_path.exists());

        let content = std::fs::read_to_string(&settings_path).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert!(val["hooks"]["PreToolUse"].as_array().unwrap().len() >= 1);
        assert!(val["hooks"]["PostToolUse"].as_array().unwrap().len() >= 1);
        assert!(val["mcpServers"]["scavenger"].is_object());
    }

    #[test]
    fn test_register_hooks_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        register_hooks(tmp.path()).unwrap();
        register_hooks(tmp.path()).unwrap();

        let content = std::fs::read_to_string(tmp.path().join(".claude/settings.local.json")).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(val["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
        assert_eq!(val["hooks"]["PostToolUse"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_register_hooks_preserves_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let claude_dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("settings.local.json"),
            r#"{"customKey": "customValue"}"#,
        ).unwrap();

        register_hooks(tmp.path()).unwrap();

        let content = std::fs::read_to_string(claude_dir.join("settings.local.json")).unwrap();
        let val: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(val["customKey"].as_str(), Some("customValue"));
        assert!(val["hooks"]["PreToolUse"].as_array().unwrap().len() >= 1);
    }
}
