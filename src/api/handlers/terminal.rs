//! WebSocket terminal handler for connecting xterm.js to Claude Code via PTY.
//!
//! Protocol:
//! - Binary frames: Raw PTY stdin/stdout (efficient for terminal data)
//! - Text frames: JSON control messages (resize, shutdown, status)

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{Read, Write},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use super::ApiError;
use crate::db::Database;
use crate::models::ProjectId;

/// Terminal session state for tracking active connections.
#[derive(Debug)]
#[allow(dead_code)] // Fields used for debugging/future features
struct TerminalSession {
    project_id: Uuid,
    created_at: Instant,
    last_activity: Instant,
}

/// Shared state for terminal session management.
#[derive(Debug, Clone, Default)]
pub struct TerminalSessions {
    sessions: Arc<Mutex<HashMap<Uuid, TerminalSession>>>,
}

impl TerminalSessions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start background task to clean up zombie sessions.
    /// Kills sessions idle for more than 30 minutes.
    pub fn start_cleanup_task(self) {
        tokio::spawn(async move {
            let cleanup_interval = Duration::from_secs(300); // 5 minutes
            let idle_timeout = Duration::from_secs(1800); // 30 minutes

            loop {
                tokio::time::sleep(cleanup_interval).await;
                let mut sessions = self.sessions.lock().await;
                let now = Instant::now();
                sessions.retain(|id, session| {
                    let elapsed = now.duration_since(session.last_activity);
                    if elapsed > idle_timeout {
                        tracing::info!(session_id = %id, "Cleaning up idle terminal session");
                        false
                    } else {
                        true
                    }
                });
            }
        });
    }
}

/// Query parameters for WebSocket connection.
#[derive(Debug, Deserialize)]
pub struct TerminalQuery {
    /// Session ID for reconnection (optional, generates new if not provided)
    pub session_id: Option<Uuid>,
}

/// Control messages sent as JSON text frames.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ControlMessage {
    /// Resize the terminal
    Resize { cols: u16, rows: u16 },
    /// Request graceful shutdown
    Shutdown,
    /// Connection status update (server -> client)
    Status { connected: bool, message: String },
}

/// WebSocket terminal handler.
///
/// Upgrades HTTP to WebSocket, spawns Claude Code in PTY, bridges I/O.
pub async fn ws_terminal_handler(
    ws: WebSocketUpgrade,
    State(db): State<Database>,
    State(sessions): State<TerminalSessions>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<TerminalQuery>,
) -> Result<impl IntoResponse, ApiError> {
    // Verify project exists and get working directory
    // Directories are ordered by is_primary DESC, so first one is primary (or first by path)
    let directories = db
        .get_project_directories(ProjectId::from(project_id))
        .await
        .map_err(|e| {
            tracing::error!("Failed to get project directories: {}", e);
            ApiError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get project".to_string(),
            ))
        })?;

    let working_directory = directories
        .into_iter()
        .next() // First directory (primary if exists, otherwise first by path)
        .ok_or_else(|| {
            ApiError::from((
                StatusCode::NOT_FOUND,
                "Project has no directory configured".to_string(),
            ))
        })?;

    let session_id = query.session_id.unwrap_or_else(Uuid::new_v4);
    let working_dir = working_directory.path;

    Ok(ws.on_upgrade(move |socket| {
        handle_terminal_session(socket, sessions, project_id, session_id, working_dir)
    }))
}

async fn handle_terminal_session(
    socket: WebSocket,
    sessions: TerminalSessions,
    project_id: Uuid,
    session_id: Uuid,
    working_dir: String,
) {
    // Register session
    {
        let mut sessions_guard = sessions.sessions.lock().await;
        sessions_guard.insert(
            session_id,
            TerminalSession {
                project_id,
                created_at: Instant::now(),
                last_activity: Instant::now(),
            },
        );
    }

    tracing::info!(
        session_id = %session_id,
        project_id = %project_id,
        working_dir = %working_dir,
        "Starting terminal session"
    );

    // Spawn PTY with Claude Code
    let pty_result = spawn_claude_pty(&working_dir);
    let (pty_master, mut child) = match pty_result {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Failed to spawn PTY: {}", e);
            cleanup_session(&sessions, session_id).await;
            return;
        }
    };

    // Bridge WebSocket <-> PTY
    let result = bridge_websocket_pty(socket, pty_master, &sessions, session_id).await;
    if let Err(e) = result {
        tracing::warn!(session_id = %session_id, "Terminal session error: {}", e);
    }

    // Cleanup: wait for child process and remove session
    tracing::info!(session_id = %session_id, "Terminal session ending, waiting for child process");
    let _ = child.wait();
    cleanup_session(&sessions, session_id).await;
}

fn spawn_claude_pty(
    working_dir: &str,
) -> Result<
    (
        Box<dyn portable_pty::MasterPty + Send>,
        Box<dyn portable_pty::Child + Send + Sync>,
    ),
    anyhow::Error,
> {
    let pty_system = native_pty_system();

    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut cmd = CommandBuilder::new("claude");
    cmd.cwd(working_dir);

    // Set terminal environment
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    let child = pair.slave.spawn_command(cmd)?;

    Ok((pair.master, child))
}

async fn bridge_websocket_pty(
    socket: WebSocket,
    pty_master: Box<dyn portable_pty::MasterPty + Send>,
    sessions: &TerminalSessions,
    session_id: Uuid,
) -> Result<(), anyhow::Error> {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Get reader/writer from PTY
    let mut pty_reader = pty_master.try_clone_reader()?;
    let mut pty_writer = pty_master.take_writer()?;

    // Channel for PTY output -> WebSocket
    let (pty_tx, mut pty_rx) = mpsc::channel::<Vec<u8>>(256);

    // Channel for resize commands
    let (resize_tx, mut resize_rx) = mpsc::channel::<(u16, u16)>(8);

    // Task: Read from PTY and send to channel (blocking read in spawn_blocking)
    let pty_read_task = {
        let pty_tx = pty_tx.clone();
        tokio::spawn(async move {
            loop {
                // Use spawn_blocking for the blocking PTY read
                let tx = pty_tx.clone();
                let result = tokio::task::spawn_blocking({
                    let mut reader = pty_reader;
                    move || {
                        let mut buf = vec![0u8; 4096];
                        match reader.read(&mut buf) {
                            Ok(0) => {
                                // EOF
                                (None, reader)
                            }
                            Ok(n) => {
                                buf.truncate(n);
                                (Some(buf), reader)
                            }
                            Err(e) => {
                                tracing::debug!("PTY read error: {}", e);
                                (None, reader)
                            }
                        }
                    }
                })
                .await;

                match result {
                    Ok((Some(data), reader)) => {
                        pty_reader = reader;
                        if tx.send(data).await.is_err() {
                            break;
                        }
                    }
                    Ok((None, _)) => break,
                    Err(_) => break,
                }
            }
        })
    };

    // Task: Handle resize commands
    let pty_master_for_resize = pty_master;
    let resize_task = tokio::spawn(async move {
        while let Some((cols, rows)) = resize_rx.recv().await {
            if let Err(e) = pty_master_for_resize.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            }) {
                tracing::warn!("Failed to resize PTY: {}", e);
            }
        }
    });

    // Task: Send PTY output to WebSocket
    let ws_send_task = tokio::spawn(async move {
        while let Some(data) = pty_rx.recv().await {
            if ws_sender.send(Message::Binary(data.into())).await.is_err() {
                break;
            }
        }
    });

    // Main loop: Read from WebSocket and write to PTY
    let sessions_clone = sessions.clone();
    loop {
        match ws_receiver.next().await {
            Some(Ok(msg)) => {
                // Update session activity
                {
                    let mut sessions_guard = sessions_clone.sessions.lock().await;
                    if let Some(session) = sessions_guard.get_mut(&session_id) {
                        session.last_activity = Instant::now();
                    }
                }

                match msg {
                    Message::Binary(data) => {
                        // Raw input to PTY
                        if let Err(e) = pty_writer.write_all(&data) {
                            tracing::debug!("PTY write error: {}", e);
                            break;
                        }
                        let _ = pty_writer.flush();
                    }
                    Message::Text(text) => {
                        // Control message
                        match serde_json::from_str::<ControlMessage>(&text) {
                            Ok(ControlMessage::Resize { cols, rows }) => {
                                let _ = resize_tx.send((cols, rows)).await;
                            }
                            Ok(ControlMessage::Shutdown) => {
                                tracing::info!(session_id = %session_id, "Received shutdown request");
                                break;
                            }
                            Ok(ControlMessage::Status { .. }) => {
                                // Server -> client only, ignore if received
                            }
                            Err(e) => {
                                tracing::warn!("Invalid control message: {}", e);
                            }
                        }
                    }
                    Message::Close(_) => {
                        tracing::info!(session_id = %session_id, "WebSocket closed by client");
                        break;
                    }
                    Message::Ping(data) => {
                        // Handled automatically by axum
                        tracing::trace!("Received ping: {:?}", data);
                    }
                    Message::Pong(_) => {
                        // Ignore pongs
                    }
                }
            }
            Some(Err(e)) => {
                tracing::warn!(session_id = %session_id, "WebSocket error: {}", e);
                break;
            }
            None => {
                tracing::info!(session_id = %session_id, "WebSocket stream ended");
                break;
            }
        }
    }

    // Cleanup tasks
    pty_read_task.abort();
    resize_task.abort();
    ws_send_task.abort();

    Ok(())
}

async fn cleanup_session(sessions: &TerminalSessions, session_id: Uuid) {
    let mut sessions_guard = sessions.sessions.lock().await;
    sessions_guard.remove(&session_id);
    tracing::info!(session_id = %session_id, "Terminal session cleaned up");
}

// ============================================================
// Native Terminal Launcher (macOS)
// ============================================================

/// Request body for opening native terminal.
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // feature_id reserved for future use
pub struct OpenTerminalRequest {
    /// Optional feature ID to start Claude Code with feature context.
    pub feature_id: Option<Uuid>,
}

/// Response for terminal open operation.
#[derive(Debug, Serialize)]
pub struct OpenTerminalResponse {
    pub success: bool,
    pub directory: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Open native Terminal.app and launch Claude Code with feature context.
///
/// If feature_id is provided, Claude Code is started with the Manifest
/// start_feature tool to begin work on that feature.
pub async fn open_native_terminal(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(_input): Json<OpenTerminalRequest>,
) -> Result<Json<OpenTerminalResponse>, ApiError> {
    // Get project's primary directory
    let directories = db
        .get_project_directories(ProjectId::from(project_id))
        .await
        .map_err(|e| {
            tracing::error!("Failed to get project directories: {}", e);
            ApiError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to get project".to_string(),
            ))
        })?;

    let working_directory = directories.into_iter().next().ok_or_else(|| {
        ApiError::from((
            StatusCode::NOT_FOUND,
            "Project has no directory configured".to_string(),
        ))
    })?;

    let directory = working_directory.path;

    // Launch native Terminal.app (macOS only)
    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        // Create a temporary shell script that cd's and runs claude
        // Use login shell (-l) to ensure profile is loaded and TERM is set
        let script_content = format!(
            "#!/bin/zsh -l\nexport TERM=xterm-256color\ncd '{}'\nexec claude\n",
            directory.replace("'", "'\\''")
        );

        // Write to a temp file
        let temp_dir = std::env::temp_dir();
        let script_path = temp_dir.join(format!("manifest-claude-{}.sh", project_id));

        if let Err(e) = std::fs::write(&script_path, &script_content) {
            tracing::error!("Failed to write temp script: {}", e);
            return Ok(Json(OpenTerminalResponse {
                success: false,
                directory,
                error: Some(format!("Failed to write temp script: {}", e)),
            }));
        }

        // Make it executable
        let _ = Command::new("chmod")
            .args(["+x", script_path.to_str().unwrap_or("")])
            .output();

        // Open Terminal.app with the script
        let result = Command::new("open")
            .args(["-a", "Terminal", script_path.to_str().unwrap_or("")])
            .spawn();

        // Clean up temp script after a delay (fire and forget)
        let script_path_clone = script_path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let _ = tokio::fs::remove_file(script_path_clone).await;
        });

        match result {
            Ok(_) => {
                tracing::info!(
                    project_id = %project_id,
                    directory = %directory,
                    "Opened native terminal with Claude Code"
                );
                Ok(Json(OpenTerminalResponse {
                    success: true,
                    directory,
                    error: None,
                }))
            }
            Err(e) => {
                tracing::error!(project_id = %project_id, "Failed to open terminal: {}", e);
                Ok(Json(OpenTerminalResponse {
                    success: false,
                    directory,
                    error: Some(e.to_string()),
                }))
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        tracing::warn!(project_id = %project_id, "Native terminal not supported on this platform");
        Err(ApiError::from((
            StatusCode::NOT_IMPLEMENTED,
            "Native terminal launch only supported on macOS".to_string(),
        )))
    }
}
