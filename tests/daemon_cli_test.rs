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
    assert!(
        combined.contains("not running") || combined.contains("Not initialized"),
        "status in uninitialized dir should not panic: {combined}"
    );
}

#[test]
fn daemon_stop_when_nothing_running() {
    let tmp = tempfile::tempdir().unwrap();
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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success() || stderr.contains("Usage"),
        "bare 'daemon' should require a subcommand"
    );
}
