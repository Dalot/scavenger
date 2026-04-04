# Daemon Subcommand Group Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the flat `scavenger daemon` start-only command with a `daemon {start,stop,restart,status}` subcommand group so users have a clear, discoverable way to manage the daemon lifecycle.

**Architecture:** The current `Commands::Daemon` unit variant becomes a subcommand group (`DaemonCommands` enum). The kill/cleanup logic already in `kill_daemon_and_wait` is reused by both `daemon stop` and `hook session-end`. Hooks remain unchanged—`hook session-end` still kills the daemon so IDE automation keeps working. The difference is now users have an obvious `scavenger daemon stop` instead of needing to know about the hook plumbing.

**Tech Stack:** Rust, clap derive API, existing daemon/socket infrastructure.

**Version bump:** 0.1.2 → 0.2.0. This is a breaking CLI change (`scavenger daemon` alone no longer starts the daemon—it now requires `scavenger daemon start`). Under SemVer 0.x, minor bumps signal breaking changes. The hook commands are unchanged so IDE integrations continue working.

---

### Task 1: Add `DaemonCommands` enum and update CLI definition

**Files:**
- Modify: `src/main.rs:27-34` (Commands enum)
- Modify: `src/main.rs:283-285` (match arm)

**Step 1: Update the `Commands` enum to make `Daemon` a subcommand group**

Change the `Daemon` variant from a unit variant to a subcommand group:

```rust
/// Manage the daemon process
Daemon {
    #[command(subcommand)]
    command: DaemonCommands,
},
```

**Step 2: Add the `DaemonCommands` enum after `Commands`**

Add it right after the `Commands` enum, before `GraphCommands`:

```rust
#[derive(Subcommand)]
enum DaemonCommands {
    /// Start the daemon in the foreground
    Start,
    /// Stop a running daemon
    Stop,
    /// Stop and restart the daemon (foreground)
    Restart,
    /// Show daemon status (running, PID, branch, uptime)
    Status,
}
```

**Step 3: Update the match arm in `main()`**

Change:
```rust
Commands::Daemon => cmd_daemon(),
```
To:
```rust
Commands::Daemon { command } => cmd_daemon(command),
```

**Step 4: Verify it compiles**

Run: `cargo check 2>&1 | head -30`
Expected: Compilation error in `cmd_daemon` (signature mismatch) — that's expected, we fix it in Task 2.

**Step 5: Commit**

```bash
git add src/main.rs
git commit -m "refactor: convert daemon to subcommand group with start/stop/restart/status"
```

---

### Task 2: Implement `cmd_daemon` dispatcher and `daemon status`

**Files:**
- Modify: `src/main.rs:444-452` (cmd_daemon function)

**Step 1: Rewrite `cmd_daemon` to dispatch subcommands**

Replace the existing `cmd_daemon()` function with:

```rust
fn cmd_daemon(command: DaemonCommands) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        DaemonCommands::Start => cmd_daemon_start(),
        DaemonCommands::Stop => cmd_daemon_stop(),
        DaemonCommands::Restart => {
            // Stop if running, then start
            let _ = cmd_daemon_stop();
            cmd_daemon_start()
        }
        DaemonCommands::Status => cmd_daemon_status(),
    }
}
```

**Step 2: Rename old `cmd_daemon` body to `cmd_daemon_start`**

```rust
fn cmd_daemon_start() -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    if !scavenger_dir.exists() {
        return Err("Not initialized. Run `scavenger init` first.".into());
    }
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(daemon::run_daemon(project_root))
}
```

**Step 3: Create `cmd_daemon_stop`**

```rust
fn cmd_daemon_stop() -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    let pid_path = scavenger_dir.join("daemon.pid");
    if let Some(pid) = read_pid(&pid_path) {
        eprintln!("Stopping daemon (PID {pid})...");
        kill_daemon_and_wait(pid, &scavenger_dir);
        eprintln!("{}", "Daemon stopped.".green().bold());
    } else {
        eprintln!("No running daemon found.");
    }
    Ok(())
}
```

**Step 4: Create `cmd_daemon_status`**

```rust
fn cmd_daemon_status() -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    let pid_path = scavenger_dir.join("daemon.pid");

    if !is_daemon_running(&pid_path) {
        println!("{}", "Daemon is not running.".yellow());
        return Ok(());
    }

    let pid = read_pid(&pid_path).unwrap_or(0);
    println!("{} (PID {})", "Daemon is running.".green().bold(), pid);

    // Try to fetch live status over socket
    let socket_path = scavenger_dir.join("daemon.sock");
    if socket_path.exists() {
        let rt = tokio::runtime::Runtime::new()?;
        let request = serde_json::json!({ "method": "status" });
        if let Ok(status) = rt.block_on(daemon::socket::send_request(&socket_path, &request)) {
            if let Some(branch) = status.get("branch").and_then(|v| v.as_str()) {
                println!("  Branch:        {}", branch.cyan());
            }
            if let Some(state) = status.get("reindex_state").and_then(|v| v.as_str()) {
                println!("  Reindex state: {state}");
            }
            if let Some(nodes) = status.get("node_count").and_then(|v| v.as_u64()) {
                let edges = status.get("edge_count").and_then(|v| v.as_u64()).unwrap_or(0);
                println!("  Graph:         {nodes} nodes, {edges} edges");
            }
        }
    }

    Ok(())
}
```

**Step 5: Verify it compiles and runs**

Run: `cargo check`
Expected: PASS

Run: `cargo run -- daemon status`
Expected: Either "Daemon is not running." or status output.

Run: `cargo run -- daemon --help`
Expected: Shows start/stop/restart/status subcommands.

**Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: implement daemon start/stop/restart/status subcommands"
```

---

### Task 3: Update `hook session-start` to use `daemon start` arg

**Files:**
- Modify: `src/main.rs:1195-1210` (SessionStart hook handler)

The `hook session-start` currently spawns `scavenger daemon` (the old bare command). It needs to spawn `scavenger daemon start` instead.

**Step 1: Update the spawn command**

Change:
```rust
std::process::Command::new(exe)
    .arg("daemon")
```
To:
```rust
std::process::Command::new(exe)
    .args(["daemon", "start"])
```

**Step 2: Verify**

Run: `cargo check`
Expected: PASS

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "fix: hook session-start spawns 'daemon start' instead of bare 'daemon'"
```

---

### Task 4: Update doctor recommendations

**Files:**
- Modify: `src/main.rs:683-695` (doctor recommendations mentioning old commands)

**Step 1: Update the daemon start recommendation**

Change:
```rust
"Start the daemon: scavenger daemon (or trigger via session hook)".into()
```
To:
```rust
"Start the daemon: scavenger daemon start (or trigger via session hook)".into()
```

**Step 2: Update the socket-missing recommendation**

Change:
```rust
"Socket missing despite running daemon — restart: scavenger hook session-end && scavenger daemon".into()
```
To:
```rust
"Socket missing despite running daemon — restart: scavenger daemon restart".into()
```

**Step 3: Verify**

Run: `cargo check`
Expected: PASS

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "fix: update doctor recommendations to use new daemon subcommands"
```

---

### Task 5: Update README CLI reference

**Files:**
- Modify: `README.md:143` (auto-managed note)
- Modify: `README.md:199-220` (CLI Reference table)

**Step 1: Update the auto-managed note after init**

Change:
```
The daemon starts and stops automatically with each agent session — no manual `scavenger daemon` needed.
```
To:
```
The daemon starts and stops automatically with each agent session — no manual management needed. You can also control it explicitly with `scavenger daemon start`, `scavenger daemon stop`, and `scavenger daemon status`.
```

**Step 2: Update the CLI Reference table**

Replace the daemon row:
```
| `scavenger daemon` | Start the daemon manually in foreground (normally auto-managed) |
```
With the four subcommand rows:
```
| `scavenger daemon start` | Start the daemon in foreground (normally auto-managed by hooks) |
| `scavenger daemon stop` | Stop a running daemon |
| `scavenger daemon restart` | Stop and restart the daemon |
| `scavenger daemon status` | Show daemon status (running, PID, branch, graph size) |
```

**Step 3: Commit**

```bash
git add README.md
git commit -m "docs: update README with daemon subcommand group"
```

---

### Task 6: Bump version to 0.2.0

**Files:**
- Modify: `Cargo.toml:2` (version field)
- Modify: `src/hooks/register.rs:207` (plugin.json version string)

**Step 1: Update Cargo.toml version**

Change:
```toml
version = "0.1.2"
```
To:
```toml
version = "0.2.0"
```

**Step 2: Update the Claude Code plugin version in register.rs**

Change:
```rust
"version": "0.1.1"
```
To:
```rust
"version": "0.2.0"
```

**Step 3: Regenerate Cargo.lock**

Run: `cargo check`
Expected: PASS (Cargo.lock updates automatically)

**Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/hooks/register.rs
git commit -m "chore: bump version to 0.2.0 (breaking CLI: daemon subcommand group)"
```

---

### Task 7: Update the `cmd_init` output messaging

**Files:**
- Modify: `src/main.rs:440` (the "daemon starts and stops" message in `cmd_init`)

**Step 1: Update the init output**

Change:
```rust
eprintln!("\nThe daemon starts and stops automatically with each session.");
```
To:
```rust
eprintln!("\nThe daemon starts and stops automatically with each session.");
eprintln!("Manual control: scavenger daemon {{start|stop|restart|status}}");
```

**Step 2: Verify**

Run: `cargo check`
Expected: PASS

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "fix: init output mentions manual daemon control"
```

---

### Task 8: Write integration test for daemon subcommands

**Files:**
- Create: `tests/daemon_cli_test.rs`

**Step 1: Write the test**

```rust
use std::process::Command;

fn scavenger_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_scavenger"))
}

#[test]
fn daemon_help_shows_subcommands() {
    let output = scavenger_bin()
        .args(["daemon", "--help"])
        .output()
        .expect("failed to run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("start"), "help should list 'start'");
    assert!(stdout.contains("stop"), "help should list 'stop'");
    assert!(stdout.contains("restart"), "help should list 'restart'");
    assert!(stdout.contains("status"), "help should list 'status'");
}

#[test]
fn daemon_status_when_not_initialized() {
    let tmp = tempfile::tempdir().unwrap();
    let output = scavenger_bin()
        .args(["daemon", "status"])
        .current_dir(tmp.path())
        .output()
        .expect("failed to run");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, String::from_utf8_lossy(&output.stderr));
    // Should indicate not running (not panic)
    assert!(
        combined.contains("not running") || combined.contains("Not initialized"),
        "status in uninitialized dir should not panic: {combined}"
    );
}

#[test]
fn daemon_stop_when_nothing_running() {
    let tmp = tempfile::tempdir().unwrap();
    // Create minimal .scavenger dir so it doesn't bail on "not initialized"
    std::fs::create_dir(tmp.path().join(".scavenger")).unwrap();
    let output = scavenger_bin()
        .args(["daemon", "stop"])
        .current_dir(tmp.path())
        .output()
        .expect("failed to run");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("No running daemon"),
        "stop with no daemon should say so: {stderr}"
    );
    assert!(output.status.success());
}

#[test]
fn bare_daemon_without_subcommand_shows_help() {
    let output = scavenger_bin()
        .args(["daemon"])
        .output()
        .expect("failed to run");
    // clap shows an error when subcommand is required but missing
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() || stderr.contains("Usage"),
        "bare 'daemon' should require a subcommand"
    );
}
```

**Step 2: Run the tests**

Run: `cargo test --test daemon_cli_test -- --nocapture`
Expected: All 4 tests PASS

**Step 3: Commit**

```bash
git add tests/daemon_cli_test.rs
git commit -m "test: add CLI tests for daemon subcommand group"
```

---

### Task 9: Run full test suite

**Step 1: Run all tests**

Run: `cargo test`
Expected: All existing tests still pass. No regressions.

**Step 2: Check for any remaining references to old `scavenger daemon` (without subcommand) in code**

Run: `rg '"scavenger daemon"' --type rust` (or equivalent grep)

If any matches remain that should say `"scavenger daemon start"`, fix them.

**Step 3: Commit any fixes**

```bash
git add -A
git commit -m "fix: update remaining references to old bare 'scavenger daemon' command"
```

---

## Summary of changes

| What | Before | After |
|------|--------|-------|
| Start daemon | `scavenger daemon` | `scavenger daemon start` |
| Stop daemon | `scavenger hook session-end` | `scavenger daemon stop` |
| Restart daemon | `scavenger hook session-end && scavenger daemon` | `scavenger daemon restart` |
| Check daemon | (no command) | `scavenger daemon status` |
| Hook session-end | Kills daemon | **Unchanged** — still kills daemon for IDE automation |
| Version | 0.1.2 | 0.2.0 |

## Future enhancements (not in this plan)

- **Idle auto-shutdown**: Daemon stops itself after N minutes of no requests. `hook session-end` would just notify the daemon "session ended" (start the idle timer) instead of killing it. This avoids cold-start cost when opening a new session quickly.
- **Background start**: `scavenger daemon start --background` to daemonize the process (currently it runs in foreground).
- **`scavenger daemon logs`**: Alias for the existing `scavenger logs` command, grouped under `daemon` for discoverability.
