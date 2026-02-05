//! Bidirectional JSON-RPC 2.0 multiplexer over stdin/stdout of a child process.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};

use super::types::{
    ClassifiedMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, JsonRpcResponseOut,
    RawJsonRpcMessage, RequestId,
};

/// Bidirectional JSON-RPC transport over a child process's stdio.
///
/// Spawns reader and writer tasks that multiplex:
/// - Outgoing requests/notifications via an mpsc channel to stdin
/// - Incoming responses routed to pending oneshot receivers
/// - Incoming notifications broadcast to subscribers
/// - Incoming agent requests dispatched to a handler
pub struct AgentTransport {
    write_tx: mpsc::Sender<String>,
    notification_tx: broadcast::Sender<JsonRpcNotification>,
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>,
    next_id: Arc<AtomicU64>,
    child: Arc<Mutex<Child>>,
}

impl AgentTransport {
    /// Spawn a child process and start reader/writer tasks.
    pub fn spawn(command: &str, args: &[String]) -> Result<Self, TransportError> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| TransportError::SpawnFailed {
                command: command.to_string(),
                source: e,
            })?;

        let stdin = child.stdin.take().ok_or(TransportError::NoStdin)?;
        let stdout = child.stdout.take().ok_or(TransportError::NoStdout)?;

        // Log stderr in background
        if let Some(stderr) = child.stderr.take() {
            let cmd = command.to_string();
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!("[{}:stderr] {}", cmd, line);
                }
            });
        }

        let (write_tx, write_rx) = mpsc::channel::<String>(64);
        let (notification_tx, _) = broadcast::channel::<JsonRpcNotification>(128);
        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Writer task: drains mpsc and writes lines to stdin
        Self::spawn_writer(stdin, write_rx);

        // Reader task: reads lines from stdout, classifies, and dispatches
        Self::spawn_reader(
            stdout,
            pending.clone(),
            notification_tx.clone(),
            write_tx.clone(),
        );

        Ok(Self {
            write_tx,
            notification_tx,
            pending,
            next_id: Arc::new(AtomicU64::new(1)),
            child: Arc::new(Mutex::new(child)),
        })
    }

    /// Send a JSON-RPC request and await the response.
    pub async fn request(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, TransportError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request_id = RequestId::Number(id);

        let req = JsonRpcRequest::new(request_id.clone(), method, params);
        let json = serde_json::to_string(&req).map_err(TransportError::Serialize)?;

        // Register a oneshot to receive the response
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(request_id.to_string(), tx);
        }

        // Send the request
        self.write_tx
            .send(json)
            .await
            .map_err(|_| TransportError::WriterClosed)?;

        // Await response with timeout
        let response = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| TransportError::Timeout {
                method: method.to_string(),
            })?
            .map_err(|_| TransportError::ResponseDropped)?;

        if let Some(err) = response.error {
            return Err(TransportError::RpcError(err));
        }

        Ok(response.result.unwrap_or(serde_json::Value::Null))
    }

    /// Send a fire-and-forget notification.
    pub async fn notify(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<(), TransportError> {
        let msg = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let json = serde_json::to_string(&msg).map_err(TransportError::Serialize)?;
        self.write_tx
            .send(json)
            .await
            .map_err(|_| TransportError::WriterClosed)?;
        Ok(())
    }

    /// Subscribe to notifications from the agent.
    pub fn subscribe(&self) -> broadcast::Receiver<JsonRpcNotification> {
        self.notification_tx.subscribe()
    }

    /// Clone the write channel (for use in spawned tasks).
    pub fn clone_write_tx(&self) -> mpsc::Sender<String> {
        self.write_tx.clone()
    }

    /// Clone the pending requests map (for use in spawned tasks).
    pub fn clone_pending(&self) -> Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>> {
        self.pending.clone()
    }

    /// Clone the ID counter (for use in spawned tasks).
    pub fn clone_next_id(&self) -> Arc<AtomicU64> {
        self.next_id.clone()
    }

    /// Kill the child process.
    pub async fn kill(&self) -> Result<(), TransportError> {
        let mut child = self.child.lock().await;
        child.kill().await.map_err(TransportError::KillFailed)?;
        Ok(())
    }

    // ---- internal tasks ----

    fn spawn_writer(mut stdin: tokio::process::ChildStdin, mut rx: mpsc::Receiver<String>) {
        tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if let Err(e) = stdin.write_all(line.as_bytes()).await {
                    tracing::error!("ACP writer: failed to write: {}", e);
                    break;
                }
                if let Err(e) = stdin.write_all(b"\n").await {
                    tracing::error!("ACP writer: failed to write newline: {}", e);
                    break;
                }
                if let Err(e) = stdin.flush().await {
                    tracing::error!("ACP writer: failed to flush: {}", e);
                    break;
                }
            }
            tracing::debug!("ACP writer task exited");
        });
    }

    fn spawn_reader(
        stdout: tokio::process::ChildStdout,
        pending: Arc<Mutex<HashMap<String, oneshot::Sender<JsonRpcResponse>>>>,
        notification_tx: broadcast::Sender<JsonRpcNotification>,
        write_tx: mpsc::Sender<String>,
    ) {
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.is_empty() {
                    continue;
                }

                let raw: RawJsonRpcMessage = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("ACP reader: malformed JSON, skipping: {}", e);
                        continue;
                    }
                };

                match raw.classify() {
                    ClassifiedMessage::Response(resp) => {
                        let key = resp.id.to_string();
                        let mut pending = pending.lock().await;
                        if let Some(tx) = pending.remove(&key) {
                            let _ = tx.send(resp);
                        } else {
                            tracing::warn!("ACP reader: no pending request for id={}", key);
                        }
                    }
                    ClassifiedMessage::Notification(notif) => {
                        // Broadcast to subscribers; ignore if no receivers
                        let _ = notification_tx.send(notif);
                    }
                    ClassifiedMessage::AgentRequest { id, method, params } => {
                        let response = handle_agent_request(&id, &method, params.as_ref()).await;
                        let json = match serde_json::to_string(&response) {
                            Ok(j) => j,
                            Err(e) => {
                                tracing::error!("ACP reader: failed to serialize response: {}", e);
                                continue;
                            }
                        };
                        if write_tx.send(json).await.is_err() {
                            tracing::error!("ACP reader: writer channel closed");
                            break;
                        }
                    }
                }
            }
            tracing::debug!("ACP reader task exited");
        });
    }
}

/// Handle an agent-initiated request (agent -> client).
async fn handle_agent_request(
    id: &RequestId,
    method: &str,
    params: Option<&serde_json::Value>,
) -> JsonRpcResponseOut {
    match method {
        "fs/readTextFile" => {
            let path = params
                .and_then(|p| p.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match tokio::fs::read_to_string(path).await {
                Ok(contents) => {
                    JsonRpcResponseOut::success(id.clone(), serde_json::json!({ "text": contents }))
                }
                Err(e) => JsonRpcResponseOut::error(
                    id.clone(),
                    -32000,
                    format!("Failed to read file: {}", e),
                ),
            }
        }
        "fs/writeTextFile" => {
            let path = params
                .and_then(|p| p.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let content = params
                .and_then(|p| p.get("content"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match tokio::fs::write(path, content).await {
                Ok(()) => {
                    JsonRpcResponseOut::success(id.clone(), serde_json::json!({ "success": true }))
                }
                Err(e) => JsonRpcResponseOut::error(
                    id.clone(),
                    -32000,
                    format!("Failed to write file: {}", e),
                ),
            }
        }
        "session/requestPermission" => {
            // Auto-approve all permission requests — this chat is product management scope
            JsonRpcResponseOut::success(id.clone(), serde_json::json!({ "decision": "allow_once" }))
        }
        _ => {
            tracing::warn!("ACP: unknown agent request method: {}", method);
            JsonRpcResponseOut::error(id.clone(), -32601, format!("Method not found: {}", method))
        }
    }
}

/// Errors from the transport layer.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("Failed to spawn '{command}': {source}")]
    SpawnFailed {
        command: String,
        source: std::io::Error,
    },
    #[error("Child process has no stdin")]
    NoStdin,
    #[error("Child process has no stdout")]
    NoStdout,
    #[error("JSON serialization error: {0}")]
    Serialize(serde_json::Error),
    #[error("Writer channel closed")]
    WriterClosed,
    #[error("Request timed out: {method}")]
    Timeout { method: String },
    #[error("Response channel dropped")]
    ResponseDropped,
    #[error("JSON-RPC error: {0}")]
    RpcError(super::types::JsonRpcError),
    #[error("Failed to kill child process: {0}")]
    KillFailed(std::io::Error),
}
