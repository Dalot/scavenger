use std::path::Path;

use fs2::FileExt;
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PluginError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

const SCAVENGER_PRE_CMD: &str = "scavenger hook pre-tool-use";
const SCAVENGER_POST_CMD: &str = "scavenger hook post-tool-use";
const SCAVENGER_AUDIT_CMD: &str = "scavenger hook audit";

// ── Cursor IDE integration ──────────────────────────────────────────

/// Create `.cursor/mcp.json` so Cursor discovers the MCP bridge.
pub fn create_cursor_mcp_config(project_root: &Path) -> Result<(), PluginError> {
    let cursor_dir = project_root.join(".cursor");
    std::fs::create_dir_all(&cursor_dir)?;

    let mcp_path = cursor_dir.join("mcp.json");

    let mut config: serde_json::Value = if mcp_path.exists() {
        let content = std::fs::read_to_string(&mcp_path)?;
        if content.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&content).unwrap_or(json!({}))
        }
    } else {
        json!({})
    };

    let mcp_servers = config
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| json!({}));

    mcp_servers["scavenger"] = json!({
        "command": "scavenger",
        "args": ["mcp-bridge"],
        "cwd": project_root.to_string_lossy(),
    });

    std::fs::write(&mcp_path, serde_json::to_string_pretty(&config)?)?;
    Ok(())
}

/// Create `.cursor/hooks.json` with native Cursor hooks for daemon
/// lifecycle and re-indexing on edits.
pub fn create_cursor_hooks(project_root: &Path) -> Result<(), PluginError> {
    let cursor_dir = project_root.join(".cursor");
    std::fs::create_dir_all(&cursor_dir)?;

    let hooks_path = cursor_dir.join("hooks.json");

    let mut config: serde_json::Value = if hooks_path.exists() {
        let content = std::fs::read_to_string(&hooks_path)?;
        if content.trim().is_empty() {
            json!({ "version": 1, "hooks": {} })
        } else {
            serde_json::from_str(&content).unwrap_or(json!({ "version": 1, "hooks": {} }))
        }
    } else {
        json!({ "version": 1, "hooks": {} })
    };

    config["version"] = json!(1);

    let hooks = config
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| json!({}));
    let hooks = hooks.as_object_mut().unwrap();

    let audit_cmd = json!({ "command": "scavenger hook audit" });

    hooks.insert(
        "sessionStart".into(),
        json!([
            { "command": "scavenger hook session-start" },
            audit_cmd,
        ]),
    );
    hooks.insert(
        "sessionEnd".into(),
        json!([
            { "command": "scavenger hook session-end" },
            audit_cmd,
        ]),
    );
    hooks.insert(
        "afterFileEdit".into(),
        json!([
            { "command": "scavenger hook after-file-edit" },
            audit_cmd,
        ]),
    );
    hooks.insert("postToolUse".into(), json!([audit_cmd]));
    hooks.insert("afterMCPExecution".into(), json!([audit_cmd]));
    hooks.insert("preCompact".into(), json!([audit_cmd]));
    hooks.insert("stop".into(), json!([audit_cmd]));

    std::fs::write(&hooks_path, serde_json::to_string_pretty(&config)?)?;
    Ok(())
}

/// Remove Scavenger entries from `.cursor/hooks.json` and `.cursor/mcp.json`.
pub fn remove_cursor_config(project_root: &Path) -> Result<(), PluginError> {
    let cursor_dir = project_root.join(".cursor");

    let mcp_path = cursor_dir.join("mcp.json");
    if mcp_path.exists() {
        let content = std::fs::read_to_string(&mcp_path)?;
        if let Ok(mut config) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(servers) = config.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
                servers.remove("scavenger");
                if servers.is_empty() {
                    config.as_object_mut().unwrap().remove("mcpServers");
                }
            }
            if config.as_object().is_some_and(|o| o.is_empty()) {
                let _ = std::fs::remove_file(&mcp_path);
            } else {
                std::fs::write(&mcp_path, serde_json::to_string_pretty(&config)?)?;
            }
        }
    }

    let hooks_path = cursor_dir.join("hooks.json");
    if hooks_path.exists() {
        let content = std::fs::read_to_string(&hooks_path)?;
        if let Ok(mut config) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(hooks) = config.get_mut("hooks").and_then(|v| v.as_object_mut()) {
                for event in &[
                    "sessionStart",
                    "sessionEnd",
                    "afterFileEdit",
                    "postToolUse",
                    "afterMCPExecution",
                    "preCompact",
                    "stop",
                ] {
                    remove_scavenger_cursor_hook(hooks, event);
                }
                if hooks.is_empty() {
                    config.as_object_mut().unwrap().remove("hooks");
                }
            }
            let is_empty = config
                .as_object()
                .is_some_and(|o| o.is_empty() || (o.len() == 1 && o.contains_key("version")));
            if is_empty {
                let _ = std::fs::remove_file(&hooks_path);
            } else {
                std::fs::write(&hooks_path, serde_json::to_string_pretty(&config)?)?;
            }
        }
    }

    Ok(())
}

fn remove_scavenger_cursor_hook(
    hooks: &mut serde_json::Map<String, serde_json::Value>,
    event: &str,
) {
    if let Some(arr_val) = hooks.get_mut(event) {
        if let Some(arr) = arr_val.as_array_mut() {
            arr.retain(|entry| {
                let cmd = entry.get("command").and_then(|v| v.as_str()).unwrap_or("");
                !cmd.starts_with("scavenger ")
            });
            if arr.is_empty() {
                hooks.remove(event);
            }
        }
    }
}

/// Path to the Claude Code plugin directory, relative to the project root.
pub fn plugin_dir(project_root: &Path) -> std::path::PathBuf {
    project_root.join(".scavenger").join("claude-plugin")
}

/// Create (or overwrite) the self-contained Claude Code plugin at
/// `.scavenger/claude-plugin/`. The plugin owns its own hooks and MCP
/// config so we never touch the user's settings files.
pub fn create_plugin(project_root: &Path) -> Result<(), PluginError> {
    let root = plugin_dir(project_root);

    std::fs::create_dir_all(root.join(".claude-plugin"))?;
    std::fs::create_dir_all(root.join("hooks"))?;

    std::fs::write(
        root.join(".claude-plugin").join("plugin.json"),
        serde_json::to_string_pretty(&json!({
            "name": "scavenger",
            "description": "AST dependency graph and session memory engine -- serves focused capsules instead of full files",
            "version": "0.2.0"
        }))?,
    )?;

    let audit = json!({ "type": "command", "command": SCAVENGER_AUDIT_CMD });

    std::fs::write(
        root.join("hooks").join("hooks.json"),
        serde_json::to_string_pretty(&json!({
            "description": "Scavenger hooks for daemon lifecycle, capsule serving, metrics, and graph updates",
            "hooks": {
                "SessionStart": [{
                    "hooks": [
                        { "type": "command", "command": "scavenger hook session-start" },
                        audit,
                    ]
                }],
                "SessionEnd": [{
                    "hooks": [
                        { "type": "command", "command": "scavenger hook session-end" },
                        audit,
                    ]
                }],
                "PreToolUse": [
                    {
                        "matcher": "Read",
                        "hooks": [{ "type": "command", "command": SCAVENGER_PRE_CMD }]
                    },
                    {
                        "hooks": [audit]
                    }
                ],
                "PostToolUse": [
                    {
                        "matcher": "Write|Edit|MultiEdit",
                        "hooks": [{ "type": "command", "command": SCAVENGER_POST_CMD }]
                    },
                    {
                        "hooks": [audit]
                    }
                ]
            }
        }))?,
    )?;

    Ok(())
}

// ── Claude Code CLI integration ─────────────────────────────────────

/// Try registering the MCP bridge via `claude mcp add`. Returns `Ok(true)` if
/// the CLI was found and registration succeeded, `Ok(false)` if `claude` is not
/// on PATH or the command failed. Never returns an error for missing CLI.
pub fn register_mcp_via_cli(project_root: &Path) -> Result<bool, PluginError> {
    let status = std::process::Command::new("claude")
        .args(["mcp", "add", "scavenger", "--", "scavenger", "mcp-bridge"])
        .current_dir(project_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => Ok(true),
        _ => Ok(false),
    }
}

/// Try removing the scavenger MCP entry via `claude mcp remove`.
pub fn remove_mcp_via_cli(project_root: &Path) -> Result<bool, PluginError> {
    let status = std::process::Command::new("claude")
        .args(["mcp", "remove", "scavenger"])
        .current_dir(project_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => Ok(true),
        _ => Ok(false),
    }
}

// ── .mcp.json (de-facto standard for MCP-compatible tools) ──────────

/// Register the MCP bridge in `.mcp.json` at the project root. Preserves
/// existing servers — only adds/overwrites the `scavenger` entry.
pub fn register_mcp_in_mcp_json(project_root: &Path) -> Result<(), PluginError> {
    let mcp_path = project_root.join(".mcp.json");

    let mut config: serde_json::Value = if mcp_path.exists() {
        let content = std::fs::read_to_string(&mcp_path)?;
        if content.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&content).unwrap_or(json!({}))
        }
    } else {
        json!({})
    };

    let mcp_servers = config
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| json!({}));

    mcp_servers["scavenger"] = json!({
        "command": "scavenger",
        "args": ["mcp-bridge"],
    });

    std::fs::write(&mcp_path, serde_json::to_string_pretty(&config)?)?;
    Ok(())
}

/// Remove the scavenger entry from `.mcp.json`. Deletes the file if it
/// becomes empty (or contains only an empty `mcpServers` object).
pub fn remove_mcp_from_mcp_json(project_root: &Path) -> Result<(), PluginError> {
    let mcp_path = project_root.join(".mcp.json");
    if !mcp_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&mcp_path)?;
    let mut config: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    if let Some(servers) = config.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
        servers.remove("scavenger");
        if servers.is_empty() {
            config.as_object_mut().unwrap().remove("mcpServers");
        }
    }

    if config.as_object().is_some_and(|o| o.is_empty()) {
        let _ = std::fs::remove_file(&mcp_path);
    } else {
        std::fs::write(&mcp_path, serde_json::to_string_pretty(&config)?)?;
    }

    Ok(())
}

// ── .claude/settings.local.json (fallback for future Claude Code fix) ─

/// Register the MCP bridge server in `.claude/settings.local.json` as a
/// fallback. Claude Code has a known bug where it doesn't read mcpServers
/// from this file, but we keep it for forward-compatibility.
pub fn register_mcp_server(project_root: &Path) -> Result<(), PluginError> {
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
        if content.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&content).unwrap_or(json!({}))
        }
    } else {
        json!({})
    };

    let mcp_servers = settings
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| json!({}));

    mcp_servers["scavenger"] = json!({
        "command": "scavenger",
        "args": ["mcp-bridge"],
        "cwd": project_root.to_string_lossy(),
    });

    let tmp_path = claude_dir.join("settings.local.json.tmp");
    let serialized = serde_json::to_string_pretty(&settings)?;
    std::fs::write(&tmp_path, &serialized)?;
    std::fs::rename(&tmp_path, &settings_path)?;

    Ok(())
}

/// Remove scavenger-owned entries that older versions wrote directly into
/// `.claude/settings.local.json`. If the file becomes an empty object after
/// cleanup, it is deleted along with its lock file.
pub fn remove_legacy_settings(project_root: &Path) -> Result<(), PluginError> {
    let claude_dir = project_root.join(".claude");
    let settings_path = claude_dir.join("settings.local.json");

    if !settings_path.exists() {
        return Ok(());
    }

    let lock_path = claude_dir.join("settings.local.json.lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    lock_file.lock_exclusive()?;
    let _guard = LockGuard(&lock_file);

    let content = std::fs::read_to_string(&settings_path)?;
    if content.trim().is_empty() {
        let _ = std::fs::remove_file(&settings_path);
        drop(_guard);
        let _ = std::fs::remove_file(&lock_path);
        return Ok(());
    }

    let mut settings: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    if !settings.is_object() {
        return Ok(());
    }

    let obj = settings.as_object_mut().unwrap();
    let mut changed = false;

    if let Some(hooks_val) = obj.get_mut("hooks") {
        if let Some(hooks) = hooks_val.as_object_mut() {
            for event in &["PreToolUse", "PostToolUse"] {
                if let Some(arr_val) = hooks.get_mut(*event) {
                    if let Some(arr) = arr_val.as_array_mut() {
                        let before = arr.len();
                        arr.retain(|entry| !is_scavenger_hook_entry(entry));
                        if arr.len() != before {
                            changed = true;
                        }
                        if arr.is_empty() {
                            hooks.remove(*event);
                            changed = true;
                        }
                    }
                }
            }
            if hooks.is_empty() {
                obj.remove("hooks");
                changed = true;
            }
        }
    }

    // NOTE: We intentionally keep mcpServers.scavenger — it's the active MCP
    // bridge registration, not a legacy entry.

    if !changed {
        return Ok(());
    }

    if obj.is_empty() {
        let _ = std::fs::remove_file(&settings_path);
        drop(_guard);
        let _ = std::fs::remove_file(&lock_path);
    } else {
        let tmp_path = claude_dir.join("settings.local.json.tmp");
        let serialized = serde_json::to_string_pretty(&settings)?;
        std::fs::write(&tmp_path, &serialized)?;
        std::fs::rename(&tmp_path, &settings_path)?;
    }

    Ok(())
}

fn is_scavenger_hook_entry(entry: &serde_json::Value) -> bool {
    if let Some(hook_str) = entry.get("hook").and_then(|v| v.as_str()) {
        if hook_str == SCAVENGER_PRE_CMD || hook_str == SCAVENGER_POST_CMD {
            return true;
        }
    }
    if let Some(hooks_arr) = entry.get("hooks").and_then(|v| v.as_array()) {
        return hooks_arr.iter().any(|h| {
            let cmd = h.get("command").and_then(|v| v.as_str()).unwrap_or("");
            cmd == SCAVENGER_PRE_CMD || cmd == SCAVENGER_POST_CMD
        });
    }
    false
}

struct LockGuard<'a>(&'a std::fs::File);

impl Drop for LockGuard<'_> {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_json(path: &Path) -> serde_json::Value {
        let content = std::fs::read_to_string(path).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    fn write_settings(dir: &Path, content: &str) {
        let claude_dir = dir.join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(claude_dir.join("settings.local.json"), content).unwrap();
    }

    // ---- create_plugin tests ----

    #[test]
    fn test_creates_plugin_directory_structure() {
        let tmp = tempfile::tempdir().unwrap();
        create_plugin(tmp.path()).unwrap();

        let root = plugin_dir(tmp.path());
        assert!(root.join(".claude-plugin/plugin.json").exists());
        assert!(root.join("hooks/hooks.json").exists());
    }

    #[test]
    fn test_plugin_manifest_content() {
        let tmp = tempfile::tempdir().unwrap();
        create_plugin(tmp.path()).unwrap();

        let manifest = read_json(&plugin_dir(tmp.path()).join(".claude-plugin/plugin.json"));
        assert_eq!(manifest["name"], "scavenger");
        assert!(manifest["description"].as_str().unwrap().contains("AST"));
        assert_eq!(manifest["version"], "0.1.1");
    }

    #[test]
    fn test_plugin_hooks_content() {
        let tmp = tempfile::tempdir().unwrap();
        create_plugin(tmp.path()).unwrap();

        let hooks = read_json(&plugin_dir(tmp.path()).join("hooks/hooks.json"));
        assert!(hooks["description"].is_string());

        let pre = &hooks["hooks"]["PreToolUse"];
        assert_eq!(pre[0]["matcher"], "Read");
        assert_eq!(pre[0]["hooks"][0]["command"], SCAVENGER_PRE_CMD);
        assert_eq!(pre[1]["hooks"][0]["command"], SCAVENGER_AUDIT_CMD);

        let post = &hooks["hooks"]["PostToolUse"];
        assert_eq!(post[0]["matcher"], "Write|Edit|MultiEdit");
        assert_eq!(post[0]["hooks"][0]["command"], SCAVENGER_POST_CMD);
        assert_eq!(post[1]["hooks"][0]["command"], SCAVENGER_AUDIT_CMD);

        let start = &hooks["hooks"]["SessionStart"];
        assert_eq!(start[0]["hooks"][1]["command"], SCAVENGER_AUDIT_CMD);
    }

    #[test]
    fn test_create_plugin_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        create_plugin(tmp.path()).unwrap();
        create_plugin(tmp.path()).unwrap();

        let hooks = read_json(&plugin_dir(tmp.path()).join("hooks/hooks.json"));
        // 2 entries: Read matcher + audit catch-all
        assert_eq!(hooks["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
    }

    // ---- remove_legacy_settings tests ----

    #[test]
    fn test_removes_old_format_hooks() {
        let tmp = tempfile::tempdir().unwrap();
        write_settings(
            tmp.path(),
            &serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [{ "matcher": "Read", "hook": SCAVENGER_PRE_CMD }],
                    "PostToolUse": [{ "matcher": "Write|Edit|MultiEdit", "hook": SCAVENGER_POST_CMD }]
                }
            }))
            .unwrap(),
        );

        remove_legacy_settings(tmp.path()).unwrap();

        let path = tmp.path().join(".claude/settings.local.json");
        assert!(!path.exists(), "empty settings file should be deleted");
    }

    #[test]
    fn test_removes_new_format_hooks() {
        let tmp = tempfile::tempdir().unwrap();
        write_settings(
            tmp.path(),
            &serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [{
                        "matcher": { "tools": ["Read"] },
                        "hooks": [{ "type": "command", "command": SCAVENGER_PRE_CMD }]
                    }],
                    "PostToolUse": [{
                        "matcher": { "tools": ["Write", "Edit", "MultiEdit"] },
                        "hooks": [{ "type": "command", "command": SCAVENGER_POST_CMD }]
                    }]
                }
            }))
            .unwrap(),
        );

        remove_legacy_settings(tmp.path()).unwrap();

        let path = tmp.path().join(".claude/settings.local.json");
        assert!(!path.exists(), "empty settings file should be deleted");
    }

    #[test]
    fn test_preserves_mcp_and_user_settings_during_cleanup() {
        let tmp = tempfile::tempdir().unwrap();
        write_settings(
            tmp.path(),
            &serde_json::to_string_pretty(&json!({
                "customKey": "customValue",
                "hooks": {
                    "PreToolUse": [
                        { "matcher": "BashTool", "hooks": [{ "type": "command", "command": "echo user" }] },
                        { "matcher": "Read", "hooks": [{ "type": "command", "command": SCAVENGER_PRE_CMD }] }
                    ]
                },
                "mcpServers": {
                    "other-server": { "command": "other" },
                    "scavenger": { "command": "scavenger", "args": ["mcp-bridge"] }
                }
            }))
            .unwrap(),
        );

        remove_legacy_settings(tmp.path()).unwrap();

        let val = read_json(&tmp.path().join(".claude/settings.local.json"));
        assert_eq!(val["customKey"], "customValue");

        let pre = val["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0]["hooks"][0]["command"], "echo user");

        // MCP servers are preserved (not legacy)
        assert_eq!(val["mcpServers"]["other-server"]["command"], "other");
        assert_eq!(val["mcpServers"]["scavenger"]["command"], "scavenger");
    }

    #[test]
    fn test_noop_when_no_settings_file() {
        let tmp = tempfile::tempdir().unwrap();
        remove_legacy_settings(tmp.path()).unwrap();
        assert!(!tmp.path().join(".claude/settings.local.json").exists());
    }

    #[test]
    fn test_noop_on_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        write_settings(tmp.path(), "{ not valid json");
        remove_legacy_settings(tmp.path()).unwrap();

        let content =
            std::fs::read_to_string(tmp.path().join(".claude/settings.local.json")).unwrap();
        assert_eq!(
            content, "{ not valid json",
            "invalid file must not be modified"
        );
    }

    #[test]
    fn test_removes_empty_settings_file() {
        let tmp = tempfile::tempdir().unwrap();
        write_settings(tmp.path(), "  ");
        remove_legacy_settings(tmp.path()).unwrap();
        assert!(!tmp.path().join(".claude/settings.local.json").exists());
    }

    // ---- Cursor config tests ----

    #[test]
    fn test_creates_cursor_mcp_config() {
        let tmp = tempfile::tempdir().unwrap();
        create_cursor_mcp_config(tmp.path()).unwrap();

        let config = read_json(&tmp.path().join(".cursor/mcp.json"));
        assert_eq!(config["mcpServers"]["scavenger"]["command"], "scavenger");
        assert_eq!(config["mcpServers"]["scavenger"]["args"][0], "mcp-bridge");
    }

    #[test]
    fn test_cursor_mcp_config_preserves_existing_servers() {
        let tmp = tempfile::tempdir().unwrap();
        let cursor_dir = tmp.path().join(".cursor");
        std::fs::create_dir_all(&cursor_dir).unwrap();
        std::fs::write(
            cursor_dir.join("mcp.json"),
            serde_json::to_string_pretty(&json!({
                "mcpServers": { "other": { "command": "other-tool" } }
            }))
            .unwrap(),
        )
        .unwrap();

        create_cursor_mcp_config(tmp.path()).unwrap();

        let config = read_json(&tmp.path().join(".cursor/mcp.json"));
        assert_eq!(config["mcpServers"]["other"]["command"], "other-tool");
        assert_eq!(config["mcpServers"]["scavenger"]["command"], "scavenger");
    }

    #[test]
    fn test_creates_cursor_hooks() {
        let tmp = tempfile::tempdir().unwrap();
        create_cursor_hooks(tmp.path()).unwrap();

        let config = read_json(&tmp.path().join(".cursor/hooks.json"));
        assert_eq!(config["version"], 1);

        let hooks = &config["hooks"];
        assert_eq!(
            hooks["sessionStart"][0]["command"],
            "scavenger hook session-start"
        );
        assert_eq!(
            hooks["sessionEnd"][0]["command"],
            "scavenger hook session-end"
        );
        assert_eq!(
            hooks["afterFileEdit"][0]["command"],
            "scavenger hook after-file-edit"
        );
    }

    #[test]
    fn test_cursor_hooks_preserves_existing_hooks() {
        let tmp = tempfile::tempdir().unwrap();
        let cursor_dir = tmp.path().join(".cursor");
        std::fs::create_dir_all(&cursor_dir).unwrap();
        std::fs::write(
            cursor_dir.join("hooks.json"),
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "hooks": {
                    "beforeShellExecution": [{ "command": "user-script.sh" }]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        create_cursor_hooks(tmp.path()).unwrap();

        let config = read_json(&tmp.path().join(".cursor/hooks.json"));
        assert_eq!(
            config["hooks"]["beforeShellExecution"][0]["command"],
            "user-script.sh"
        );
        assert!(config["hooks"]["sessionStart"].is_array());
    }

    #[test]
    fn test_cursor_config_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        create_cursor_mcp_config(tmp.path()).unwrap();
        create_cursor_hooks(tmp.path()).unwrap();
        create_cursor_mcp_config(tmp.path()).unwrap();
        create_cursor_hooks(tmp.path()).unwrap();

        let mcp = read_json(&tmp.path().join(".cursor/mcp.json"));
        assert!(mcp["mcpServers"]["scavenger"].is_object());

        let hooks = read_json(&tmp.path().join(".cursor/hooks.json"));
        // sessionStart has 2 entries: session-start + audit
        assert_eq!(hooks["hooks"]["sessionStart"].as_array().unwrap().len(), 2);
        // postToolUse has 1 entry: audit only
        assert_eq!(hooks["hooks"]["postToolUse"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_remove_cursor_config() {
        let tmp = tempfile::tempdir().unwrap();
        create_cursor_mcp_config(tmp.path()).unwrap();
        create_cursor_hooks(tmp.path()).unwrap();

        remove_cursor_config(tmp.path()).unwrap();

        assert!(!tmp.path().join(".cursor/mcp.json").exists());
        assert!(!tmp.path().join(".cursor/hooks.json").exists());
    }

    // ---- .mcp.json tests ----

    #[test]
    fn test_register_mcp_in_mcp_json_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        register_mcp_in_mcp_json(tmp.path()).unwrap();

        let config = read_json(&tmp.path().join(".mcp.json"));
        assert_eq!(config["mcpServers"]["scavenger"]["command"], "scavenger");
        assert_eq!(config["mcpServers"]["scavenger"]["args"][0], "mcp-bridge");
    }

    #[test]
    fn test_register_mcp_in_mcp_json_preserves_existing() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".mcp.json"),
            serde_json::to_string_pretty(&json!({
                "mcpServers": { "supabase": { "command": "npx", "args": ["-y", "@supabase/mcp-server-supabase"] } }
            }))
            .unwrap(),
        )
        .unwrap();

        register_mcp_in_mcp_json(tmp.path()).unwrap();

        let config = read_json(&tmp.path().join(".mcp.json"));
        assert_eq!(config["mcpServers"]["supabase"]["command"], "npx");
        assert_eq!(config["mcpServers"]["scavenger"]["command"], "scavenger");
    }

    #[test]
    fn test_register_mcp_in_mcp_json_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        register_mcp_in_mcp_json(tmp.path()).unwrap();
        register_mcp_in_mcp_json(tmp.path()).unwrap();

        let config = read_json(&tmp.path().join(".mcp.json"));
        let servers = config["mcpServers"].as_object().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(config["mcpServers"]["scavenger"]["command"], "scavenger");
    }

    #[test]
    fn test_remove_mcp_from_mcp_json_removes_scavenger() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".mcp.json"),
            serde_json::to_string_pretty(&json!({
                "mcpServers": {
                    "scavenger": { "command": "scavenger" },
                    "other": { "command": "other" }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        remove_mcp_from_mcp_json(tmp.path()).unwrap();

        let config = read_json(&tmp.path().join(".mcp.json"));
        assert!(config["mcpServers"].get("scavenger").is_none());
        assert_eq!(config["mcpServers"]["other"]["command"], "other");
    }

    #[test]
    fn test_remove_mcp_from_mcp_json_deletes_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        register_mcp_in_mcp_json(tmp.path()).unwrap();
        assert!(tmp.path().join(".mcp.json").exists());

        remove_mcp_from_mcp_json(tmp.path()).unwrap();
        assert!(!tmp.path().join(".mcp.json").exists());
    }

    #[test]
    fn test_remove_mcp_from_mcp_json_noop_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        remove_mcp_from_mcp_json(tmp.path()).unwrap();
    }

    // CLI registration (`register_mcp_via_cli`) is not unit-tested because it
    // spawns `claude` as a subprocess. It gracefully returns Ok(false) when the
    // CLI is missing, which is verified by integration tests.

    #[test]
    fn test_remove_cursor_config_preserves_other_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let cursor_dir = tmp.path().join(".cursor");
        std::fs::create_dir_all(&cursor_dir).unwrap();

        std::fs::write(
            cursor_dir.join("mcp.json"),
            serde_json::to_string_pretty(&json!({
                "mcpServers": {
                    "scavenger": { "command": "scavenger" },
                    "other": { "command": "other" }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            cursor_dir.join("hooks.json"),
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "hooks": {
                    "sessionStart": [{ "command": "scavenger hook session-start" }],
                    "beforeShellExecution": [{ "command": "user-script.sh" }]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        remove_cursor_config(tmp.path()).unwrap();

        let mcp = read_json(&tmp.path().join(".cursor/mcp.json"));
        assert_eq!(mcp["mcpServers"]["other"]["command"], "other");
        assert!(mcp["mcpServers"].get("scavenger").is_none());

        let hooks = read_json(&tmp.path().join(".cursor/hooks.json"));
        assert_eq!(
            hooks["hooks"]["beforeShellExecution"][0]["command"],
            "user-script.sh"
        );
        assert!(hooks["hooks"].get("sessionStart").is_none());
    }
}
