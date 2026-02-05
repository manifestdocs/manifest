//! Agent lifecycle management.
//!
//! Wraps the transport with ACP protocol lifecycle:
//! initialize -> session/new -> prompt -> shutdown.

use tokio::sync::broadcast;

use super::registry::AgentSpawnConfig;
use super::transport::{AgentTransport, TransportError};
use super::types::{
    ClientInfo, ContentBlock, InitializeParams, InitializeResult, JsonRpcNotification,
    NewSessionParams, NewSessionResult, PromptParams, PromptResult, SessionUpdate,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// An active ACP agent process with an established session.
pub struct AgentProcess {
    transport: AgentTransport,
    session_id: String,
    agent_name: String,
    last_activity: tokio::time::Instant,
}

impl AgentProcess {
    /// Spawn a new agent, run the ACP initialize + session/new handshake.
    pub async fn start(config: AgentSpawnConfig, agent_name: &str) -> Result<Self, ProcessError> {
        let transport = AgentTransport::spawn(config.command, &config.args)
            .map_err(|e| ProcessError::Transport(e))?;

        // Step 1: initialize
        let init_params = InitializeParams {
            protocol_version: 1,
            client_info: ClientInfo {
                name: "manifest-server".to_string(),
                version: VERSION.to_string(),
            },
            client_capabilities: serde_json::json!({}),
        };

        let init_result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            transport.request(
                "initialize",
                Some(serde_json::to_value(&init_params).unwrap()),
            ),
        )
        .await
        .map_err(|_| ProcessError::InitializeTimeout)?
        .map_err(ProcessError::Transport)?;

        let _init: InitializeResult = serde_json::from_value(init_result)
            .map_err(|e| ProcessError::Protocol(format!("Bad initialize result: {}", e)))?;

        tracing::info!("ACP: {} initialized", agent_name);

        // Step 2: session/new
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "/".to_string());
        let session_params = NewSessionParams {
            cwd,
            mcp_servers: vec![],
        };

        let params_json = serde_json::to_value(&session_params).unwrap();
        tracing::debug!("ACP session/new params: {}", params_json);

        let session_result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            transport.request("session/new", Some(params_json)),
        )
        .await
        .map_err(|_| ProcessError::SessionTimeout)?
        .map_err(ProcessError::Transport)?;

        let session: NewSessionResult = serde_json::from_value(session_result)
            .map_err(|e| ProcessError::Protocol(format!("Bad session/new result: {}", e)))?;

        tracing::info!(
            "ACP: {} session established: {}",
            agent_name,
            session.session_id
        );

        Ok(Self {
            transport,
            session_id: session.session_id,
            agent_name: agent_name.to_string(),
            last_activity: tokio::time::Instant::now(),
        })
    }

    /// The agent's session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The agent name (e.g. "claude", "gemini").
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    /// When the agent was last active.
    pub fn last_activity(&self) -> tokio::time::Instant {
        self.last_activity
    }

    /// Touch the last activity timestamp.
    pub fn touch(&mut self) {
        self.last_activity = tokio::time::Instant::now();
    }

    /// Send a prompt and return a notification receiver for streaming updates.
    ///
    /// The caller should:
    /// 1. Subscribe to the returned receiver for `session/update` notifications
    /// 2. Wait for the prompt future to resolve (indicates prompt is complete)
    pub async fn prompt(
        &mut self,
        content: Vec<ContentBlock>,
    ) -> Result<PromptHandle, ProcessError> {
        self.last_activity = tokio::time::Instant::now();

        let notification_rx = self.transport.subscribe();

        let params = PromptParams {
            session_id: self.session_id.clone(),
            prompt: content,
        };

        // Send the prompt request. The response comes after the agent finishes,
        // while session/update notifications stream during processing.
        let result = self
            .transport
            .request(
                "session/prompt",
                Some(serde_json::to_value(&params).unwrap()),
            )
            .await
            .map_err(ProcessError::Transport)?;

        let prompt_result: PromptResult = serde_json::from_value(result)
            .map_err(|e| ProcessError::Protocol(format!("Bad prompt result: {}", e)))?;

        Ok(PromptHandle {
            notification_rx,
            prompt_result,
        })
    }

    /// Send a prompt request in a non-blocking way, returning immediately.
    /// Returns a notification receiver for streaming and a join handle for the response.
    pub fn prompt_streaming(
        &mut self,
        content: Vec<ContentBlock>,
    ) -> (
        broadcast::Receiver<JsonRpcNotification>,
        tokio::task::JoinHandle<Result<PromptResult, ProcessError>>,
    ) {
        self.last_activity = tokio::time::Instant::now();

        let notification_rx = self.transport.subscribe();

        let params = PromptParams {
            session_id: self.session_id.clone(),
            prompt: content,
        };

        tracing::debug!(
            "ACP session/prompt params: {}",
            serde_json::to_value(&params).unwrap()
        );

        let transport_write_tx = self.transport.clone_write_tx();
        let transport_pending = self.transport.clone_pending();
        let next_id = self.transport.clone_next_id();

        // We need to send the request and listen for the response.
        // Use a dedicated task so we can return immediately.
        let handle = tokio::spawn(async move {
            // Build and send the request manually to avoid borrowing &self
            let id = next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let request_id = super::types::RequestId::Number(id);
            let req = super::types::JsonRpcRequest::new(
                request_id.clone(),
                "session/prompt",
                Some(serde_json::to_value(&params).unwrap()),
            );
            let json = serde_json::to_string(&req)
                .map_err(|e| ProcessError::Transport(TransportError::Serialize(e)))?;

            let (tx, rx) = tokio::sync::oneshot::channel();
            {
                let mut pending = transport_pending.lock().await;
                pending.insert(request_id.to_string(), tx);
            }

            transport_write_tx
                .send(json)
                .await
                .map_err(|_| ProcessError::Transport(TransportError::WriterClosed))?;

            // Wait for the response (with a generous timeout for long prompts)
            let response = tokio::time::timeout(std::time::Duration::from_secs(600), rx)
                .await
                .map_err(|_| {
                    ProcessError::Transport(TransportError::Timeout {
                        method: "session/prompt".to_string(),
                    })
                })?
                .map_err(|_| ProcessError::Transport(TransportError::ResponseDropped))?;

            if let Some(err) = response.error {
                return Err(ProcessError::Transport(TransportError::RpcError(err)));
            }

            let result: PromptResult =
                serde_json::from_value(response.result.unwrap_or(serde_json::Value::Null))
                    .map_err(|e| ProcessError::Protocol(format!("Bad prompt result: {}", e)))?;

            Ok(result)
        });

        (notification_rx, handle)
    }

    /// Cancel the current prompt.
    pub async fn cancel(&self) -> Result<(), ProcessError> {
        self.transport
            .notify(
                "session/cancel",
                Some(serde_json::json!({ "session_id": self.session_id })),
            )
            .await
            .map_err(ProcessError::Transport)
    }

    /// Kill the agent process.
    pub async fn shutdown(self) -> Result<(), ProcessError> {
        tracing::info!("ACP: shutting down {} agent", self.agent_name);
        self.transport.kill().await.map_err(ProcessError::Transport)
    }
}

/// Result of a completed prompt.
pub struct PromptHandle {
    /// Receiver for session/update notifications that arrived during the prompt.
    pub notification_rx: broadcast::Receiver<JsonRpcNotification>,
    /// The final prompt result.
    pub prompt_result: PromptResult,
}

impl PromptHandle {
    /// Consume remaining notifications and extract session updates.
    pub fn drain_updates(&mut self) -> Vec<SessionUpdate> {
        let mut updates = Vec::new();
        while let Ok(notif) = self.notification_rx.try_recv() {
            if notif.method == "session/update" {
                if let Some(params) = &notif.params {
                    if let Some(update) = SessionUpdate::from_notification_params(params) {
                        updates.push(update);
                    }
                }
            }
        }
        updates
    }
}

/// Errors from agent process management.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("Initialize timed out (10s)")]
    InitializeTimeout,
    #[error("Session creation timed out (10s)")]
    SessionTimeout,
    #[error("Protocol error: {0}")]
    Protocol(String),
}
