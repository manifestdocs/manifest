//! Chat completions endpoint using headless Claude Code.
//!
//! Receives a ChatRequest (messages + context), spawns `claude -p` with
//! `--output-format stream-json`, and translates the output into SSE events
//! matching the StreamEvent format expected by the web client.

use async_stream::stream;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response, Sse},
    Json,
};
use serde::Deserialize;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::db::Database;

// ============================================================
// Request types (matches web client's ChatRequest)
// ============================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    #[allow(dead_code)]
    pub context: Option<ChatContext>,
    pub model: Option<String>,
    #[allow(dead_code)]
    pub stream: Option<bool>,
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ChatContext {
    pub feature_id: Option<String>,
    pub feature_title: Option<String>,
    pub feature_details: Option<String>,
    pub project_id: Option<String>,
    pub is_leaf: Option<bool>,
}

// ============================================================
// Handler
// ============================================================

/// Chat completions endpoint.
///
/// Accepts messages + optional feature context, spawns Claude CLI in headless
/// mode, and streams the response as SSE events.
pub async fn chat_completions(
    State(_db): State<Database>,
    Json(input): Json<ChatRequest>,
) -> Result<Response, (StatusCode, String)> {
    // Separate system messages from conversation messages
    let mut system_prompt = String::new();
    let mut conversation: Vec<&ChatMessage> = Vec::new();

    for msg in &input.messages {
        if msg.role == "system" {
            if !system_prompt.is_empty() {
                system_prompt.push_str("\n\n");
            }
            system_prompt.push_str(&msg.content);
        } else {
            conversation.push(msg);
        }
    }

    // Build the user prompt — when resuming a session, Claude already has
    // prior context so we only send the latest user message.
    let user_prompt = if input.session_id.is_some() {
        conversation
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default()
    } else {
        build_conversation_prompt(&conversation)
    };

    if user_prompt.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No user message provided".into()));
    }

    // Determine the model to use
    let model = input.model.as_deref().unwrap_or("sonnet");

    // Build CLI arguments.
    // The prompt is passed via stdin to avoid --tools "" consuming the positional arg.
    // --verbose is required for stream-json output with -p.
    //
    // We rely on the user's existing MCP configuration (manifest plugin) rather
    // than passing --mcp-config + --strict-mcp-config, because the Streamable HTTP
    // MCP handshake causes Claude CLI to hang during initialization.
    // Instead, --allowed-tools restricts which tools Claude can actually use.
    let mut args: Vec<String> = vec![
        "-p".into(),
        "--verbose".into(),
        "--model".into(),
        model.into(),
        "--output-format".into(),
        "stream-json".into(),
        "--tools".into(),
        "".to_string(), // Disable built-in tools (Read, Edit, Bash, etc.)
    ];

    // Always whitelist MCP tools so Claude can read/update features,
    // regardless of whether a slash command system prompt is present.
    args.extend([
        "--allowed-tools".into(),
        [
            "mcp__manifest__update_feature",
            "mcp__manifest__get_feature",
            "mcp__manifest__find_features",
            "mcp__manifest__list_projects",
        ]
        .join(","),
    ]);

    // Session support: resume existing or start new
    let new_session_id = if let Some(ref sid) = input.session_id {
        // Resuming an existing session
        args.extend(["--resume".into(), sid.clone()]);
        sid.clone()
    } else {
        // First turn: generate a new session ID
        let sid = uuid::Uuid::new_v4().to_string();
        args.extend(["--session-id".into(), sid.clone()]);
        sid
    };

    // Add system prompt if present
    if !system_prompt.is_empty() {
        args.push("--system-prompt".into());
        args.push(system_prompt);
    }

    tracing::info!("Spawning claude CLI with model={}", model);

    // Spawn claude in headless mode — prompt goes via stdin
    let mut child = Command::new("claude")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            tracing::error!("Failed to spawn claude: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to spawn claude: {}", e),
            )
        })?;

    // Write the prompt to stdin and close it
    let mut stdin = child.stdin.take().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to open stdin".into(),
        )
    })?;

    // Write prompt and close stdin in a background task
    tokio::spawn(async move {
        if let Err(e) = stdin.write_all(user_prompt.as_bytes()).await {
            tracing::error!("Failed to write to claude stdin: {}", e);
        }
        drop(stdin);
    });

    let stdout = child.stdout.take().ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to capture stdout".into(),
        )
    })?;

    // Log stderr in the background
    let stderr = child.stderr.take();
    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::warn!("claude stderr: {}", line);
            }
        });
    }

    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    // Transform Claude's stream-json into SSE events matching the client's StreamEvent format
    let stream = stream! {
        // Emit session ID immediately so the client can track it
        yield Ok::<_, std::convert::Infallible>(
            axum::response::sse::Event::default().data(
                serde_json::json!({
                    "type": "session",
                    "session_id": new_session_id
                })
                .to_string(),
            )
        );

        while let Ok(Some(line)) = lines.next_line().await {
            if line.is_empty() {
                continue;
            }

            let json: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let event_type = match json.get("type").and_then(|t| t.as_str()) {
                Some(t) => t,
                None => continue,
            };

            match event_type {
                "content_block_delta" => {
                    // Text streaming delta
                    if let Some(delta) = json.get("delta") {
                        if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                            let event = serde_json::json!({
                                "type": "content_block_delta",
                                "delta": { "type": "text_delta", "text": text }
                            });
                            yield Ok::<_, std::convert::Infallible>(
                                axum::response::sse::Event::default()
                                    .data(event.to_string())
                            );
                        }
                    }
                }
                "assistant" => {
                    // Complete assistant message — emit content blocks and tool uses
                    if let Some(content) = json.get("message")
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_array())
                    {
                        for item in content {
                            let item_type = item.get("type").and_then(|t| t.as_str());
                            match item_type {
                                Some("text") => {
                                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                        let event = serde_json::json!({
                                            "type": "content_block_delta",
                                            "delta": { "type": "text_delta", "text": text }
                                        });
                                        yield Ok::<_, std::convert::Infallible>(
                                            axum::response::sse::Event::default()
                                                .data(event.to_string())
                                        );
                                    }
                                }
                                Some("tool_use") => {
                                    let event = serde_json::json!({
                                        "type": "tool_call",
                                        "tool": {
                                            "id": item.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                                            "name": item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                                            "input": item.get("input").unwrap_or(&serde_json::Value::Null)
                                        }
                                    });
                                    yield Ok::<_, std::convert::Infallible>(
                                        axum::response::sse::Event::default()
                                            .data(event.to_string())
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "content_block_start" => {
                    // Check if this is a tool_use block starting
                    if let Some(cb) = json.get("content_block") {
                        if cb.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            let event = serde_json::json!({
                                "type": "tool_call",
                                "tool": {
                                    "id": cb.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                                    "name": cb.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                                    "input": {}
                                }
                            });
                            yield Ok::<_, std::convert::Infallible>(
                                axum::response::sse::Event::default()
                                    .data(event.to_string())
                            );
                        }
                    }
                }
                "result" | "message_stop" => {
                    yield Ok(
                        axum::response::sse::Event::default().data("[DONE]")
                    );
                    break;
                }
                _ => {}
            }
        }
    };

    Ok(Sse::new(stream).into_response())
}

// ============================================================
// Helpers
// ============================================================

/// Format conversation messages into a single prompt string for Claude's `-p` flag.
///
/// Multi-turn conversations are formatted as labeled turns so Claude can
/// distinguish prior context from the current request.
fn build_conversation_prompt(messages: &[&ChatMessage]) -> String {
    if messages.len() == 1 {
        return messages[0].content.clone();
    }

    let mut parts = Vec::new();
    for msg in messages {
        let label = match msg.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            _ => continue,
        };
        parts.push(format!("{}: {}", label, msg.content));
    }
    parts.join("\n\n")
}
