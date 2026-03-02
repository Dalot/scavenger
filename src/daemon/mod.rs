pub mod coordinator;
pub mod effectiveness;
pub mod federation;
pub mod handlers;
pub mod metrics;
pub mod socket;
pub mod watcher;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::{Mutex, RwLock};
use tokio::signal;
use tokio::sync::watch;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::config::Config;
use crate::db;
use crate::db::queries;
use crate::graph::{self, SharedGraph};

/// Global request counter for generating lightweight request IDs.
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn next_request_id() -> u64 {
    REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Initialize the tracing subscriber with two layers:
/// - JSON file layer -> .scavenger/daemon.log (daily rotation)
/// - Compact stderr layer for foreground daemon mode
/// - (optional, `telemetry` feature) OpenTelemetry OTLP export layer
///
/// Log level controlled by SCAVENGER_LOG env var (default: info).
pub fn init_tracing(scavenger_dir: &Path) {
    let file_appender = tracing_appender::rolling::daily(scavenger_dir, "daemon.log");

    let file_layer = fmt::layer()
        .json()
        .with_writer(file_appender)
        .with_target(true)
        .with_span_events(fmt::format::FmtSpan::CLOSE);

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false)
        .compact();

    let filter = EnvFilter::try_from_env("SCAVENGER_LOG")
        .unwrap_or_else(|_| EnvFilter::new("scavenger=info"));

    #[cfg(feature = "telemetry")]
    {
        use opentelemetry::trace::TracerProvider;
        use opentelemetry_otlp::SpanExporter;
        use tracing_opentelemetry::OpenTelemetryLayer;

        let exporter = match SpanExporter::builder().with_tonic().build() {
            Ok(e) => e,
            Err(e) => {
                eprintln!("OpenTelemetry exporter init failed: {e}");
                tracing_subscriber::registry()
                    .with(filter)
                    .with(file_layer)
                    .with(stderr_layer)
                    .init();
                return;
            }
        };

        let provider = opentelemetry_sdk::trace::TracerProvider::builder()
            .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
            .build();

        let tracer = provider.tracer("scavenger");
        let otel_layer = OpenTelemetryLayer::new(tracer);

        tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .with(stderr_layer)
            .with(otel_layer)
            .init();
    }

    #[cfg(not(feature = "telemetry"))]
    {
        tracing_subscriber::registry()
            .with(filter)
            .with(file_layer)
            .with(stderr_layer)
            .init();
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
    pub metrics: Arc<metrics::DaemonMetrics>,
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

    // Initialize tracing before anything else
    init_tracing(&scavenger_dir);

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
            tracing::info!("performing full freshness scan (last shutdown was dirty)");
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
        current_branch: Arc::new(RwLock::new(branch.clone())),
        session_id: Arc::new(RwLock::new(session_id)),
        reindex_state: Arc::new(RwLock::new(ReindexState::Ready)),
        metrics: Arc::new(metrics::DaemonMetrics::new()),
    });

    // Step 11: Start file watcher
    let watcher_state = state.clone();
    match watcher::start_watcher(&project_root, watcher_state.clone()) {
        Ok(mut rx) => {
            tokio::spawn(async move {
                handle_watch_events(&watcher_state, &mut rx).await;
            });
            tracing::info!("file watcher started");
        }
        Err(e) => {
            tracing::warn!(error = %e, "file watcher failed to start, continuing without");
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
            tracing::error!(error = %e, "socket listener error");
        }
    });

    // Periodic metrics snapshot (every 60s)
    let snapshot_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let nodes = snapshot_state.graph.read().node_count();
            let edges = snapshot_state.graph.read().edge_count();
            let snapshot = snapshot_state.metrics.snapshot(nodes, edges);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let meta = snapshot_state.meta_db.lock();
            let json_str = serde_json::to_string(&snapshot).unwrap_or_default();
            let _ = meta.execute(
                "INSERT INTO metrics_snapshots (timestamp, json_blob) VALUES (?1, ?2)",
                rusqlite::params![now, json_str],
            );
            // Retain only last 24h of snapshots
            let _ = meta.execute(
                "DELETE FROM metrics_snapshots WHERE timestamp < ?1",
                rusqlite::params![now - 86400],
            );
        }
    });

    let nodes = state.graph.read().node_count();
    let edges = state.graph.read().edge_count();
    tracing::info!(
        branch = %branch,
        nodes,
        edges,
        pid = std::process::id(),
        "daemon ready"
    );

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

    tracing::info!("shutting down...");

    let _ = shutdown_tx.send(true);
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

    tracing::info!("stopped cleanly");
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
                    tracing::error!(error = %e, "branch switch error");
                }
            }
            WatchEvent::FilesChanged(files) => {
                if *state.reindex_state.read() == ReindexState::Switching {
                    continue;
                }
                *state.reindex_state.write() = ReindexState::Indexing;

                let reindex_start = std::time::Instant::now();
                let file_count = files.len();
                let mut cross_file_queue: HashSet<std::path::PathBuf> = HashSet::new();

                for file in &files {
                    let file_str = file.to_string_lossy().to_string();
                    match watcher::route_file(file) {
                        FileRoute::Code => {
                            let _span =
                                tracing::info_span!("reindex_file", file = %file_str).entered();

                            let prep = {
                                let db_guard = state.branch_db.lock();
                                let Some(ref conn) = *db_guard else { continue };
                                let graph = state.graph.read();
                                match index::incremental_reindex_prep(conn, &graph, &file_str) {
                                    Ok(p) => p,
                                    Err(e) => {
                                        tracing::warn!(file = %file_str, error = %e, "reindex prep failed");
                                        continue;
                                    }
                                }
                            };

                            {
                                let db_guard = state.branch_db.lock();
                                let Some(ref conn) = *db_guard else { continue };
                                let mut graph = state.graph.write();
                                match index::incremental_reindex_swap(conn, &mut graph, prep) {
                                    Ok(stats) => {
                                        tracing::info!(
                                            file = %file_str,
                                            nodes_removed = stats.nodes_removed,
                                            nodes_added = stats.nodes_added,
                                            edges_added = stats.edges_added,
                                            annotations_migrated = stats.annotations_migrated,
                                            "reindexed"
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!(file = %file_str, error = %e, "reindex swap failed");
                                    }
                                }

                                let affected = index::cross_file_affected(&graph, &file_str);
                                cross_file_queue.extend(affected);
                            }
                        }
                        FileRoute::Doc => {
                            if let Ok(content) = std::fs::read_to_string(file) {
                                let db_guard = state.branch_db.lock();
                                if let Some(ref conn) = *db_guard {
                                    if let Err(e) =
                                        doc_indexer::index_doc_file(conn, &file_str, &content)
                                    {
                                        tracing::warn!(file = %file_str, error = %e, "doc reindex failed");
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

                let duration_us = reindex_start.elapsed().as_micros() as u64;
                state.metrics.reindex_count.inc();
                state.metrics.reindex_duration_us.record(duration_us);
                tracing::info!(
                    files = file_count,
                    cross_file = cross_file_queue.len(),
                    duration_us,
                    "reindex batch complete"
                );
            }
        }
    }
}

/// Detect current git branch, falling back to "main".
/// Detached HEAD returns `HEAD_<first12chars>` per design doc §8.7.
pub fn detect_branch(project_root: &Path) -> String {
    let branch = std::process::Command::new("git")
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
        .unwrap_or_else(|| "main".to_string());

    if branch == "HEAD" {
        let hash = std::process::Command::new("git")
            .args(["rev-parse", "--short=12", "HEAD"])
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
            .unwrap_or_else(|| "unknown".to_string());
        format!("HEAD_{hash}")
    } else {
        branch
    }
}
