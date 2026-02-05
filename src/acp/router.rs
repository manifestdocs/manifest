//! ACP router: session pool + SSE translation layer.
//!
//! Manages a pool of agent processes and translates ACP session/update
//! notifications into SSE events for the web client.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::response::sse;
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;

use super::process::{AgentProcess, ProcessError};
use super::registry;
use super::types::{ContentBlock, SessionUpdate};

/// Manages active agent sessions and translates between ACP and SSE.
pub struct AcpRouter {
    sessions: Arc<Mutex<HashMap<String, AgentProcess>>>,
}

impl AcpRouter {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get a clone of the sessions map (for use by the reaper task).
    pub fn sessions_ref(&self) -> Arc<Mutex<HashMap<String, AgentProcess>>> {
        self.sessions.clone()
    }

    /// Reap idle sessions from a shared sessions map.
    /// Called by the background reaper task.
    pub async fn reap_idle_sessions(
        sessions: &Arc<Mutex<HashMap<String, AgentProcess>>>,
        max_idle: Duration,
    ) {
        let mut sessions = sessions.lock().await;
        let now = tokio::time::Instant::now();
        let mut to_remove = Vec::new();

        for (sid, process) in sessions.iter() {
            if now.duration_since(process.last_activity()) > max_idle {
                tracing::info!("ACP: reaping idle {} session {}", process.agent_name(), sid);
                to_remove.push(sid.clone());
            }
        }

        for sid in to_remove {
            if let Some(process) = sessions.remove(&sid) {
                let _ = process.shutdown().await;
            }
        }
    }

    /// Perform a chat completion: spawn or reuse an agent, send a prompt,
    /// and return a stream of SSE events.
    ///
    /// Returns `(session_id, sse_stream)`.
    pub async fn chat_completions(
        &self,
        agent_name: &str,
        session_id: Option<&str>,
        system_prompt: String,
        user_prompt: String,
    ) -> Result<
        (
            String,
            ReceiverStream<Result<sse::Event, std::convert::Infallible>>,
        ),
        RouterError,
    > {
        // Resolve agent spawn config
        let config = registry::spawn_config(agent_name)
            .ok_or_else(|| RouterError::UnsupportedAgent(agent_name.to_string()))?;

        // Determine session: reuse existing or spawn new
        let effective_session_id;
        let mut sessions = self.sessions.lock().await;

        if let Some(sid) = session_id {
            if sessions.contains_key(sid) {
                effective_session_id = sid.to_string();
            } else {
                // Session died or was reaped — spawn new process
                let process = AgentProcess::start(config, agent_name)
                    .await
                    .map_err(|e| map_process_error(agent_name, e))?;
                effective_session_id = process.session_id().to_string();
                sessions.insert(effective_session_id.clone(), process);
            }
        } else {
            // No session_id — always spawn new
            let process = AgentProcess::start(config, agent_name)
                .await
                .map_err(|e| map_process_error(agent_name, e))?;
            effective_session_id = process.session_id().to_string();
            sessions.insert(effective_session_id.clone(), process);
        }

        // Build prompt content blocks
        let mut content = Vec::new();
        if !system_prompt.is_empty() {
            content.push(ContentBlock::Text {
                text: system_prompt,
            });
        }
        content.push(ContentBlock::Text { text: user_prompt });

        // Start the streaming prompt
        let process = sessions.get_mut(&effective_session_id).unwrap();
        let (mut notification_rx, prompt_handle) = process.prompt_streaming(content);

        // Drop the sessions lock before streaming
        drop(sessions);

        // Create an mpsc channel for SSE events
        let (sse_tx, sse_rx) =
            tokio::sync::mpsc::channel::<Result<sse::Event, std::convert::Infallible>>(64);

        let sid = effective_session_id.clone();
        let sessions_ref = self.sessions.clone();

        // Spawn a task that reads notifications and forwards SSE events
        tokio::spawn(async move {
            // First event: session ID
            let _ = sse_tx
                .send(Ok(sse::Event::default().data(
                    serde_json::json!({
                        "type": "session",
                        "session_id": sid
                    })
                    .to_string(),
                )))
                .await;

            // Read notifications until the prompt completes or the channel closes
            loop {
                tokio::select! {
                    notif_result = notification_rx.recv() => {
                        match notif_result {
                            Ok(notif) if notif.method == "session/update" => {
                                tracing::debug!("ACP received session/update: {:?}", notif.params);
                                if let Some(params) = &notif.params {
                                    if let Some(event) = session_update_to_sse(params) {
                                        tracing::debug!("ACP sending SSE event");
                                        if sse_tx.send(Ok(event)).await.is_err() {
                                            break; // Client disconnected
                                        }
                                    }
                                }
                            }
                            Ok(notif) => {
                                tracing::debug!("ACP received non-update notification: {}", notif.method);
                            } // Non-update notification
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                tracing::warn!("ACP SSE: lagged {} notifications", n);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                // Agent process reader exited — check if prompt also finished
                                break;
                            }
                        }
                    }
                    // Check if prompt completed (JoinHandle finished)
                    _ = async {
                        // Poll until handle is finished
                        loop {
                            if prompt_handle.is_finished() {
                                return;
                            }
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    } => {
                        // Drain remaining notifications
                        while let Ok(notif) = notification_rx.try_recv() {
                            if notif.method == "session/update" {
                                if let Some(params) = &notif.params {
                                    if let Some(event) = session_update_to_sse(params) {
                                        let _ = sse_tx.send(Ok(event)).await;
                                    }
                                }
                            }
                        }
                        break;
                    }
                }
            }

            // Handle prompt result
            match prompt_handle.await {
                Ok(Ok(_)) => {
                    let _ = sse_tx.send(Ok(sse::Event::default().data("[DONE]"))).await;
                }
                Ok(Err(e)) => {
                    let event = serde_json::json!({
                        "type": "error",
                        "error": { "message": e.to_string() }
                    });
                    let _ = sse_tx
                        .send(Ok(sse::Event::default().data(event.to_string())))
                        .await;
                    let _ = sse_tx.send(Ok(sse::Event::default().data("[DONE]"))).await;
                }
                Err(e) => {
                    let event = serde_json::json!({
                        "type": "error",
                        "error": { "message": format!("Agent crashed: {}", e) }
                    });
                    let _ = sse_tx
                        .send(Ok(sse::Event::default().data(event.to_string())))
                        .await;
                    let _ = sse_tx.send(Ok(sse::Event::default().data("[DONE]"))).await;

                    // Clean up the crashed session
                    let mut sessions = sessions_ref.lock().await;
                    if let Some(process) = sessions.remove(&sid) {
                        let _ = process.shutdown().await;
                    }
                    return;
                }
            }

            // Touch last_activity
            let mut sessions = sessions_ref.lock().await;
            if let Some(process) = sessions.get_mut(&sid) {
                process.touch();
            }
        });

        Ok((effective_session_id, ReceiverStream::new(sse_rx)))
    }
}

/// Translate ACP session/update notification params into an SSE event.
fn session_update_to_sse(params: &serde_json::Value) -> Option<sse::Event> {
    let update = SessionUpdate::from_notification_params(params)?;
    let json = match update {
        SessionUpdate::AgentMessageChunk { text } => {
            serde_json::json!({
                "type": "content_block_delta",
                "delta": { "type": "text_delta", "text": text }
            })
        }
        SessionUpdate::ToolCall { id, title } => {
            serde_json::json!({
                "type": "tool_call",
                "tool": { "id": id, "name": title }
            })
        }
        SessionUpdate::ToolCallUpdate { id, status } => {
            serde_json::json!({
                "type": "tool_call_update",
                "tool": { "id": id },
                "status": status
            })
        }
        SessionUpdate::Plan { content } => {
            tracing::debug!("ACP: agent plan: {}", content);
            return None; // Plans are logged, not streamed
        }
    };

    Some(sse::Event::default().data(json.to_string()))
}

fn map_process_error(agent_name: &str, e: ProcessError) -> RouterError {
    match &e {
        ProcessError::Transport(super::transport::TransportError::SpawnFailed { .. }) => {
            RouterError::AgentNotInstalled {
                agent: agent_name.to_string(),
                command: registry::spawn_config(agent_name)
                    .map(|c| c.command.to_string())
                    .unwrap_or_else(|| agent_name.to_string()),
            }
        }
        ProcessError::InitializeTimeout => RouterError::InitializeTimeout,
        _ => RouterError::Process(e),
    }
}

/// Errors from the ACP router.
#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    #[error("Unsupported agent: {0}")]
    UnsupportedAgent(String),
    #[error("Agent '{agent}' not installed. Install {command}.")]
    AgentNotInstalled { agent: String, command: String },
    #[error("Agent initialization timed out (10s)")]
    InitializeTimeout,
    #[error("Agent error: {0}")]
    Process(#[from] ProcessError),
}
