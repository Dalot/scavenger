use std::path::PathBuf;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::daemon::socket::send_request;

#[derive(Deserialize, JsonSchema)]
pub struct GetCapsuleParams {
    /// File path relative to project root
    pub file: String,
    /// Symbol name. If omitted, returns capsule for the file's primary export.
    pub symbol: Option<String>,
    /// Your intent or question — drives context selection strategy.
    pub query: Option<String>,
    /// Control context depth. "minimal" = target + 1-hop only (~200-800 tokens).
    /// "standard" (default) = full structural context + annotations (~800-3000 tokens).
    /// "detailed" = everything including doc chunks and full body (~3000-8000 tokens).
    pub detail_level: Option<String>,
    /// Token budget override (default from config, typically 8000).
    pub budget: Option<u32>,
    /// Override max caller count (default from detail_level).
    pub max_callers: Option<u32>,
    /// Override max callee count (default from detail_level).
    pub max_callees: Option<u32>,
    /// Override max annotation count (default from detail_level).
    pub max_annotations: Option<u32>,
    /// Whether to include full function body if budget allows (default from detail_level).
    pub include_body: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadAnnotationsParams {
    /// Filter by anchor type: 'node', 'file', 'scope'.
    pub anchor_type: Option<String>,
    /// Filter by anchor value.
    pub anchor_value: Option<String>,
    /// Filter by tags (comma-separated).
    pub tags: Option<String>,
    /// Full-text search query across annotation text and tags.
    pub query: Option<String>,
    /// If true, returns a session start summary.
    pub session_summary: Option<bool>,
    /// Maximum results (default 10).
    pub limit: Option<u32>,
}

#[derive(Deserialize, JsonSchema)]
pub struct WriteAnnotationParams {
    /// Annotation ID to update. Omit to create a new annotation.
    pub id: Option<String>,
    /// The annotation text. Be specific — retrieved via keyword search.
    pub text: String,
    /// Comma-separated keywords for retrieval (e.g. 'auth,jwt,redis').
    pub tags: Option<String>,
    /// Symbol name to anchor to. Resolved via search.
    pub symbol: Option<String>,
    /// File path to anchor to if no symbol specified.
    pub file: Option<String>,
    /// Scope name to anchor to (e.g. 'auth', 'api').
    pub scope: Option<String>,
    /// Annotation kind: 'fact' (default), 'strategy', 'pitfall', 'context'.
    pub kind: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct DeleteAnnotationParams {
    /// Annotation ID to delete.
    pub id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct SearchDocsParams {
    /// Search query.
    pub query: String,
    /// Maximum results (default 5).
    pub limit: Option<u32>,
}

#[derive(Clone)]
pub struct ScavengerBridge {
    socket_path: PathBuf,
    pub tool_router: ToolRouter<Self>,
}

impl ScavengerBridge {
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path: socket_path.clone(),
            tool_router: Self::tool_router(),
        }
    }

    async fn uds_request(&self, request: &Value) -> Value {
        match send_request(&self.socket_path, request).await {
            Ok(v) => v,
            Err(e) => json!({ "error": format!("daemon connection failed: {e}") }),
        }
    }
}

#[tool_router]
impl ScavengerBridge {
    #[tool(
        name = "get_capsule",
        description = "Primary navigation tool — replaces grep and file reads for structural questions. Pass a symbol name to get its callers, callees, and what would break if it changed (no grep needed). Pass a file path to get focused context within a token budget instead of reading the raw file. Returns signatures, dependency neighborhood, annotations, and behavioral signals from the AST graph.\n\nParameters:\n- detail_level: Control context depth. \"minimal\" = target + 1-hop only (~200-800 tokens). \"standard\" (default) = full structural context + annotations (~800-3000 tokens). \"detailed\" = everything including doc chunks and full body (~3000-8000 tokens).\n- budget: Token budget override (default from config, typically 8000).\n- max_callers: Override max caller count (default from detail_level).\n- max_callees: Override max callee count (default from detail_level).\n- max_annotations: Override max annotation count (default from detail_level).\n- include_body: Whether to include full function body if budget allows (default from detail_level)."
    )]
    async fn get_capsule(&self, params: Parameters<GetCapsuleParams>) -> Result<String, String> {
        let p = params.0;
        let mut req = json!({
            "method": "capsule",
            "file": p.file,
        });
        if let Some(s) = &p.symbol {
            req["symbol"] = json!(s);
        }
        if let Some(q) = &p.query {
            req["query"] = json!(q);
        }
        if let Some(dl) = &p.detail_level {
            req["detail_level"] = json!(dl);
        }
        if let Some(b) = p.budget {
            req["budget"] = json!(b);
        }
        if let Some(mc) = p.max_callers {
            req["max_callers"] = json!(mc);
        }
        if let Some(mc) = p.max_callees {
            req["max_callees"] = json!(mc);
        }
        if let Some(ma) = p.max_annotations {
            req["max_annotations"] = json!(ma);
        }
        if let Some(ib) = p.include_body {
            req["include_body"] = json!(ib);
        }
        let resp = self.uds_request(&req).await;

        if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
            return Err(err.to_string());
        }

        let capsule_text = resp.get("capsule").and_then(|v| v.as_str()).unwrap_or("");
        Ok(capsule_text.to_string())
    }

    #[tool(
        name = "read_annotations",
        description = "Retrieve annotations and session memory. At session start, call with session_summary=true to resume from prior sessions (activity, stale annotations, active signals). Per-node annotations are also included in get_capsule results automatically."
    )]
    async fn read_annotations(
        &self,
        params: Parameters<ReadAnnotationsParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut req = json!({ "method": "annotation_read" });
        if let Some(at) = &p.anchor_type {
            req["anchor_type"] = json!(at);
        }
        if let Some(av) = &p.anchor_value {
            req["anchor_value"] = json!(av);
        }
        if let Some(t) = &p.tags {
            req["tags"] = json!(t);
        }
        if let Some(q) = &p.query {
            req["query"] = json!(q);
        }
        if p.session_summary.unwrap_or(false) {
            req["session_summary"] = json!(true);
        }
        if let Some(l) = p.limit {
            req["limit"] = json!(l);
        }
        let resp = self.uds_request(&req).await;
        serde_json::to_string_pretty(&resp).map_err(|e| e.to_string())
    }

    #[tool(
        name = "write_annotation",
        description = "Persist a fact, decision, or note anchored to code. Creates a new annotation or updates an existing one. Use for cross-session knowledge: architectural decisions, discovered bugs, learned patterns. Anchor to a symbol, file, or scope for precise future retrieval via get_capsule and read_annotations."
    )]
    async fn write_annotation(
        &self,
        params: Parameters<WriteAnnotationParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let mut req = json!({
            "method": "annotation_write",
            "text": p.text,
        });
        if let Some(i) = &p.id {
            req["id"] = json!(i);
        }
        if let Some(t) = &p.tags {
            req["tags"] = json!(t);
        }
        if let Some(k) = &p.kind {
            req["kind"] = json!(k);
        }
        if let Some(s) = &p.symbol {
            req["anchor_type"] = json!("node");
            req["anchor_value"] = json!(s);
        } else if let Some(f) = &p.file {
            req["anchor_type"] = json!("file");
            req["anchor_value"] = json!(f);
        } else if let Some(sc) = &p.scope {
            req["anchor_type"] = json!("scope");
            req["anchor_value"] = json!(sc);
        }
        let resp = self.uds_request(&req).await;

        if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
            return Err(err.to_string());
        }

        serde_json::to_string_pretty(&resp).map_err(|e| e.to_string())
    }

    #[tool(
        name = "delete_annotation",
        description = "Delete an annotation by ID."
    )]
    async fn delete_annotation(
        &self,
        params: Parameters<DeleteAnnotationParams>,
    ) -> Result<String, String> {
        let p = params.0;
        let req = json!({
            "method": "annotation_delete",
            "id": p.id,
        });
        let resp = self.uds_request(&req).await;

        if let Some(err) = resp.get("error").and_then(|v| v.as_str()) {
            return Err(err.to_string());
        }

        serde_json::to_string_pretty(&resp).map_err(|e| e.to_string())
    }

    #[tool(
        name = "search_docs",
        description = "Search indexed documentation files (CLAUDE.md, README.md, docs/**/*.md). Use to find design rationale, architecture decisions, or project conventions without loading entire documentation files."
    )]
    async fn search_docs(&self, params: Parameters<SearchDocsParams>) -> Result<String, String> {
        let p = params.0;
        let mut req = json!({
            "method": "search_docs",
            "query": p.query,
        });
        if let Some(l) = p.limit {
            req["limit"] = json!(l);
        }
        let resp = self.uds_request(&req).await;
        serde_json::to_string_pretty(&resp).map_err(|e| e.to_string())
    }
}

#[tool_handler]
impl ServerHandler for ScavengerBridge {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Scavenger: AST dependency graph and session memory engine. \
                 Prefer get_capsule over grep/read for code navigation — it resolves \
                 callers, callees, and impact from the graph directly. \
                 Use write_annotation to persist cross-session knowledge."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..ServerInfo::default()
        }
    }
}

/// Start the MCP bridge as a stdio JSON-RPC server.
/// If the daemon isn't running, spawns it and waits for the socket to appear.
pub async fn run_mcp_bridge(scavenger_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    use rmcp::ServiceExt;

    let socket_path = scavenger_dir.join("daemon.sock");

    if !socket_path.exists() {
        ensure_daemon_running(&scavenger_dir).await?;
    }

    let bridge = ScavengerBridge::new(socket_path);
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    let server = bridge.serve(transport).await?;
    server.waiting().await?;
    Ok(())
}

/// Spawn the daemon process if not already running and wait for the socket.
async fn ensure_daemon_running(
    scavenger_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let pid_path = scavenger_dir.join("daemon.pid");
    let socket_path = scavenger_dir.join("daemon.sock");

    let already_running = if pid_path.exists() {
        std::fs::read_to_string(&pid_path)
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok())
            .is_some_and(|pid| pid > 0 && std::path::Path::new(&format!("/proc/{pid}")).exists())
    } else {
        false
    };

    if !already_running {
        let exe = std::env::current_exe().unwrap_or_else(|_| "scavenger".into());
        std::process::Command::new(exe)
            .args(["daemon", "start"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
    }

    // Wait up to 5 seconds for the socket to appear
    for _ in 0..50 {
        if socket_path.exists() {
            return Ok(());
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    Err("Daemon did not start within 5 seconds".into())
}
