use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::sync::watch;

use super::handlers;
use super::DaemonState;

/// Listen on a Unix Domain Socket, dispatching JSON requests to handlers.
/// Protocol: length-prefixed JSON (4-byte big-endian length prefix).
pub async fn listen(
    socket_path: PathBuf,
    state: Arc<DaemonState>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = UnixListener::bind(&socket_path)?;

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _addr)) => {
                        let state = state.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, state).await {
                                tracing::debug!(error = %e, "connection handler error");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "socket accept error");
                    }
                }
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
        }
    }

    Ok(())
}

async fn handle_connection(
    mut stream: tokio::net::UnixStream,
    state: Arc<DaemonState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Read length prefix (4 bytes, big-endian)
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let msg_len = u32::from_be_bytes(len_buf) as usize;

    if msg_len > 10 * 1024 * 1024 {
        return Err("Message too large".into());
    }

    // Read message body
    let mut msg_buf = vec![0u8; msg_len];
    stream.read_exact(&mut msg_buf).await?;

    let request: serde_json::Value = serde_json::from_slice(&msg_buf)?;
    let response = handlers::dispatch(&state, request).await;

    // Write response with length prefix
    let resp_bytes = serde_json::to_vec(&response)?;
    let resp_len = (resp_bytes.len() as u32).to_be_bytes();
    stream.write_all(&resp_len).await?;
    stream.write_all(&resp_bytes).await?;

    Ok(())
}

/// Send a request to the daemon via UDS and receive the response.
pub async fn send_request(
    socket_path: &std::path::Path,
    request: &serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = tokio::net::UnixStream::connect(socket_path).await?;

    let req_bytes = serde_json::to_vec(request)?;
    let len = (req_bytes.len() as u32).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(&req_bytes).await?;

    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let resp_len = u32::from_be_bytes(len_buf) as usize;

    let mut resp_buf = vec![0u8; resp_len];
    stream.read_exact(&mut resp_buf).await?;

    let response: serde_json::Value = serde_json::from_slice(&resp_buf)?;
    Ok(response)
}
