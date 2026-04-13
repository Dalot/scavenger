mod bridge;
mod capsule;
mod config;
mod daemon;
mod db;
mod graph;
mod hooks;
mod memory;
mod observe;
mod query;

use clap::{Parser, Subcommand};
use owo_colors::OwoColorize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "scavenger",
    version,
    about = "AST dependency graph and session memory engine for AI coding agents (Claude Code, Cursor)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize Scavenger on a project
    Init,

    /// Manage the daemon process
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },

    /// Manually re-index files
    Index {
        /// Path to re-index (defaults to project root)
        path: Option<PathBuf>,
    },

    /// Print a capsule to stdout
    Capsule {
        /// File to generate capsule for
        #[arg(help = "Source file to generate capsule for")]
        file: PathBuf,
        /// Symbol name within the file
        #[arg(help = "Specific symbol name within the file to focus on")]
        symbol: Option<String>,
        /// Query string for intent detection
        #[arg(
            long,
            help = "Query string for intent detection to prioritize relevant context"
        )]
        query: Option<String>,
        /// Token budget override
        #[arg(long, help = "Maximum token budget for the generated capsule")]
        budget: Option<u32>,
        /// Context depth: "minimal", "standard" (default), "detailed"
        #[arg(long, help = "Context depth level: minimal, standard, or detailed")]
        detail_level: Option<String>,
        /// Override max caller count
        #[arg(long, help = "Maximum number of caller functions to include")]
        max_callers: Option<u32>,
        /// Override max callee count
        #[arg(long, help = "Maximum number of callee functions to include")]
        max_callees: Option<u32>,
        /// Override max annotation count
        #[arg(long, help = "Maximum number of annotations to include")]
        max_annotations: Option<u32>,
        /// Include full function body if budget allows
        #[arg(long, help = "Include full function bodies when budget allows")]
        include_body: Option<bool>,
    },

    /// Query annotations
    Memory {
        /// Search query
        #[arg(long)]
        query: Option<String>,
        /// Max results
        #[arg(long, default_value = "20")]
        limit: u32,
    },

    /// Graph inspection commands
    Graph {
        #[command(subcommand)]
        command: GraphCommands,
    },

    /// Add an annotation to a symbol
    Annotate {
        /// Symbol name to annotate
        symbol: String,
        /// Annotation text
        text: String,
        /// Tags (comma-separated)
        #[arg(long)]
        tags: Option<String>,
    },

    /// Merge annotations from another branch
    MergeAnnotations {
        /// Source branch to merge from
        branch: String,
    },

    /// Run health diagnostics
    Doctor {
        /// Show verbose output
        #[arg(long)]
        verbose: bool,
        /// Output format
        #[arg(long, default_value = "human")]
        format: OutputFormat,
        /// Watch mode: re-check every N seconds
        #[arg(long)]
        watch: Option<u64>,
    },

    /// Show token savings and operational metrics
    Stats {
        /// Filter by session
        #[arg(long)]
        session: Option<String>,
        /// Filter by branch
        #[arg(long)]
        branch: Option<String>,
        /// Output as JSON (for programmatic consumption)
        #[arg(long)]
        json: bool,
    },

    /// Manage federated repositories
    Federate {
        #[command(subcommand)]
        command: FederateCommands,
    },

    /// Hook handlers (called by Claude Code / Cursor)
    Hook {
        #[command(subcommand)]
        command: HookCommands,
    },

    /// View session metrics collected by audit hooks
    Metrics {
        #[command(subcommand)]
        command: MetricsCommands,
    },

    /// Remove scavenger plugin and legacy configuration from this project
    Clean {
        /// Also remove the .scavenger/ directory and all indexed data
        #[arg(long)]
        purge: bool,
    },

    /// Inspect the database directly (no sqlite3 needed)
    Db {
        #[command(subcommand)]
        command: DbCommands,
    },

    /// Start the MCP bridge (stdio JSON-RPC server for Claude Code / Cursor)
    McpBridge,

    /// Live observability dashboard (TUI)
    Observe {
        /// Refresh interval in seconds
        #[arg(long, default_value = "2")]
        interval: u64,
    },

    /// View daemon logs with filtering
    Logs {
        /// Follow the log in real-time (like tail -f)
        #[arg(long, short)]
        follow: bool,

        /// Filter by minimum log level (trace, debug, info, warn, error)
        #[arg(long, default_value = "info")]
        level: String,

        /// Filter by span/method name (e.g. "capsule", "reindex", "hook_post")
        #[arg(long)]
        method: Option<String>,

        /// Number of recent lines to show (default: 50)
        #[arg(long, short, default_value = "50")]
        lines: usize,
    },

    /// Run evaluation suites to measure Scavenger's quality and performance
    Eval {
        /// Which eval suite to run: relevance, accuracy, performance, agent
        #[arg(
            long,
            help = "Specific eval suite to run: relevance, accuracy, performance, or agent"
        )]
        suite: Option<String>,

        /// Run all suites (default)
        #[arg(long, help = "Run all available eval suites")]
        all: bool,

        /// Which tier to run: component, agent, all
        #[arg(
            long,
            default_value = "component",
            help = "Evaluation tier: component, agent, or all"
        )]
        tier: String,

        /// Code to evaluate against — the project(s) that Scavenger will
        /// index and run evals on. Can point to a single project directory
        /// or a directory of projects. Defaults to eval/corpus/
        #[arg(
            long,
            help = "Path to evaluation corpus (project or directory of projects)"
        )]
        corpus: Option<String>,

        /// Run agent tasks matching this glob pattern
        #[arg(long, help = "Glob pattern to filter agent tasks to run")]
        tasks: Option<String>,

        /// Which AI agent to use for tier-2 evals: claude, cursor
        #[arg(long, help = "AI agent to use for tier-2 evals: claude or cursor")]
        agent: Option<String>,

        /// Output results as structured JSON
        #[arg(long, help = "Output evaluation results as structured JSON")]
        json: bool,

        /// Use a custom thresholds file instead of eval/thresholds.toml
        #[arg(
            long,
            help = "Path to custom thresholds file (default: eval/thresholds.toml)"
        )]
        thresholds: Option<String>,

        /// Run agent eval without Scavenger (baseline only)
        #[arg(long, help = "Run baseline evaluation without Scavenger enabled")]
        baseline: bool,

        /// Compare results against a previous eval run
        #[arg(long, help = "Path to previous eval results to compare against")]
        compare: Option<String>,

        /// Generate an HTML report from the last eval run
        #[arg(long, help = "Generate an HTML report from the last evaluation run")]
        report: bool,
    },
}

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

#[derive(Subcommand)]
enum GraphCommands {
    /// Show node/edge counts and centrality top-10
    Stats,
    /// Show ASCII neighborhood tree for a symbol
    Show {
        /// Symbol name to inspect
        symbol: String,
    },
}

#[derive(Subcommand)]
enum FederateCommands {
    /// Add a federated repository
    Add { path: PathBuf },
    /// Remove a federated repository
    Remove { path: PathBuf },
    /// List all federated repositories
    List,
    /// Verify all federated repositories
    Verify,
}

#[derive(Subcommand)]
enum HookCommands {
    /// Handle PreToolUse hook (Claude Code: injects capsule as additionalContext)
    PreToolUse,
    /// Handle PostToolUse hook (Claude Code: triggers re-index on edits)
    PostToolUse,
    /// Handle SessionStart hook (starts daemon, returns additional_context for Cursor)
    SessionStart,
    /// Handle SessionEnd hook (stops the daemon)
    SessionEnd,
    /// Handle afterFileEdit hook (Cursor: triggers re-index on edits)
    AfterFileEdit,
    /// Generic audit hook — logs metrics for any Cursor hook event
    Audit,
}

#[derive(Subcommand)]
enum MetricsCommands {
    /// List all sessions with metrics (auto-detects WITH/WITHOUT scavenger)
    List,
    /// Show detailed metrics for a session
    Show {
        /// Session/conversation ID (prefix match supported)
        session: String,
    },
    /// Compare two sessions side by side (with vs without scavenger)
    Compare {
        /// First session ID (prefix match supported)
        session_a: String,
        /// Second session ID (prefix match supported)
        session_b: String,
    },
    /// Label a session for easier identification
    Tag {
        /// Session/conversation ID (prefix match supported)
        session: String,
        /// Label (e.g. "baseline", "with-scavenger", "prompt-1-without")
        label: String,
    },
}

#[derive(Subcommand)]
enum DbCommands {
    /// Overview: node/edge/file/annotation counts, DB sizes, last indexed time
    Summary,
    /// List indexed AST symbols
    Nodes {
        /// Max rows to show
        #[arg(
            long,
            default_value = "30",
            help = "Maximum number of nodes to display"
        )]
        limit: u32,
    },
    /// List indexed source files
    Files {
        /// Max rows to show
        #[arg(
            long,
            default_value = "30",
            help = "Maximum number of files to display"
        )]
        limit: u32,
    },
    /// List annotations
    Annotations {
        /// Max rows to show
        #[arg(
            long,
            default_value = "30",
            help = "Maximum number of annotations to display"
        )]
        limit: u32,
    },
    /// Show recent token_log entries (from daemon_meta.db)
    Tokens {
        /// Max rows to show
        #[arg(
            long,
            default_value = "20",
            help = "Maximum number of token log entries to display"
        )]
        limit: u32,
    },
    /// Run a read-only SQL query against the branch DB
    Query {
        /// SQL statement (SELECT only)
        sql: String,
        /// Query against daemon_meta.db instead of the branch DB
        #[arg(long)]
        meta: bool,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum OutputFormat {
    Human,
    Json,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => cmd_init(),
        Commands::Daemon { command } => cmd_daemon(command),
        Commands::Index { path } => cmd_index(path),
        Commands::Capsule {
            file,
            symbol,
            query,
            budget,
            detail_level,
            max_callers,
            max_callees,
            max_annotations,
            include_body,
        } => cmd_capsule(
            file,
            symbol,
            query,
            budget,
            detail_level,
            max_callers,
            max_callees,
            max_annotations,
            include_body,
        ),
        Commands::Memory { query, limit } => cmd_memory(query, limit),
        Commands::Graph { command } => cmd_graph(command),
        Commands::Annotate { symbol, text, tags } => cmd_annotate(symbol, text, tags),
        Commands::MergeAnnotations { branch } => cmd_merge_annotations(branch),
        Commands::Doctor {
            verbose,
            format,
            watch,
        } => cmd_doctor(verbose, format, watch),
        Commands::Stats {
            session,
            branch,
            json,
        } => cmd_stats(session, branch, json),
        Commands::Federate { command } => cmd_federate(command),
        Commands::Hook { command } => cmd_hook(command),
        Commands::Metrics { command } => cmd_metrics(command),
        Commands::Clean { purge } => cmd_clean(purge),
        Commands::Db { command } => cmd_db(command),
        Commands::McpBridge => cmd_mcp_bridge(),
        Commands::Observe { interval } => cmd_observe(interval),
        Commands::Logs {
            follow,
            level,
            method,
            lines,
        } => cmd_logs(follow, level, method, lines),
        Commands::Eval {
            suite,
            all,
            tier,
            corpus,
            tasks,
            agent,
            json,
            thresholds,
            baseline,
            compare,
            report,
        } => cmd_eval(
            suite, all, tier, corpus, tasks, agent, json, thresholds, baseline, compare, report,
        ),
    };

    if let Err(e) = result {
        eprintln!("{} {e}", "Error:".red().bold());
        std::process::exit(1);
    }
}

fn cmd_init() -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);

    // Step 1: mkdir .scavenger/ with mode 0700
    if scavenger_dir.exists() {
        eprintln!(
            "{} .scavenger/ already exists. Re-initializing...",
            "Warning:".yellow().bold()
        );
    }
    std::fs::create_dir_all(&scavenger_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&scavenger_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    std::fs::create_dir_all(scavenger_dir.join("indexes"))?;

    let cfg = config::Config::load(&project_root)?;
    eprintln!("{}", "Scavenger: initializing...".bold());

    // Step 2: Detect branch and open DB
    let branch = daemon::detect_branch(&project_root);
    eprintln!("  Branch: {}", branch.cyan());
    let conn = db::open_branch_db(&scavenger_dir, &branch)?;
    let _meta_conn = db::open_daemon_meta_db(&scavenger_dir)?;

    // Step 3: Bulk index all source files
    let source_files = graph::index::collect_source_files(&project_root);
    eprintln!(
        "  Indexing {} source files...",
        source_files.len().to_string().cyan()
    );
    let mut graph_state = graph::GraphState::new();
    let stats = graph::index::bulk_index(&conn, &mut graph_state, &source_files)?;
    eprintln!(
        "  Indexed: {} files, {} symbols, {} edges",
        stats.files_indexed.to_string().green(),
        stats.symbols_extracted.to_string().green(),
        stats.edges_created.to_string().green(),
    );

    // Step 4: Index doc files
    let doc_files =
        graph::doc_indexer::collect_doc_files(&project_root, &cfg.docs.patterns, &cfg.docs.exclude);
    if !doc_files.is_empty() {
        eprintln!(
            "  Indexing {} doc files...",
            doc_files.len().to_string().cyan()
        );
        let mut doc_chunks = 0u32;
        for doc_path in &doc_files {
            if let Ok(content) = std::fs::read_to_string(doc_path) {
                let rel = doc_path.to_string_lossy().to_string();
                if let Ok(count) = graph::doc_indexer::index_doc_file(&conn, &rel, &content) {
                    doc_chunks += count;
                }
            }
        }
        eprintln!("  Doc chunks: {}", doc_chunks.to_string().green());
    }

    // Step 5: Create Claude Code plugin
    eprintln!("  Creating Claude Code plugin...");
    hooks::register::create_plugin(&project_root)?;

    // Step 5b: Register MCP bridge (any available agent CLI)
    eprintln!("  Registering MCP bridge (agent CLI)...");
    let _cli_ok = hooks::register::register_mcp_via_cli(&project_root)?;
    if let Some(agent) = hooks::register::detect_available_agent() {
        eprintln!("    Registered via `{} mcp add`", agent.cli_name());
    } else {
        eprintln!("    No agent CLI found — register manually with:");
        eprintln!("    claude mcp add scavenger -- scavenger mcp-bridge");
    }
    hooks::register::register_mcp_server(&project_root)?;

    // Step 5c: Clean up legacy settings.local.json entries from older versions
    if let Err(e) = hooks::register::remove_legacy_settings(&project_root) {
        eprintln!(
            "  {} Could not clean legacy settings: {e}",
            "Warning:".yellow().bold()
        );
    }

    // Step 5d: Register in .mcp.json for other MCP-compatible tools
    eprintln!("  Registering MCP bridge (.mcp.json)...");
    hooks::register::register_mcp_in_mcp_json(&project_root)?;

    // Step 5e: Create Cursor IDE config
    eprintln!("  Registering MCP bridge (Cursor)...");
    hooks::register::create_cursor_mcp_config(&project_root)?;
    eprintln!("  Creating Cursor hooks...");
    hooks::register::create_cursor_hooks(&project_root)?;

    // Step 6: Append .scavenger/ to .gitignore
    append_to_gitignore(&project_root)?;

    eprintln!("\n{}", "Done!".green().bold());
    eprintln!(
        "\n  {} claude --plugin-dir .scavenger/claude-plugin/",
        "Claude Code:".cyan().bold(),
    );
    eprintln!(
        "  {} MCP tools + hooks registered in .cursor/ — reload the Cursor window to activate.",
        "Cursor:".cyan().bold(),
    );
    eprintln!(
        "  {} MCP bridge registered in .mcp.json",
        "Other tools:".cyan().bold(),
    );
    eprintln!("\nThe daemon starts and stops automatically with each session.");
    eprintln!("Manual control: scavenger daemon {{start|stop|restart|status}}");
    Ok(())
}

fn cmd_daemon(command: DaemonCommands) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        DaemonCommands::Start => cmd_daemon_start(),
        DaemonCommands::Stop => cmd_daemon_stop(),
        DaemonCommands::Restart => {
            let _ = cmd_daemon_stop();
            cmd_daemon_start()
        }
        DaemonCommands::Status => cmd_daemon_status(),
    }
}

fn cmd_daemon_start() -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    if !scavenger_dir.exists() {
        return Err("Not initialized. Run `scavenger init` first.".into());
    }
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(daemon::run_daemon(project_root))
}

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
                let edges = status
                    .get("edge_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                println!("  Graph:         {nodes} nodes, {edges} edges");
            }
        }
    }

    Ok(())
}

fn cmd_index(path: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = path.unwrap_or_else(|| std::env::current_dir().unwrap());
    let scavenger_dir = db::scavenger_dir(&project_root);
    let branch = daemon::detect_branch(&project_root);
    let conn = db::open_branch_db(&scavenger_dir, &branch)?;

    let source_files = graph::index::collect_source_files(&project_root);
    eprintln!(
        "Re-indexing {} files on branch {branch}...",
        source_files.len()
    );

    let mut g = graph::GraphState::new();
    g.load_from_db(&conn)?;
    let stats = graph::index::bulk_index(&conn, &mut g, &source_files)?;

    eprintln!(
        "Indexed: {} files, {} symbols, {} edges",
        stats.files_indexed, stats.symbols_extracted, stats.edges_created
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_capsule(
    file: PathBuf,
    symbol: Option<String>,
    query_str: Option<String>,
    budget: Option<u32>,
    detail_level: Option<String>,
    max_callers: Option<u32>,
    max_callees: Option<u32>,
    max_annotations: Option<u32>,
    include_body: Option<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    let cfg = config::Config::load(&project_root)?;
    let branch = daemon::detect_branch(&project_root);
    let conn = db::open_branch_db(&scavenger_dir, &branch)?;

    let mut g = graph::GraphState::new();
    g.load_from_db(&conn)?;
    g.compute_pagerank(0.85, 30);

    let file_str = file.to_string_lossy().to_string();
    let level = detail_level
        .as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or_default();
    let mut constraints = capsule::budget::CapsuleConstraints::from_detail(level);
    if let Some(v) = max_callers {
        constraints.max_callers = v;
    }
    if let Some(v) = max_callees {
        constraints.max_callees = v;
    }
    if let Some(v) = max_annotations {
        constraints.max_annotations = v;
    }
    if let Some(v) = include_body {
        constraints.include_body = v;
    }
    let qr = query::run_query(
        &conn,
        &g,
        &cfg,
        &file_str,
        symbol.as_deref(),
        query_str.as_deref(),
        &constraints,
    );
    let result = capsule::assemble(&conn, &g, &cfg, &qr, budget, &constraints);

    println!("{}", result.text);
    println!(
        "({} tokens, {} items)",
        result.token_count, result.items_included
    );
    Ok(())
}

fn cmd_memory(query_str: Option<String>, limit: u32) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    let branch = daemon::detect_branch(&project_root);
    let conn = db::open_branch_db(&scavenger_dir, &branch)?;

    if let Some(q) = query_str {
        let matches = db::queries::search_annotations_fts(&conn, &q, limit)?;
        for m in &matches {
            println!("  {} (rank: {:.3})", m.id, m.rank);
        }
        eprintln!("{} results", matches.len());
    } else {
        eprintln!("Use --query to search annotations");
    }
    Ok(())
}

fn cmd_graph(command: GraphCommands) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    let branch = daemon::detect_branch(&project_root);
    let conn = db::open_branch_db(&scavenger_dir, &branch)?;

    let mut g = graph::GraphState::new();
    g.load_from_db(&conn)?;
    g.compute_pagerank(0.85, 30);

    match command {
        GraphCommands::Stats => {
            println!("Nodes: {}", g.node_count());
            println!("Edges: {}", g.edge_count());
            println!("\nTop 10 by centrality:");
            let mut nodes: Vec<_> = g
                .graph
                .node_indices()
                .filter_map(|idx| g.graph.node_weight(idx))
                .collect();
            nodes.sort_by(|a, b| {
                b.centrality
                    .partial_cmp(&a.centrality)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            for (i, w) in nodes.iter().take(10).enumerate() {
                println!("  {}. {} ({}) — {:.4}", i + 1, w.name, w.kind, w.centrality);
            }
        }
        GraphCommands::Show { symbol } => {
            let found = g
                .graph
                .node_indices()
                .find(|&idx| g.graph.node_weight(idx).is_some_and(|w| w.name == symbol));
            if let Some(idx) = found {
                let w = g.graph.node_weight(idx).unwrap();
                println!("{} ({})", w.name.bold(), w.kind);
                println!(
                    "  File: {}:{}-{}",
                    w.file_path.display(),
                    w.line_start,
                    w.line_end
                );
                println!("  Signature: {}", w.signature);
                println!("  Centrality: {:.4}", w.centrality);

                let callers = g.callers_of(&w.id);
                if !callers.is_empty() {
                    println!("\n  Callers ({}):", callers.len());
                    for c in callers.iter().take(10) {
                        println!("    ← {} ({})", c.name, c.file_path.display());
                    }
                }

                let callees = g.callees_of(&w.id);
                if !callees.is_empty() {
                    println!("\n  Callees ({}):", callees.len());
                    for c in callees.iter().take(10) {
                        println!("    → {} ({})", c.name, c.file_path.display());
                    }
                }
            } else {
                eprintln!("Symbol '{}' not found", symbol);
            }
        }
    }
    Ok(())
}

fn cmd_annotate(
    symbol: String,
    text: String,
    tags: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    let branch = daemon::detect_branch(&project_root);
    let conn = db::open_branch_db(&scavenger_dir, &branch)?;

    let id = uuid::Uuid::new_v4().to_string();

    // Try to resolve symbol via FTS5
    let anchor_value = match db::queries::search_nodes_fts(&conn, &symbol, 1) {
        Ok(matches) if !matches.is_empty() => matches[0].id.clone(),
        _ => symbol.clone(),
    };

    memory::annotations::upsert_annotation(
        &conn,
        &id,
        Some(memory::annotations::AnchorType::Node),
        Some(&anchor_value),
        &text,
        tags.as_deref(),
        memory::annotations::AnnotationKind::Fact,
    )?;

    println!("Annotation {} created (anchored to {})", id, anchor_value);
    Ok(())
}

fn cmd_merge_annotations(branch: String) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    let current_branch = daemon::detect_branch(&project_root);

    let source_conn = db::open_branch_db(&scavenger_dir, &branch)?;
    let target_conn = db::open_branch_db(&scavenger_dir, &current_branch)?;

    let result = memory::MemoryManager::merge_annotations(&source_conn, &target_conn)?;
    println!(
        "Merged from {branch}: {} imported, {} deduped",
        result.imported, result.deduped
    );
    Ok(())
}

fn cmd_doctor(
    verbose: bool,
    format: OutputFormat,
    watch: Option<u64>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(interval) = watch {
        loop {
            // Clear screen
            print!("\x1B[2J\x1B[H");
            run_doctor_once(verbose, &format)?;
            std::thread::sleep(std::time::Duration::from_secs(interval));
        }
    } else {
        run_doctor_once(verbose, &format)
    }
}

fn run_doctor_once(verbose: bool, format: &OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    let no_color = std::env::var("NO_COLOR").is_ok();

    let mut checks: Vec<DiagCheck> = Vec::new();
    let mut recommendations: Vec<String> = Vec::new();

    // Process checks
    let pid_path = scavenger_dir.join("daemon.pid");
    let pid_alive = if pid_path.exists() {
        let pid_str = std::fs::read_to_string(&pid_path).unwrap_or_default();
        let pid: i32 = pid_str.trim().parse().unwrap_or(0);
        pid > 0 && std::path::Path::new(&format!("/proc/{pid}")).exists()
    } else {
        false
    };
    checks.push(DiagCheck::new("Daemon process", "Process", pid_alive));
    if !pid_alive {
        recommendations
            .push("Start the daemon: scavenger daemon start (or trigger via session hook)".into());
    }
    checks.push(DiagCheck::new("PID file", "Process", pid_path.exists()));

    let sock = scavenger_dir.join("daemon.sock");
    checks.push(DiagCheck::new(
        "Socket accessible",
        "Process",
        sock.exists(),
    ));
    if pid_alive && !sock.exists() {
        recommendations.push(
            "Socket missing despite running daemon — restart: scavenger daemon restart".into(),
        );
    }

    // Config check
    let config_ok = config::Config::load(&project_root).is_ok();
    checks.push(DiagCheck::new("Config valid", "Config", config_ok));
    if !config_ok {
        recommendations.push("Fix or regenerate scavenger.toml: scavenger init".into());
    }

    // DB integrity
    let branch = daemon::detect_branch(&project_root);
    let db_ok = if let Ok(conn) = db::open_branch_db(&scavenger_dir, &branch) {
        conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .is_ok_and(|r| r == "ok")
    } else {
        false
    };
    checks.push(DiagCheck::new("DB integrity", "FileIntegrity", db_ok));
    if !db_ok {
        recommendations.push("Database may be corrupt — re-index: scavenger index".into());
    }

    // Plugin check
    let plugin = hooks::register::plugin_dir(&project_root);
    let hooks_ok = plugin.join("hooks/hooks.json").exists();
    checks.push(DiagCheck::new("Plugin hooks", "Dependencies", hooks_ok));
    if !hooks_ok {
        recommendations.push("Plugin hooks missing — run: scavenger init".into());
    }

    // .scavenger dir exists
    checks.push(DiagCheck::new(
        "Initialized",
        "FileIntegrity",
        scavenger_dir.exists(),
    ));

    // Branch DB exists
    let sanitized = branch.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
    let branch_db = scavenger_dir
        .join("indexes")
        .join(format!("{sanitized}.db"));
    checks.push(DiagCheck::new(
        "Branch DB exists",
        "FileIntegrity",
        branch_db.exists(),
    ));

    // Log analysis (parse recent daemon logs for error patterns)
    let mut log_errors = 0u32;
    let mut log_warnings = 0u32;
    let mut empty_capsules = 0u32;
    let mut total_capsules = 0u32;

    let mut log_files: Vec<_> = std::fs::read_dir(&scavenger_dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("daemon.log"))
        .collect();
    log_files.sort_by_key(|e| std::cmp::Reverse(e.metadata().ok().and_then(|m| m.modified().ok())));

    if let Some(log_file) = log_files.first() {
        let content = std::fs::read_to_string(log_file.path()).unwrap_or_default();
        for line in content.lines().rev().take(500) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) {
                let level = parsed.get("level").and_then(|v| v.as_str()).unwrap_or("");
                match level {
                    "ERROR" => log_errors += 1,
                    "WARN" => log_warnings += 1,
                    _ => {}
                }
                if let Some(fields) = parsed.get("fields").and_then(|v| v.as_object())
                    && fields.get("message").and_then(|v| v.as_str()) == Some("capsule served")
                {
                    total_capsules += 1;
                    if fields.get("empty").and_then(|v| v.as_bool()) == Some(true) {
                        empty_capsules += 1;
                    }
                }
            }
        }
    }

    let log_health = log_errors == 0;
    checks.push(DiagCheck::new(
        "No recent errors in logs",
        "Logs",
        log_health,
    ));
    if log_errors > 0 {
        recommendations.push(format!(
            "{log_errors} errors in recent logs — run: scavenger logs --level error"
        ));
    }

    let empty_rate_ok =
        total_capsules == 0 || (empty_capsules as f64 / total_capsules as f64) < 0.5;
    checks.push(DiagCheck::new(
        "Empty capsule rate < 50%",
        "Effectiveness",
        empty_rate_ok,
    ));
    if !empty_rate_ok {
        recommendations.push(format!(
            "High empty capsule rate ({empty_capsules}/{total_capsules}) — ensure files are indexed: scavenger index"
        ));
    }

    // Daemon metrics (if available)
    let daemon_metrics = fetch_daemon_metrics(&scavenger_dir);
    if let Some(ref dm) = daemon_metrics {
        let p99 = dm
            .pointer("/capsule/latency_us/p99")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let latency_ok = p99 < 5_000_000;
        checks.push(DiagCheck::new(
            "Capsule P99 < 5s",
            "Performance",
            latency_ok,
        ));
        if !latency_ok {
            recommendations.push(format!(
                "Capsule P99 latency is {p99}us — check graph size and DB performance"
            ));
        }
    }

    // Composite health score (0-100)
    let passed = checks.iter().filter(|c| c.passed).count();
    let total = checks.len();
    let health_score = (passed as f64 / total as f64 * 100.0).round() as u32;

    match format {
        OutputFormat::Json => {
            let check_results: Vec<serde_json::Value> = checks
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "name": c.name,
                        "category": c.category,
                        "passed": c.passed,
                    })
                })
                .collect();
            let result = serde_json::json!({
                "health_score": health_score,
                "checks": check_results,
                "recommendations": recommendations,
                "log_analysis": {
                    "errors": log_errors,
                    "warnings": log_warnings,
                    "total_capsules": total_capsules,
                    "empty_capsules": empty_capsules,
                },
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        OutputFormat::Human => {
            let score_str = format!("{health_score}/100");
            if health_score >= 80 {
                println!("Scavenger Health: {}\n", score_str.green().bold());
            } else if health_score >= 50 {
                println!("Scavenger Health: {}\n", score_str.yellow().bold());
            } else {
                println!("Scavenger Health: {}\n", score_str.red().bold());
            }

            for c in &checks {
                let icon = if c.passed {
                    if no_color { "[OK]" } else { "\u{2714}" }
                } else if no_color {
                    "[FAIL]"
                } else {
                    "\u{2718}"
                };
                if no_color {
                    println!("  {icon} {}", c.name);
                } else if c.passed {
                    println!("  {} {}", icon.green(), c.name);
                } else {
                    println!("  {} {}", icon.red(), c.name);
                }
            }

            println!("\n{passed}/{total} checks passed");

            if verbose {
                println!();
                println!("{}", "Log Analysis (last 500 entries)".bold());
                println!("{}", "─".repeat(40));
                println!("  Errors:    {log_errors}");
                println!("  Warnings:  {log_warnings}");
                if total_capsules > 0 {
                    let erate = empty_capsules as f64 / total_capsules as f64 * 100.0;
                    println!("  Capsules:  {total_capsules} ({empty_capsules} empty, {erate:.0}%)");
                }
            }

            if !recommendations.is_empty() {
                println!();
                println!("{}", "Recommendations".bold());
                println!("{}", "─".repeat(40));
                for r in &recommendations {
                    println!("  - {r}");
                }
            }
        }
    }

    let failed = checks.iter().filter(|c| !c.passed).count();
    if failed > 0 {
        std::process::exit(if failed == checks.len() { 2 } else { 1 });
    }
    Ok(())
}

struct DiagCheck {
    name: &'static str,
    category: &'static str,
    passed: bool,
}

impl DiagCheck {
    fn new(name: &'static str, category: &'static str, passed: bool) -> Self {
        Self {
            name,
            category,
            passed,
        }
    }
}

fn cmd_stats(
    session_filter: Option<String>,
    branch_filter: Option<String>,
    json_output: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    let meta_conn = db::open_daemon_meta_db(&scavenger_dir)?;

    let mut where_clause = String::from("WHERE 1=1");
    if let Some(ref s) = session_filter {
        where_clause.push_str(&format!(" AND session_id = '{s}'"));
    }
    if let Some(ref b) = branch_filter {
        where_clause.push_str(&format!(" AND branch = '{b}'"));
    }

    let total_calls: i64 = meta_conn
        .query_row(
            &format!("SELECT COUNT(*) FROM token_log {where_clause}"),
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let total_actual: i64 = meta_conn
        .query_row(
            &format!("SELECT COALESCE(SUM(tokens_actual), 0) FROM token_log {where_clause}"),
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let total_estimated: i64 = meta_conn
        .query_row(
            &format!("SELECT COALESCE(SUM(tokens_estimated), 0) FROM token_log {where_clause}"),
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let savings = if total_estimated > 0 {
        ((total_estimated - total_actual) as f64 / total_estimated as f64) * 100.0
    } else {
        0.0
    };

    // Fetch live daemon metrics if available
    let daemon_metrics = fetch_daemon_metrics(&scavenger_dir);

    if json_output {
        let mut result = serde_json::json!({
            "token_savings": {
                "total_calls": total_calls,
                "tokens_actual": total_actual,
                "tokens_estimated": total_estimated,
                "savings_pct": (savings * 10.0).round() / 10.0,
                "tokens_saved": total_estimated - total_actual,
            },
        });
        if let Some(dm) = daemon_metrics {
            result["daemon"] = dm;
        }
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!("{}", "Token Savings".bold());
    println!("{}", "─".repeat(50));
    println!("  Capsule calls:      {total_calls}");
    if total_calls > 0 {
        let avg_actual = total_actual / total_calls;
        println!("  Tokens served:      {total_actual}  (avg {avg_actual}/call)");
    } else {
        println!("  Tokens served:      {total_actual}");
    }
    println!("  Tokens without idx: {total_estimated}");
    println!("  Savings:            {savings:.1}%");
    if total_estimated > 0 {
        println!("  Tokens saved:       {}", total_estimated - total_actual);
    }

    if let Some(dm) = daemon_metrics {
        println!();
        println!("{}", "Performance (live daemon)".bold());
        println!("{}", "─".repeat(50));
        if let Some(cap) = dm.get("capsule") {
            let lat = cap.get("latency_us").unwrap_or(&serde_json::Value::Null);
            let p50 = lat.get("p50").and_then(|v| v.as_u64()).unwrap_or(0);
            let p95 = lat.get("p95").and_then(|v| v.as_u64()).unwrap_or(0);
            let p99 = lat.get("p99").and_then(|v| v.as_u64()).unwrap_or(0);
            println!(
                "  Capsule latency:    P50={}us  P95={}us  P99={}us",
                p50, p95, p99
            );

            let empty = cap.get("empty").and_then(|v| v.as_u64()).unwrap_or(0);
            let total = cap.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            if total > 0 {
                let empty_pct = empty as f64 / total as f64 * 100.0;
                println!("  Empty capsules:     {empty} / {total} ({empty_pct:.1}%)");
            }

            let budget = cap
                .get("budget_utilization_pct")
                .unwrap_or(&serde_json::Value::Null);
            let avg_util = budget.get("avg").and_then(|v| v.as_u64()).unwrap_or(0);
            println!("  Budget utilization: {avg_util}% avg");
        }

        if let Some(reindex) = dm.get("reindex") {
            let count = reindex.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            let lat = reindex
                .get("latency_us")
                .unwrap_or(&serde_json::Value::Null);
            let p50 = lat.get("p50").and_then(|v| v.as_u64()).unwrap_or(0);
            if count > 0 {
                println!("  Reindex:            {count} events (P50={p50}us)");
            }
        }

        if let Some(graph) = dm.get("graph") {
            let nodes = graph.get("nodes").and_then(|v| v.as_u64()).unwrap_or(0);
            let edges = graph.get("edges").and_then(|v| v.as_u64()).unwrap_or(0);
            println!();
            println!("{}", "Graph".bold());
            println!("{}", "─".repeat(50));
            println!("  Nodes: {nodes}  Edges: {edges}");
        }

        let uptime = dm.get("uptime_secs").and_then(|v| v.as_u64()).unwrap_or(0);
        let errors = dm.get("errors").and_then(|v| v.as_u64()).unwrap_or(0);
        println!();
        println!("{}", "Daemon".bold());
        println!("{}", "─".repeat(50));
        println!("  Uptime: {}s  Errors: {errors}", uptime);
    } else {
        println!();
        eprintln!("  (daemon not running — operational metrics unavailable)");
    }

    Ok(())
}

fn fetch_daemon_metrics(scavenger_dir: &std::path::Path) -> Option<serde_json::Value> {
    let socket_path = scavenger_dir.join("daemon.sock");
    if !socket_path.exists() {
        return None;
    }
    let rt = tokio::runtime::Runtime::new().ok()?;
    let request = serde_json::json!({ "method": "metrics" });
    rt.block_on(daemon::socket::send_request(&socket_path, &request))
        .ok()
}

fn cmd_federate(command: FederateCommands) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    let meta_conn = db::open_daemon_meta_db(&scavenger_dir)?;

    // Ensure federated_repos table exists in daemon_meta
    meta_conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS federated_repos (
            path TEXT PRIMARY KEY,
            added_at INTEGER NOT NULL,
            last_seen INTEGER
        )",
    )?;

    match command {
        FederateCommands::Add { path } => {
            let abs_path = std::fs::canonicalize(&path)?;
            if !abs_path.join(".scavenger").exists() {
                return Err(format!(
                    "No .scavenger/ directory in {}. Run `scavenger init` there first.",
                    abs_path.display()
                )
                .into());
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs() as i64;
            meta_conn.execute(
                "INSERT OR REPLACE INTO federated_repos (path, added_at, last_seen) VALUES (?1, ?2, ?2)",
                rusqlite::params![abs_path.to_string_lossy().to_string(), now],
            )?;
            println!("Added federated repo: {}", abs_path.display());
        }
        FederateCommands::Remove { path } => {
            let abs_path = std::fs::canonicalize(&path).unwrap_or(path);
            meta_conn.execute(
                "DELETE FROM federated_repos WHERE path = ?1",
                rusqlite::params![abs_path.to_string_lossy().to_string()],
            )?;
            println!("Removed federated repo: {}", abs_path.display());
        }
        FederateCommands::List => {
            let mut stmt =
                meta_conn.prepare("SELECT path, added_at, last_seen FROM federated_repos")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            })?;
            println!("Federated Repositories:");
            for row in rows {
                let (path, added, last_seen) = row?;
                println!(
                    "  {} (added: {}, last_seen: {})",
                    path,
                    added,
                    last_seen
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "never".to_string())
                );
            }
        }
        FederateCommands::Verify => {
            let mut stmt = meta_conn.prepare("SELECT path FROM federated_repos")?;
            let paths: Vec<String> = stmt
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;

            for path in &paths {
                let p = std::path::Path::new(path);
                let accessible = p.join(".scavenger").exists();
                let icon = if accessible { "\u{2714}" } else { "\u{2718}" };
                println!("  {icon} {path}");
            }
        }
    }
    Ok(())
}

fn cmd_hook(command: HookCommands) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);

    if !scavenger_dir.exists() {
        eprintln!(
            "scavenger hook: .scavenger/ not found in {} (cwd mismatch?)",
            project_root.display()
        );
        println!("{{}}");
        return Ok(());
    }

    match command {
        HookCommands::PreToolUse => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(hooks::handle_pre_tool_use(&scavenger_dir))?;
        }
        HookCommands::PostToolUse => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(hooks::handle_post_tool_use(&scavenger_dir))?;
        }
        HookCommands::AfterFileEdit => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(hooks::handle_after_file_edit(&scavenger_dir))?;
        }
        HookCommands::Audit => {
            hooks::metrics::handle_audit(&scavenger_dir)?;
        }
        HookCommands::SessionStart => {
            if !scavenger_dir.exists() {
                return Ok(());
            }
            let pid_path = scavenger_dir.join("daemon.pid");
            if !is_daemon_running(&pid_path) {
                let exe = std::env::current_exe().unwrap_or_else(|_| "scavenger".into());
                std::process::Command::new(exe)
                    .args(["daemon", "start"])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()?;
            }
            hooks::handle_session_start_with_context();
        }
        HookCommands::SessionEnd => {
            let pid_path = scavenger_dir.join("daemon.pid");
            if let Some(pid) = read_pid(&pid_path) {
                kill_daemon_and_wait(pid, &scavenger_dir);
            }
        }
    }
    Ok(())
}

fn read_pid(pid_path: &std::path::Path) -> Option<i32> {
    std::fs::read_to_string(pid_path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .filter(|&pid| pid > 0)
}

fn is_daemon_running(pid_path: &std::path::Path) -> bool {
    read_pid(pid_path).is_some_and(|pid| std::path::Path::new(&format!("/proc/{pid}")).exists())
}

/// Send SIGTERM, wait up to 5s for exit, then SIGKILL if still alive.
/// Cleans up PID file, socket, and lock on success.
fn kill_daemon_and_wait(pid: i32, scavenger_dir: &std::path::Path) {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }

        let proc_path = format!("/proc/{pid}");
        for _ in 0..50 {
            if !std::path::Path::new(&proc_path).exists() {
                cleanup_daemon_files(scavenger_dir);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        eprintln!("Daemon PID {pid} did not exit after SIGTERM, sending SIGKILL...");
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }

        for _ in 0..20 {
            if !std::path::Path::new(&proc_path).exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        cleanup_daemon_files(scavenger_dir);
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, scavenger_dir);
    }
}

fn cleanup_daemon_files(scavenger_dir: &std::path::Path) {
    let _ = std::fs::remove_file(scavenger_dir.join("daemon.pid"));
    let _ = std::fs::remove_file(scavenger_dir.join("daemon.sock"));
    let _ = std::fs::remove_file(scavenger_dir.join("daemon.lock"));
}

fn cmd_metrics(command: MetricsCommands) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);

    match command {
        MetricsCommands::List => {
            let sessions = hooks::metrics::list_sessions(&scavenger_dir);
            if sessions.is_empty() {
                eprintln!("No metrics sessions found.");
                eprintln!(
                    "Metrics are collected automatically via audit hooks (Claude Code plugin or Cursor hooks)."
                );
                eprintln!(
                    "Start a session and the audit hooks will log tool calls to .scavenger/metrics/"
                );
                return Ok(());
            }
            print!("{}", hooks::metrics::format_list(&scavenger_dir, &sessions));
        }
        MetricsCommands::Show { session } => {
            let id = resolve_session_id(&scavenger_dir, &session)?;
            match hooks::metrics::analyze_session(&scavenger_dir, &id) {
                Some(s) => print!("{}", hooks::metrics::format_summary(&s)),
                None => return Err(format!("No metrics found for session {id}").into()),
            }
        }
        MetricsCommands::Compare {
            session_a,
            session_b,
        } => {
            let id_a = resolve_session_id(&scavenger_dir, &session_a)?;
            let id_b = resolve_session_id(&scavenger_dir, &session_b)?;
            let a = hooks::metrics::analyze_session(&scavenger_dir, &id_a)
                .ok_or_else(|| format!("No metrics found for session {id_a}"))?;
            let b = hooks::metrics::analyze_session(&scavenger_dir, &id_b)
                .ok_or_else(|| format!("No metrics found for session {id_b}"))?;
            print!("{}", hooks::metrics::format_comparison(&a, &b));
        }
        MetricsCommands::Tag { session, label } => {
            let id = resolve_session_id(&scavenger_dir, &session)?;
            hooks::metrics::tag_session(&scavenger_dir, &id, &label)?;
            eprintln!("Tagged session {} as \"{}\"", &id[..8.min(id.len())], label);
        }
    }
    Ok(())
}

/// Resolve a session ID prefix to the full ID. Prefers exact matches.
fn resolve_session_id(
    scavenger_dir: &std::path::Path,
    prefix: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let sessions = hooks::metrics::list_sessions(scavenger_dir);

    if let Some(exact) = sessions.iter().find(|id| *id == prefix) {
        return Ok(exact.clone());
    }

    let matches: Vec<_> = sessions
        .iter()
        .filter(|id| id.starts_with(prefix))
        .collect();
    match matches.len() {
        0 => Err(format!("No session found matching prefix '{prefix}'").into()),
        1 => Ok(matches[0].clone()),
        _ => {
            eprintln!("Multiple sessions match prefix '{prefix}':");
            for id in &matches {
                eprintln!("  {id}");
            }
            Err("Provide a longer prefix to disambiguate".into())
        }
    }
}

fn cmd_clean(purge: bool) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let plugin = hooks::register::plugin_dir(&project_root);
    let scavenger_dir = db::scavenger_dir(&project_root);

    let mut removed = Vec::new();

    let pid_path = scavenger_dir.join("daemon.pid");
    if let Some(pid) = read_pid(&pid_path) {
        kill_daemon_and_wait(pid, &scavenger_dir);
        removed.push("stopped running daemon");
    }

    if plugin.exists() {
        std::fs::remove_dir_all(&plugin)?;
        removed.push("Claude Code plugin (.scavenger/claude-plugin/)");
    }

    if hooks::register::remove_mcp_via_cli(&project_root).unwrap_or(false) {
        removed.push("Claude Code MCP registration (via claude mcp remove)");
    }

    if let Ok(()) = hooks::register::remove_mcp_from_mcp_json(&project_root) {
        removed.push(".mcp.json scavenger entry");
    }

    if let Ok(()) = hooks::register::remove_cursor_config(&project_root) {
        removed.push("Cursor config (.cursor/mcp.json + hooks.json entries)");
    }

    if purge && scavenger_dir.exists() {
        std::fs::remove_dir_all(&scavenger_dir)?;
        removed.push(".scavenger/ directory (all indexed data)");
    }

    if removed.is_empty() {
        eprintln!("Nothing to clean.");
    } else {
        eprintln!("{}", "Cleaned:".green().bold());
        for item in &removed {
            eprintln!("  - {item}");
        }
    }

    Ok(())
}

fn cmd_db(command: DbCommands) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    if !scavenger_dir.exists() {
        return Err("Not initialized. Run `scavenger init` first.".into());
    }
    let branch = daemon::detect_branch(&project_root);

    match command {
        DbCommands::Summary => {
            let conn = db::open_branch_db(&scavenger_dir, &branch)?;
            let meta_conn = db::open_daemon_meta_db(&scavenger_dir)?;

            let node_count: i64 = conn.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get(0))?;
            let edge_count: i64 = conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
            let file_count: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
            let annotation_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM annotations", [], |r| r.get(0))?;
            let signal_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM behavioral_signals", [], |r| r.get(0))?;
            let doc_chunk_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM doc_chunks", [], |r| r.get(0))?;
            let session_event_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM session_log", [], |r| r.get(0))?;
            let token_log_count: i64 = meta_conn
                .query_row("SELECT COUNT(*) FROM token_log", [], |r| r.get(0))
                .unwrap_or(0);
            let last_indexed: Option<i64> = conn
                .query_row("SELECT MAX(last_indexed) FROM files", [], |r| r.get(0))
                .unwrap_or(None);

            let sanitized = branch.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
            let branch_db_path = scavenger_dir
                .join("indexes")
                .join(format!("{sanitized}.db"));
            let meta_db_path = scavenger_dir.join("daemon_meta.db");
            let branch_db_size = std::fs::metadata(&branch_db_path)
                .map(|m| m.len())
                .unwrap_or(0);
            let meta_db_size = std::fs::metadata(&meta_db_path)
                .map(|m| m.len())
                .unwrap_or(0);

            println!("Scavenger DB Summary (branch: {})", branch.cyan());
            println!("{}", "─".repeat(45));
            println!("  Nodes:               {node_count}");
            println!("  Edges:               {edge_count}");
            println!("  Files indexed:       {file_count}");
            println!("  Doc chunks:          {doc_chunk_count}");
            println!("  Annotations:         {annotation_count}");
            println!("  Behavioral signals:  {signal_count}");
            println!("  Session events:      {session_event_count}");
            println!("  Token log entries:   {token_log_count}");
            if let Some(ts) = last_indexed {
                println!("  Last indexed:        {ts} (unix)");
            }
            println!();
            println!(
                "  Branch DB:           {} ({:.1} MB)",
                branch_db_path.display(),
                branch_db_size as f64 / 1_048_576.0
            );
            println!(
                "  Meta DB:             {} ({:.1} MB)",
                meta_db_path.display(),
                meta_db_size as f64 / 1_048_576.0
            );
        }
        DbCommands::Nodes { limit } => {
            let conn = db::open_branch_db(&scavenger_dir, &branch)?;
            let mut stmt = conn.prepare(
                "SELECT name, kind, file_path, line_start, centrality FROM nodes ORDER BY centrality DESC LIMIT ?1"
            )?;
            let rows = stmt
                .query_map([limit], |row| {
                    Ok(vec![
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?.to_string(),
                        format!("{:.4}", row.get::<_, f64>(4)?),
                    ])
                })?
                .collect::<Result<Vec<_>, _>>()?;
            print_table(
                &["Name", "Kind", "File", "Line", "Central"],
                &[60, 12, 80, 6, 8],
                &rows,
            );
        }
        DbCommands::Files { limit } => {
            let conn = db::open_branch_db(&scavenger_dir, &branch)?;
            let mut stmt = conn.prepare(
                "SELECT file_path, file_type, raw_token_estimate, last_indexed FROM files ORDER BY last_indexed DESC LIMIT ?1"
            )?;
            let rows = stmt
                .query_map([limit], |row| {
                    Ok(vec![
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?.to_string(),
                        row.get::<_, i64>(3)?.to_string(),
                    ])
                })?
                .collect::<Result<Vec<_>, _>>()?;
            print_table(
                &["File", "Type", "Tokens", "LastIndexed"],
                &[80, 8, 8, 12],
                &rows,
            );
        }
        DbCommands::Annotations { limit } => {
            let conn = db::open_branch_db(&scavenger_dir, &branch)?;
            let mut stmt = conn.prepare(
                "SELECT id, kind, anchor_type, anchor_value, stale, substr(text, 1, 120) FROM annotations ORDER BY updated_at DESC LIMIT ?1"
            )?;
            let rows = stmt
                .query_map([limit], |row| {
                    Ok(vec![
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?
                            .unwrap_or_else(|| "fact".into()),
                        row.get::<_, Option<String>>(2)?
                            .unwrap_or_else(|| "-".into()),
                        row.get::<_, Option<String>>(3)?
                            .unwrap_or_else(|| "-".into()),
                        if row.get::<_, bool>(4)? {
                            "yes".into()
                        } else {
                            "no".into()
                        },
                        row.get::<_, String>(5)?.replace('\n', " "),
                    ])
                })?
                .collect::<Result<Vec<_>, _>>()?;
            print_table(
                &["ID", "Kind", "AnchorType", "AnchorValue", "Stale", "Text"],
                &[36, 10, 12, 40, 5, 80],
                &rows,
            );
        }
        DbCommands::Tokens { limit } => {
            let meta_conn = db::open_daemon_meta_db(&scavenger_dir)?;
            let mut stmt = meta_conn.prepare(
                "SELECT timestamp, session_id, branch, tool_name, tokens_actual, tokens_estimated, files_touched FROM token_log ORDER BY timestamp DESC LIMIT ?1"
            )?;
            let rows = stmt
                .query_map([limit], |row| {
                    let actual: i64 = row.get(4)?;
                    let estimated: i64 = row.get(5)?;
                    let saved = if estimated > 0 {
                        format!("{:.0}%", (1.0 - actual as f64 / estimated as f64) * 100.0)
                    } else {
                        "-".into()
                    };
                    Ok(vec![
                        row.get::<_, i64>(0)?.to_string(),
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        actual.to_string(),
                        estimated.to_string(),
                        saved,
                        row.get::<_, Option<String>>(6)?
                            .unwrap_or_else(|| "-".into()),
                    ])
                })?
                .collect::<Result<Vec<_>, _>>()?;
            print_table(
                &[
                    "Timestamp",
                    "Session",
                    "Branch",
                    "Tool",
                    "Actual",
                    "Estimated",
                    "Saved",
                    "File",
                ],
                &[12, 36, 45, 14, 8, 10, 6, 80],
                &rows,
            );
        }
        DbCommands::Query { sql, meta } => {
            let sql_lower = sql.trim().to_lowercase();
            if !sql_lower.starts_with("select")
                && !sql_lower.starts_with("pragma")
                && !sql_lower.starts_with("explain")
            {
                return Err("Only SELECT, PRAGMA, and EXPLAIN statements are allowed.".into());
            }

            let conn = if meta {
                db::open_daemon_meta_db(&scavenger_dir)?
            } else {
                db::open_branch_db(&scavenger_dir, &branch)?
            };

            let mut stmt = conn.prepare(&sql)?;
            let col_count = stmt.column_count();
            let col_names: Vec<String> = (0..col_count)
                .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
                .collect();
            println!("{}", col_names.join(" | "));
            println!(
                "{}",
                "─".repeat(col_names.iter().map(|c| c.len() + 3).sum::<usize>())
            );

            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let vals: Vec<String> = (0..col_count)
                    .map(|i| {
                        row.get::<_, rusqlite::types::Value>(i)
                            .map(|v| match v {
                                rusqlite::types::Value::Null => "NULL".to_string(),
                                rusqlite::types::Value::Integer(n) => n.to_string(),
                                rusqlite::types::Value::Real(f) => format!("{f:.4}"),
                                rusqlite::types::Value::Text(s) => s,
                                rusqlite::types::Value::Blob(b) => format!("[{} bytes]", b.len()),
                            })
                            .unwrap_or_else(|_| "?".to_string())
                    })
                    .collect();
                println!("{}", vals.join(" | "));
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_eval(
    suite: Option<String>,
    all: bool,
    tier: String,
    corpus: Option<String>,
    tasks: Option<String>,
    agent: Option<String>,
    json: bool,
    thresholds: Option<String>,
    baseline: bool,
    compare: Option<String>,
    report: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    use scavenger::eval::{EvalOptions, EvalSuite, EvalTier, run_evals};

    let suites = if all || suite.is_none() {
        vec![
            EvalSuite::Relevance,
            EvalSuite::Accuracy,
            EvalSuite::Performance,
        ]
    } else {
        match suite.as_deref() {
            Some("relevance") => vec![EvalSuite::Relevance],
            Some("accuracy") => vec![EvalSuite::Accuracy],
            Some("performance") => vec![EvalSuite::Performance],
            Some("agent") => vec![EvalSuite::Agent],
            Some(other) => {
                eprintln!("Unknown suite: {}", other);
                return Ok(());
            }
            None => vec![],
        }
    };

    let eval_tier = match tier.as_str() {
        "component" => EvalTier::Component,
        "agent" => EvalTier::Agent,
        "all" => EvalTier::All,
        _ => EvalTier::Component,
    };

    let opts = EvalOptions {
        suites,
        tier: eval_tier,
        corpus_path: corpus,
        json,
        thresholds_path: thresholds,
        agent,
        tasks_pattern: tasks,
        baseline,
        compare_run_id: compare,
        report,
    };

    match run_evals(&opts) {
        Ok(runs) => {
            let runs: Vec<scavenger::eval::EvalRun> = runs;
            let any_failed = runs.iter().any(|r| r.summary.failed > 0);
            if any_failed {
                std::process::exit(1);
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("Eval error: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_mcp_bridge() -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);

    if !scavenger_dir.exists() {
        return Err("Not initialized. Run `scavenger init` first.".into());
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(bridge::run_mcp_bridge(scavenger_dir))
}

fn cmd_observe(interval: u64) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);
    if !scavenger_dir.exists() {
        return Err("Not initialized. Run `scavenger init` first.".into());
    }
    observe::run(&scavenger_dir, interval)
}

fn cmd_logs(
    follow: bool,
    level_filter: String,
    method_filter: Option<String>,
    max_lines: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let project_root = std::env::current_dir()?;
    let scavenger_dir = db::scavenger_dir(&project_root);

    let log_dir = &scavenger_dir;
    let mut log_files: Vec<_> = std::fs::read_dir(log_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("daemon.log"))
        .collect();

    if log_files.is_empty() {
        eprintln!("No daemon log files found in {}", log_dir.display());
        return Ok(());
    }

    log_files.sort_by_key(|e| std::cmp::Reverse(e.metadata().ok().and_then(|m| m.modified().ok())));

    let level_num = match level_filter.to_lowercase().as_str() {
        "trace" => 0,
        "debug" => 1,
        "info" => 2,
        "warn" => 3,
        "error" => 4,
        _ => 2,
    };

    let format_line = |line: &str| -> Option<String> {
        let parsed: serde_json::Value = serde_json::from_str(line).ok()?;

        let log_level = parsed
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("INFO");
        let line_level_num = match log_level.to_uppercase().as_str() {
            "TRACE" => 0,
            "DEBUG" => 1,
            "INFO" => 2,
            "WARN" => 3,
            "ERROR" => 4,
            _ => 2,
        };
        if line_level_num < level_num {
            return None;
        }

        if let Some(ref method) = method_filter {
            let span_str = parsed
                .get("span")
                .or_else(|| parsed.get("spans"))
                .map(|v| v.to_string())
                .unwrap_or_default();
            let target = parsed.get("target").and_then(|v| v.as_str()).unwrap_or("");
            let message = parsed
                .get("fields")
                .and_then(|f| f.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !span_str.contains(method) && !target.contains(method) && !message.contains(method) {
                return None;
            }
        }

        let ts = parsed
            .get("timestamp")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let ts_short = if ts.len() > 19 { &ts[11..19] } else { ts };

        let fields = parsed.get("fields").and_then(|v| v.as_object());
        let message = fields
            .and_then(|f| f.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let mut extra_fields = String::new();
        if let Some(f) = fields {
            for (k, v) in f {
                if k == "message" {
                    continue;
                }
                if !extra_fields.is_empty() {
                    extra_fields.push_str(", ");
                }
                extra_fields.push_str(&format!("{k}={v}"));
            }
        }

        let level_colored = match log_level.to_uppercase().as_str() {
            "ERROR" => format!("{}", log_level.red().bold()),
            "WARN" => format!("{}", log_level.yellow()),
            "INFO" => format!("{}", log_level.green()),
            "DEBUG" => format!("{}", log_level.blue()),
            _ => log_level.to_string(),
        };

        if extra_fields.is_empty() {
            Some(format!("{ts_short} {level_colored:<5} {message}"))
        } else {
            Some(format!(
                "{ts_short} {level_colored:<5} {message}  {}",
                extra_fields.dimmed()
            ))
        }
    };

    // Read and display recent lines
    let log_path = log_files[0].path();
    let content = std::fs::read_to_string(&log_path).unwrap_or_default();
    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(max_lines * 3); // read extra to account for filtering
    let mut displayed = 0;
    let mut output_lines = Vec::new();
    for line in &all_lines[start..] {
        if let Some(formatted) = format_line(line) {
            output_lines.push(formatted);
        }
    }
    let skip = output_lines.len().saturating_sub(max_lines);
    for line in &output_lines[skip..] {
        println!("{line}");
        displayed += 1;
    }
    if displayed == 0 {
        eprintln!("No log entries match the filter.");
    }

    if follow {
        use std::io::BufRead;
        let file = std::fs::File::open(&log_path)?;
        let mut reader = std::io::BufReader::new(file);
        reader.seek_relative(content.len() as i64)?;
        eprintln!("{}", "--- following log (Ctrl+C to stop) ---".dimmed());
        let mut line_buf = String::new();
        loop {
            line_buf.clear();
            match reader.read_line(&mut line_buf) {
                Ok(0) => {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                Ok(_) => {
                    if let Some(formatted) = format_line(line_buf.trim()) {
                        println!("{formatted}");
                    }
                }
                Err(_) => break,
            }
        }
    }

    Ok(())
}

/// Print a table with adaptive column widths.
///
/// Each column width = min(cap, max(header_len, max_data_len)).
/// The last column has no cap -- it gets whatever space remains.
/// Values exceeding the cap are truncated with "…".
fn print_table(headers: &[&str], caps: &[usize], rows: &[Vec<String>]) {
    if rows.is_empty() {
        eprintln!("No rows found.");
        return;
    }

    let ncols = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();

    for row in rows {
        for (i, val) in row.iter().enumerate().take(ncols) {
            widths[i] = widths[i].max(val.len());
        }
    }

    // Apply caps
    for (i, w) in widths.iter_mut().enumerate() {
        *w = (*w).min(caps.get(i).copied().unwrap_or(usize::MAX));
    }

    // Header
    let mut header_line = String::new();
    for (i, h) in headers.iter().enumerate() {
        if i > 0 {
            header_line.push_str("  ");
        }
        header_line.push_str(&format!("{:<width$}", h, width = widths[i]));
    }
    println!("{header_line}");
    println!("{}", "─".repeat(header_line.len()));

    // Rows
    for row in rows {
        let mut line = String::new();
        for (i, w) in widths.iter().enumerate().take(ncols) {
            if i > 0 {
                line.push_str("  ");
            }
            let val = row.get(i).map(|s| s.as_str()).unwrap_or("");
            let w = *w;
            if val.len() > w && w > 1 {
                line.push_str(&val[..w - 1]);
                line.push('…');
            } else {
                line.push_str(&format!("{:<width$}", val, width = w));
            }
        }
        println!("{line}");
    }
}

fn append_to_gitignore(project_root: &std::path::Path) -> Result<(), std::io::Error> {
    let gitignore_path = project_root.join(".gitignore");
    let entry = ".scavenger/";

    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        if content.lines().any(|l| l.trim() == entry) {
            return Ok(());
        }
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&gitignore_path)?;
        use std::io::Write;
        if !content.ends_with('\n') {
            writeln!(file)?;
        }
        writeln!(file, "{entry}")?;
    } else {
        std::fs::write(&gitignore_path, format!("{entry}\n"))?;
    }
    Ok(())
}
