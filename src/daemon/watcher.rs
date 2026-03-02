use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::RecursiveMode;
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

    // Build gitignore matcher for filtering
    let gitignore = build_gitignore(&root);

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
                    eprintln!("Watcher error: {e}");
                }
            }
        },
    )?;

    debouncer.watch(&root, RecursiveMode::Recursive)?;

    // Spawn a processing loop that drains pending events periodically
    let gitignore_clone = gitignore;
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
                for path in &event.paths {
                    // Check VCS operations
                    if path.starts_with(&git_dir) {
                        if path.ends_with("index.lock") {
                            saw_git_index_lock = true;
                        }
                        if path.ends_with("HEAD") || path.file_name().map_or(false, |n| n == "HEAD")
                        {
                            saw_git_head = true;
                        }
                        continue;
                    }

                    // Skip .scavenger/ directory
                    if path.starts_with(&scavenger_dir) {
                        continue;
                    }

                    // Skip gitignored files
                    if is_gitignored(path, &root_clone, &gitignore_clone) {
                        continue;
                    }

                    if path.is_file() {
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
                let _ = event_tx_clone.send(WatchEvent::FilesChanged(files));
            }
        }
    });

    // Keep the debouncer alive by leaking it (it needs to live for the daemon's lifetime)
    std::mem::forget(debouncer);

    Ok(event_rx)
}

fn build_gitignore(root: &Path) -> ignore::gitignore::Gitignore {
    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    let gitignore_path = root.join(".gitignore");
    if gitignore_path.exists() {
        let _ = builder.add(&gitignore_path);
    }
    builder.build().unwrap_or_else(|_| {
        ignore::gitignore::GitignoreBuilder::new(root)
            .build()
            .unwrap()
    })
}

fn is_gitignored(path: &Path, root: &Path, gitignore: &ignore::gitignore::Gitignore) -> bool {
    let is_dir = path.is_dir();
    gitignore
        .matched_path_or_any_parents(path.strip_prefix(root).unwrap_or(path), is_dir)
        .is_ignore()
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
}
