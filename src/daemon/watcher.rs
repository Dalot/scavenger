use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::RecursiveMode;
use notify::event::{EventKind, ModifyKind};
use notify_debouncer_full::{DebouncedEvent, new_debouncer};
use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::DaemonState;

/// Events emitted by the file watcher after debounce.
#[derive(Debug)]
pub enum WatchEvent {
    FilesChanged(Vec<PathBuf>),
    BranchSwitch,
}

/// Start the file watcher. Returns a channel receiver for debounced events.
/// Uses notify-debouncer-full with 300ms trailing-edge debounce.
/// Respects .gitignore via the ignore crate at event filtering time.
pub fn start_watcher(
    project_root: &Path,
    _state: Arc<DaemonState>,
) -> Result<mpsc::UnboundedReceiver<WatchEvent>, Box<dyn std::error::Error>> {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let root = project_root.to_path_buf();

    let git_dir = root.join(".git");
    let scavenger_dir = root.join(".scavenger");

    let pending_events: Arc<Mutex<Vec<DebouncedEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let pending_clone = pending_events.clone();
    let event_tx_clone = event_tx.clone();
    let root_clone = root.clone();

    // VCS deferral state
    let vcs_deferred = Arc::new(Mutex::new(false));
    let vcs_deferred_clone = vcs_deferred.clone();

    let mut debouncer = new_debouncer(
        Duration::from_millis(300),
        None,
        move |result: Result<Vec<DebouncedEvent>, Vec<notify::Error>>| match result {
            Ok(events) => {
                let mut pending = pending_clone.lock();
                pending.extend(events);
            }
            Err(errors) => {
                for e in errors {
                    tracing::warn!("watcher error: {e}");
                }
            }
        },
    )?;

    debouncer.watch(&root, RecursiveMode::Recursive)?;

    // Spawn a processing loop that drains pending events periodically
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(350));
        loop {
            interval.tick().await;

            let events: Vec<DebouncedEvent> = {
                let mut pending = pending_events.lock();
                std::mem::take(&mut *pending)
            };

            if events.is_empty() {
                continue;
            }

            let mut changed_files = HashSet::new();
            let mut saw_git_head = false;
            let mut saw_git_index_lock = false;

            for event in &events {
                // Skip non-mutating events: access (open/close/read) and
                // metadata-only changes (atime, permissions). Without this
                // filter, the daemon's own file reads trigger IN_ATTRIB via
                // inotify, creating an infinite reindex loop.
                match event.kind {
                    EventKind::Access(_) => continue,
                    EventKind::Modify(ModifyKind::Metadata(_)) => continue,
                    _ => {}
                }

                for path in &event.paths {
                    // Check VCS operations
                    if path.starts_with(&git_dir) {
                        if path.ends_with("index.lock") {
                            saw_git_index_lock = true;
                        }
                        if path.ends_with("HEAD") || path.file_name().is_some_and(|n| n == "HEAD") {
                            saw_git_head = true;
                        }
                        continue;
                    }

                    // Skip .scavenger/ directory
                    if path.starts_with(&scavenger_dir) {
                        continue;
                    }

                    // Skip gitignored files
                    if is_gitignored(path, &root_clone) {
                        continue;
                    }

                    if path.is_file() {
                        tracing::info!(kind = ?event.kind, path = %path.display(), "watcher: file changed");
                        changed_files.insert(path.clone());
                    }
                }
            }

            // VCS deferral: if index.lock appeared, defer processing
            if saw_git_index_lock {
                *vcs_deferred_clone.lock() = true;
                continue;
            }

            // If we were deferred and index.lock is gone, process now
            let was_deferred = {
                let mut d = vcs_deferred_clone.lock();
                let was = *d;
                *d = false;
                was
            };

            // Branch switch detection: HEAD changed after VCS batch
            if saw_git_head && (was_deferred || !saw_git_index_lock) {
                let _ = event_tx_clone.send(WatchEvent::BranchSwitch);
            }

            if !changed_files.is_empty() {
                let files: Vec<PathBuf> = changed_files.into_iter().collect();
                tracing::debug!(count = files.len(), files = ?files, "watcher batch");
                let _ = event_tx_clone.send(WatchEvent::FilesChanged(files));
            }
        }
    });

    // Keep the debouncer alive by leaking it (it needs to live for the daemon's lifetime)
    std::mem::forget(debouncer);

    Ok(event_rx)
}

/// Check if a path is git-ignored using `git check-ignore`.
/// Handles ALL gitignore sources: root .gitignore, nested .gitignore files,
/// .git/info/exclude, and global gitignore config.
pub fn is_gitignored(path: &Path, project_root: &Path) -> bool {
    std::process::Command::new("git")
        .args(["check-ignore", "-q"])
        .arg(path)
        .current_dir(project_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Route a changed file to the appropriate indexer.
pub fn route_file(path: &Path) -> FileRoute {
    match path.extension().and_then(|e| e.to_str()) {
        Some("md") | Some("markdown") => FileRoute::Doc,
        Some(ext) if is_code_extension(ext) => FileRoute::Code,
        _ => FileRoute::Skip,
    }
}

#[derive(Debug, PartialEq)]
pub enum FileRoute {
    Code,
    Doc,
    Skip,
}

fn is_code_extension(ext: &str) -> bool {
    matches!(
        ext,
        "rs" | "py"
            | "pyi"
            | "ts"
            | "tsx"
            | "js"
            | "mjs"
            | "cjs"
            | "jsx"
            | "go"
            | "java"
            | "cs"
            | "c"
            | "h"
            | "cpp"
            | "cc"
            | "cxx"
            | "hpp"
            | "hxx"
            | "rb"
            | "sh"
            | "bash"
            | "php"
            | "swift"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_code_files() {
        assert_eq!(route_file(Path::new("foo.rs")), FileRoute::Code);
        assert_eq!(route_file(Path::new("bar.py")), FileRoute::Code);
        assert_eq!(route_file(Path::new("baz.ts")), FileRoute::Code);
        assert_eq!(route_file(Path::new("qux.go")), FileRoute::Code);
    }

    #[test]
    fn test_route_doc_files() {
        assert_eq!(route_file(Path::new("README.md")), FileRoute::Doc);
        assert_eq!(route_file(Path::new("docs/guide.markdown")), FileRoute::Doc);
    }

    #[test]
    fn test_route_skip_files() {
        assert_eq!(route_file(Path::new("image.png")), FileRoute::Skip);
        assert_eq!(route_file(Path::new("data.json")), FileRoute::Skip);
    }

    #[test]
    fn test_is_gitignored() {
        let tmp = std::env::temp_dir().join("scavenger_test_gitignore");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        // Init a git repo
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&tmp)
            .output()
            .unwrap();

        // Write .gitignore with patterns
        std::fs::write(tmp.join(".gitignore"), "ignored_dir/\n*.log\n.secret/\n").unwrap();

        // Create test files and directories
        std::fs::create_dir_all(tmp.join("ignored_dir")).unwrap();
        std::fs::write(tmp.join("ignored_dir/file.py"), "x").unwrap();
        std::fs::create_dir_all(tmp.join(".secret")).unwrap();
        std::fs::write(tmp.join(".secret/keys.py"), "x").unwrap();
        std::fs::write(tmp.join("app.log"), "x").unwrap();
        std::fs::create_dir_all(tmp.join("src")).unwrap();
        std::fs::write(tmp.join("src/main.py"), "x").unwrap();

        // Files in gitignored directories should be ignored
        assert!(
            is_gitignored(&tmp.join("ignored_dir/file.py"), &tmp),
            "file in ignored_dir/ should be gitignored"
        );
        assert!(
            is_gitignored(&tmp.join(".secret/keys.py"), &tmp),
            "file in .secret/ should be gitignored"
        );

        // Files matching gitignore patterns should be ignored
        assert!(
            is_gitignored(&tmp.join("app.log"), &tmp),
            "*.log files should be gitignored"
        );

        // Tracked source files should NOT be ignored
        assert!(
            !is_gitignored(&tmp.join("src/main.py"), &tmp),
            "src/main.py should NOT be gitignored"
        );

        // Non-existent file in ignored dir should still be ignored
        assert!(
            is_gitignored(&tmp.join("ignored_dir/new_file.py"), &tmp),
            "non-existent file in ignored_dir/ should be gitignored"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_is_gitignored_nested_gitignore() {
        let tmp = std::env::temp_dir().join("scavenger_test_nested_gitignore");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&tmp)
            .output()
            .unwrap();

        // Root .gitignore only ignores *.log
        std::fs::write(tmp.join(".gitignore"), "*.log\n").unwrap();

        // Nested .gitignore ignores a local directory
        std::fs::create_dir_all(tmp.join("subdir/cache")).unwrap();
        std::fs::write(tmp.join("subdir/.gitignore"), "cache/\n").unwrap();
        std::fs::write(tmp.join("subdir/cache/data.py"), "x").unwrap();
        std::fs::write(tmp.join("subdir/real.py"), "x").unwrap();

        assert!(
            is_gitignored(&tmp.join("subdir/cache/data.py"), &tmp),
            "file in subdir/cache/ should be caught by nested .gitignore"
        );
        assert!(
            !is_gitignored(&tmp.join("subdir/real.py"), &tmp),
            "subdir/real.py should NOT be gitignored"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
