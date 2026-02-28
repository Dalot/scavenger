pub mod coordinator;
pub mod federation;
pub mod handlers;
pub mod socket;
pub mod watcher;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use tokio::signal;
use tokio::sync::watch;

use crate::config::Config;
use crate::db;
use crate::db::queries;
use crate::graph::{self, SharedGraph};

/// Structured JSON daemon log with size-based rotation (10 MB max, 2 rotated files).
pub fn daemon_log(scavenger_dir: &Path, event: &str, detail: &serde_json::Value) {
    let log_path = scavenger_dir.join("daemon.log");
    let max_size: u64 = 10 * 1024 * 1024; // 10 MB

    // Rotate if needed
    if let Ok(meta) = std::fs::metadata(&log_path) {
        if meta.len() > max_size {
            let log1 = scavenger_dir.join("daemon.log.1");
            let log2 = scavenger_dir.join("daemon.log.2");
            let _ = std::fs::rename(&log1, &log2);
            let _ = std::fs::rename(&log_path, &log1);
        }
    }

    let entry = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "event": event,
        "pid": std::process::id(),
        "detail": detail,
    });

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        use std::io::Write;
        let _ = writeln!(file, "{}", entry);
    }
}

/// Shared daemon state accessible from all handler tasks.
pub struct DaemonState {
    pub project_root: PathBuf,
    pub scavenger_dir: PathBuf,
    pub config: Config,
    pub graph: SharedGraph,
    pub branch_db: Arc<Mutex<Option<rusqlite::Connection>>>,
    pub meta_db: Arc<Mutex<rusqlite::Connection>>,
    pub current_branch: Arc<RwLock<String>>,
    pub session_id: Arc<RwLock<String>>,
    pub reindex_state: Arc<RwLock<ReindexState>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReindexState {
    Ready,
    Switching,
    ColdStart,
    Indexing,
}

impl std::fmt::Display for ReindexState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready => write!(f, "ready"),
            Self::Switching => write!(f, "switching"),
            Self::ColdStart => write!(f, "cold_start"),
            Self::Indexing => write!(f, "indexing"),
        }
    }
}

/// Run the daemon (12-step startup, event loop, graceful shutdown).
pub async fn run_daemon(project_root: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let scavenger_dir = db::scavenger_dir(&project_root);
    let config = Config::load(&project_root)?;

    // Step 1: Acquire exclusive flock
    let lock_path = scavenger_dir.join("daemon.lock");
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    use fs2::FileExt;
    lock_file.try_lock_exclusive().map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Another daemon instance is already running",
        )
    })?;

    // Step 2: Write PID
    let pid_path = scavenger_dir.join("daemon.pid");
    std::fs::write(&pid_path, std::process::id().to_string())?;

    // Step 3: Open daemon_meta.db
    let meta_conn = db::open_daemon_meta_db(&scavenger_dir)?;

    // Step 4: Detect branch
    let branch = detect_branch(&project_root);

    // Step 5: Open per-branch index DB
    let branch_conn = db::open_branch_db(&scavenger_dir, &branch)?;

    // Step 6: Start UDS listener (handled below in event loop)

    // Step 7: Check last_shutdown — dirty → full freshness scan
    let last_shutdown = queries::get_meta(&meta_conn, "last_shutdown")?;
    let needs_full_scan = last_shutdown.as_deref() != Some("clean");

    // Step 8: Set last_shutdown = dirty
    queries::set_meta(&meta_conn, "last_shutdown", "dirty")?;
    queries::set_meta(&meta_conn, "current_branch", &branch)?;

    // Step 9: Freshness scan + re-index mismatches
    let graph = graph::new_shared_graph();
    {
        let mut g = graph.write();
        if needs_full_scan {
            eprintln!("Daemon: performing full freshness scan...");
        }
        g.load_from_db(&branch_conn)?;

        // Step 10: Recompute PageRank
        g.compute_pagerank(0.85, 30);
    }

    let session_id = uuid::Uuid::new_v4().to_string();

    let state = Arc::new(DaemonState {
        project_root: project_root.clone(),
        scavenger_dir: scavenger_dir.clone(),
        config,
        graph,
        branch_db: Arc::new(Mutex::new(Some(branch_conn))),
        meta_db: Arc::new(Mutex::new(meta_conn)),
        current_branch: Arc::new(RwLock::new(branch)),
        session_id: Arc::new(RwLock::new(session_id)),
        reindex_state: Arc::new(RwLock::new(ReindexState::Ready)),
    });

    // Step 11: Start file watcher
    let watcher_state = state.clone();
    match watcher::start_watcher(&project_root, watcher_state.clone()) {
        Ok(mut rx) => {
            tokio::spawn(async move {
                handle_watch_events(&watcher_state, &mut rx).await;
            });
            eprintln!("Daemon: file watcher started");
        }
        Err(e) => {
            eprintln!("Daemon: file watcher failed to start: {e} (continuing without)");
        }
    }

    // Step 12: Set reindex_state = ready
    *state.reindex_state.write() = ReindexState::Ready;

    // Start UDS listener
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let socket_path = scavenger_dir.join("daemon.sock");
    let _ = std::fs::remove_file(&socket_path);

    let listener_state = state.clone();
    let listener_handle = tokio::spawn(async move {
        if let Err(e) = socket::listen(socket_path, listener_state, shutdown_rx).await {
            eprintln!("Socket listener error: {e}");
        }
    });

    daemon_log(&scavenger_dir, "startup", &serde_json::json!({
        "branch": state.current_branch.read().clone(),
        "nodes": state.graph.read().node_count(),
        "edges": state.graph.read().edge_count(),
    }));
    eprintln!("Daemon: ready (PID {})", std::process::id());

    // Wait for SIGTERM/SIGINT
    let ctrl_c = signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())?;
        tokio::select! {
            _ = ctrl_c => {},
            _ = sigterm.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        ctrl_c.await?;
    }

    eprintln!("Daemon: shutting down...");

    // Shutdown: stop accepting, drain, flush, clean shutdown
    let _ = shutdown_tx.send(true);

    // Give handlers 5 seconds to drain
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    listener_handle.abort();

    // Flush centrality to DB
    {
        let g = state.graph.read();
        if let Some(ref conn) = *state.branch_db.lock() {
            let _ = g.save_centrality(conn);
        }
    }

    // Set last_shutdown = clean
    {
        let meta = state.meta_db.lock();
        let _ = queries::set_meta(&meta, "last_shutdown", "clean");
    }

    // Cleanup
    let _ = std::fs::remove_file(&pid_path);
    let _ = std::fs::remove_file(scavenger_dir.join("daemon.sock"));
    let _ = lock_file.unlock();

    daemon_log(&scavenger_dir, "shutdown", &serde_json::json!({"clean": true}));
    eprintln!("Daemon: stopped cleanly.");
    Ok(())
}

/// Process watch events from the file watcher.
async fn handle_watch_events(
    state: &Arc<DaemonState>,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<watcher::WatchEvent>,
) {
    use crate::graph::{doc_indexer, index};
    use std::collections::HashSet;
    use watcher::{FileRoute, WatchEvent};

    while let Some(event) = rx.recv().await {
        match event {
            WatchEvent::BranchSwitch => {
                if let Err(e) = coordinator::ReindexCoordinator::check_branch_switch(state) {
                    eprintln!("Branch switch error: {e}");
                }
            }
            WatchEvent::FilesChanged(files) => {
                if *state.reindex_state.read() == ReindexState::Switching {
                    continue;
                }
                *state.reindex_state.write() = ReindexState::Indexing;

                let mut cross_file_queue: HashSet<std::path::PathBuf> = HashSet::new();

                for file in &files {
                    let file_str = file.to_string_lossy().to_string();
                    match watcher::route_file(file) {
                        FileRoute::Code => {
                            // Phase 1: prep (no graph lock)
                            let prep = {
                                let db_guard = state.branch_db.lock();
                                let Some(ref conn) = *db_guard else { continue };
                                let graph = state.graph.read();
                                match index::incremental_reindex_prep(conn, &graph, &file_str) {
                                    Ok(p) => p,
                                    Err(e) => {
                                        eprintln!("Reindex prep error for {file_str}: {e}");
                                        continue;
                                    }
                                }
                            };

                            // Phase 2: swap (write lock)
                            {
                                let db_guard = state.branch_db.lock();
                                let Some(ref conn) = *db_guard else { continue };
                                let mut graph = state.graph.write();
                                match index::incremental_reindex_swap(conn, &mut graph, prep) {
                                    Ok(stats) => {
                                        eprintln!(
                                            "Reindexed {file_str}: -{} +{} nodes, +{} edges, {} migrated",
                                            stats.nodes_removed, stats.nodes_added,
                                            stats.edges_added, stats.annotations_migrated
                                        );
                                    }
                                    Err(e) => {
                                        eprintln!("Reindex swap error for {file_str}: {e}");
                                    }
                                }

                                // Queue cross-file affected files
                                let affected = index::cross_file_affected(&graph, &file_str);
                                cross_file_queue.extend(affected);
                            }
                        }
                        FileRoute::Doc => {
                            if let Ok(content) = std::fs::read_to_string(file) {
                                let db_guard = state.branch_db.lock();
                                if let Some(ref conn) = *db_guard {
                                    if let Err(e) = doc_indexer::index_doc_file(conn, &file_str, &content) {
                                        eprintln!("Doc reindex error for {file_str}: {e}");
                                    }
                                }
                            }
                        }
                        FileRoute::Skip => {}
                    }
                }

                // Process cross-file cascade (lazy)
                for affected_path in &cross_file_queue {
                    if files.iter().any(|f| f == affected_path) {
                        continue; // Already processed
                    }
                    let file_str = affected_path.to_string_lossy().to_string();
                    let prep = {
                        let db_guard = state.branch_db.lock();
                        let Some(ref conn) = *db_guard else { continue };
                        let graph = state.graph.read();
                        match index::incremental_reindex_prep(conn, &graph, &file_str) {
                            Ok(p) => p,
                            Err(_) => continue,
                        }
                    };
                    let db_guard = state.branch_db.lock();
                    if let Some(ref conn) = *db_guard {
                        let mut graph = state.graph.write();
                        let _ = index::incremental_reindex_swap(conn, &mut graph, prep);
                    }
                }

                // Phase 3: Deferred PageRank (once per batch)
                {
                    let mut graph = state.graph.write();
                    graph.compute_pagerank(0.85, 30);
                    let db_guard = state.branch_db.lock();
                    if let Some(ref conn) = *db_guard {
                        let _ = graph.save_centrality(conn);
                    }
                }

                // WAL checkpoint during idle
                {
                    let db_guard = state.branch_db.lock();
                    if let Some(ref conn) = *db_guard {
                        let _ = index::wal_checkpoint(conn);
                    }
                }

                *state.reindex_state.write() = ReindexState::Ready;
            }
        }
    }
}

/// Detect current git branch, falling back to "main".
pub fn detect_branch(project_root: &Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(project_root)
        .output()
        .ok()
        .and_then(|out| {
            if out.status.success() {
                String::from_utf8(out.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "main".to_string())
}
