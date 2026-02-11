//! WebSocket terminal handler for PTY-backed shell sessions.
//!
//! Protocol:
//! - Client → Server: Text frames. If valid JSON with `type: "resize"`, handle as resize.
//!   Otherwise, write raw string to PTY stdin.
//! - Server → Client: Binary frames with raw PTY output bytes.
//! - One PTY per WebSocket connection. PTY killed on disconnect.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query,
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use std::io::{Read, Write};
use tokio::sync::mpsc;

/// Result type for spawning a PTY shell process.
type SpawnShellResult = (
    Box<dyn portable_pty::MasterPty + Send>,
    Box<dyn portable_pty::Child + Send + Sync>,
);

/// Query parameters for the terminal WebSocket endpoint.
#[derive(Debug, Deserialize)]
pub struct TerminalParams {
    /// Working directory to start the shell in.
    pub cwd: Option<String>,
}

/// Resize control message from the client.
#[derive(Debug, Deserialize)]
struct ResizeMessage {
    #[serde(rename = "type")]
    msg_type: String,
    rows: u16,
    cols: u16,
}

/// Default localhost origins allowed to open terminal WebSocket connections.
/// Browsers always send the `Origin` header on WebSocket handshakes, so checking
/// it blocks cross-origin attacks (e.g. malicious JS on evil.com connecting to
/// localhost:17010). Non-browser clients can spoof this, but they can already
/// spawn shells directly.
pub const ALLOWED_WS_ORIGINS: &[&str] = &[
    "http://localhost:5173",
    "http://localhost:5174",
    "http://localhost:5175",
    "http://localhost:17010",
    "http://127.0.0.1:5173",
    "http://127.0.0.1:5174",
    "http://127.0.0.1:5175",
    "http://127.0.0.1:17010",
];

/// Check if the Origin header is from an allowed origin.
fn is_origin_allowed(headers: &HeaderMap) -> bool {
    let origin = match headers.get("origin").and_then(|v| v.to_str().ok()) {
        Some(o) => o,
        // No Origin header — not a browser request (e.g. CLI WebSocket client).
        // Allow these through since they can already spawn shells directly.
        None => return true,
    };

    // Check custom origins from MANIFEST_CORS_ORIGINS first
    if let Ok(custom) = std::env::var("MANIFEST_CORS_ORIGINS") {
        return custom.split(',').any(|allowed| allowed.trim() == origin);
    }

    ALLOWED_WS_ORIGINS.iter().any(|allowed| *allowed == origin)
}

/// WebSocket endpoint: `/api/v1/terminal/ws?cwd=/path/to/dir`
///
/// Validates the Origin header to prevent cross-origin browser attacks,
/// then upgrades to WebSocket, spawns a PTY shell, and bridges I/O.
pub async fn ws_terminal_handler(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    Query(params): Query<TerminalParams>,
) -> Result<impl IntoResponse, StatusCode> {
    if !is_origin_allowed(&headers) {
        let origin = headers
            .get("origin")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown");
        tracing::warn!(origin = %origin, "Terminal WebSocket rejected: origin not allowed");
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(ws.on_upgrade(move |socket| handle_session(socket, params.cwd)))
}

async fn handle_session(socket: WebSocket, cwd: Option<String>) {
    let working_dir =
        cwd.unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/".to_string()));

    // Validate cwd against path restrictions
    let restrictions = crate::api::config::PathRestrictions::from_env();
    let path = std::path::Path::new(&working_dir);
    if !path.is_absolute() || restrictions.validate(path).is_err() {
        tracing::warn!(path = %working_dir, "Terminal session rejected: path not allowed");
        return;
    }

    tracing::info!(working_dir = %working_dir, "Starting terminal session");

    // Spawn PTY with user's shell
    let pty_result = spawn_shell(&working_dir);
    let (pty_master, mut child) = match pty_result {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Failed to spawn PTY: {}", e);
            return;
        }
    };

    // Bridge WebSocket <-> PTY
    if let Err(e) = bridge_ws_pty(socket, pty_master).await {
        tracing::warn!("Terminal session error: {}", e);
    }

    // Cleanup: wait for child process
    tracing::info!("Terminal session ending, waiting for child process");
    let _ = child.wait();
}

fn spawn_shell(working_dir: &str) -> Result<SpawnShellResult, anyhow::Error> {
    let pty_system = native_pty_system();

    let pair = pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    })?;

    // Use the user's preferred shell, falling back to zsh/powershell
    let shell = if cfg!(target_os = "windows") {
        "powershell".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "zsh".to_string())
    };

    let mut cmd = CommandBuilder::new(&shell);
    let path = std::path::Path::new(working_dir);
    if path.is_dir() {
        cmd.cwd(working_dir);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");

    let child = pair.slave.spawn_command(cmd)?;
    Ok((pair.master, child))
}

async fn bridge_ws_pty(
    socket: WebSocket,
    pty_master: Box<dyn portable_pty::MasterPty + Send>,
) -> Result<(), anyhow::Error> {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    let mut pty_reader = pty_master.try_clone_reader()?;
    let mut pty_writer = pty_master.take_writer()?;

    // Channel for PTY output -> WebSocket
    let (pty_tx, mut pty_rx) = mpsc::channel::<Vec<u8>>(256);
    // Channel for resize commands
    let (resize_tx, mut resize_rx) = mpsc::channel::<(u16, u16)>(8);

    // Task: Read PTY output (blocking) and forward to channel
    let pty_read_task = {
        let pty_tx = pty_tx.clone();
        tokio::spawn(async move {
            loop {
                let tx = pty_tx.clone();
                let result = tokio::task::spawn_blocking({
                    let mut reader = pty_reader;
                    move || {
                        let mut buf = vec![0u8; 4096];
                        match reader.read(&mut buf) {
                            Ok(0) => (None, reader),
                            Ok(n) => {
                                buf.truncate(n);
                                (Some(buf), reader)
                            }
                            Err(_) => (None, reader),
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
                    Ok((None, _)) | Err(_) => break,
                }
            }
        })
    };

    // Task: Handle resize commands
    let pty_master_for_resize = pty_master;
    let resize_task = tokio::spawn(async move {
        while let Some((cols, rows)) = resize_rx.recv().await {
            let _ = pty_master_for_resize.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    });

    // Task: Send PTY output as binary WS frames
    let ws_send_task = tokio::spawn(async move {
        while let Some(data) = pty_rx.recv().await {
            if ws_sender.send(Message::Binary(data.into())).await.is_err() {
                break;
            }
        }
    });

    // Main loop: Read from WebSocket and dispatch to PTY
    loop {
        match ws_receiver.next().await {
            Some(Ok(Message::Text(text))) => {
                // Try to parse as resize control message
                if let Ok(msg) = serde_json::from_str::<ResizeMessage>(&text) {
                    if msg.msg_type == "resize" {
                        let _ = resize_tx.send((msg.cols, msg.rows)).await;
                        continue;
                    }
                }
                // Otherwise, write as raw input to PTY
                if let Err(e) = pty_writer.write_all(text.as_bytes()) {
                    tracing::debug!("PTY write error: {}", e);
                    break;
                }
                let _ = pty_writer.flush();
            }
            Some(Ok(Message::Binary(data))) => {
                if let Err(e) = pty_writer.write_all(&data) {
                    tracing::debug!("PTY write error: {}", e);
                    break;
                }
                let _ = pty_writer.flush();
            }
            Some(Ok(Message::Close(_))) | None => break,
            Some(Ok(_)) => {} // Ping/Pong handled by axum
            Some(Err(e)) => {
                tracing::warn!("WebSocket error: {}", e);
                break;
            }
        }
    }

    pty_read_task.abort();
    resize_task.abort();
    ws_send_task.abort();

    Ok(())
}
