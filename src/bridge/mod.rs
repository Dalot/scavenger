// MCP bridge placeholder — full rmcp integration requires async runtime setup.
// For v1, the MCP bridge functionality is handled via the daemon UDS protocol
// (src/daemon/handlers.rs) and CLI hook commands (src/hooks/mod.rs).
//
// The rmcp-based stdio JSON-RPC bridge will be implemented as a separate binary
// mode (scavenger daemon --mcp-bridge) that translates stdio to UDS.
//
// Current architecture: Claude Code hooks → CLI → UDS → daemon handlers.

use std::path::Path;

use serde_json::{json, Value};

/// Send a capsule request to the daemon via UDS.
pub async fn get_capsule(
    socket_path: &Path,
    file: &str,
    symbol: Option<&str>,
    query: Option<&str>,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut request = json!({
        "method": "capsule",
        "file": file,
    });
    if let Some(s) = symbol {
        request["symbol"] = json!(s);
    }
    if let Some(q) = query {
        request["query"] = json!(q);
    }
    crate::daemon::socket::send_request(socket_path, &request).await
}

/// Send an annotation read request to the daemon via UDS.
pub async fn read_annotations(
    socket_path: &Path,
    anchor_type: Option<&str>,
    anchor_value: Option<&str>,
    tags: Option<&str>,
    query: Option<&str>,
    session_summary: bool,
    limit: Option<u32>,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut request = json!({ "method": "annotation_read" });
    if let Some(at) = anchor_type {
        request["anchor_type"] = json!(at);
    }
    if let Some(av) = anchor_value {
        request["anchor_value"] = json!(av);
    }
    if let Some(t) = tags {
        request["tags"] = json!(t);
    }
    if let Some(q) = query {
        request["query"] = json!(q);
    }
    if session_summary {
        request["session_summary"] = json!(true);
    }
    if let Some(l) = limit {
        request["limit"] = json!(l);
    }
    crate::daemon::socket::send_request(socket_path, &request).await
}

/// Send an annotation write request to the daemon via UDS.
pub async fn write_annotation(
    socket_path: &Path,
    id: Option<&str>,
    text: &str,
    tags: Option<&str>,
    symbol: Option<&str>,
    file: Option<&str>,
    scope: Option<&str>,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut request = json!({
        "method": "annotation_write",
        "text": text,
    });
    if let Some(i) = id {
        request["id"] = json!(i);
    }
    if let Some(t) = tags {
        request["tags"] = json!(t);
    }

    // Anchor resolution cascade: symbol → file → scope → None
    if let Some(s) = symbol {
        request["anchor_type"] = json!("node");
        request["anchor_value"] = json!(s);
    } else if let Some(f) = file {
        request["anchor_type"] = json!("file");
        request["anchor_value"] = json!(f);
    } else if let Some(sc) = scope {
        request["anchor_type"] = json!("scope");
        request["anchor_value"] = json!(sc);
    }

    crate::daemon::socket::send_request(socket_path, &request).await
}

/// Send an annotation delete request to the daemon via UDS.
pub async fn delete_annotation(
    socket_path: &Path,
    id: &str,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let request = json!({
        "method": "annotation_delete",
        "id": id,
    });
    crate::daemon::socket::send_request(socket_path, &request).await
}

/// Send a doc search request to the daemon via UDS.
pub async fn search_docs(
    socket_path: &Path,
    query: &str,
    limit: Option<u32>,
) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut request = json!({
        "method": "search_docs",
        "query": query,
    });
    if let Some(l) = limit {
        request["limit"] = json!(l);
    }
    crate::daemon::socket::send_request(socket_path, &request).await
}
