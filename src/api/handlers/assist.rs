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

use manifest_core::config::ServerConfig;

use super::ApiError;
use crate::db::Database;

// ============================================================
// Request types (matches web client's ChatRequest)
// ============================================================

/// Chat completions request from the web client.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    /// Conversation messages including system, user, and assistant turns.
    pub messages: Vec<ChatMessage>,
    /// Optional feature context to scope the conversation.
    pub context: Option<ChatContext>,
    /// Model to use (e.g., "sonnet", "opus"). Defaults to "sonnet".
    pub model: Option<String>,
    /// Whether to stream the response. Currently always true.
    #[allow(dead_code)]
    pub stream: Option<bool>,
    /// Session ID for multi-turn conversations. Omit for first turn.
    pub session_id: Option<String>,
}

/// A single message in a chat conversation.
#[derive(Debug, Deserialize)]
pub struct ChatMessage {
    /// Message role: "system", "user", or "assistant".
    pub role: String,
    /// The message text content.
    pub content: String,
}

/// Optional context scoping a chat conversation to a specific feature.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatContext {
    /// Feature being discussed, if any.
    pub feature_id: Option<String>,
    /// Title of the feature for display.
    pub feature_title: Option<String>,
    /// Full feature details/specification.
    pub feature_details: Option<String>,
    /// Project the feature belongs to.
    pub project_id: Option<String>,
    /// Whether the feature is a leaf node (can have sessions).
    pub is_leaf: Option<bool>,
    /// Pre-formatted version summary for plan view context.
    pub version_summary: Option<String>,
    /// Whether the chat is in the version/plan view.
    pub is_version_view: Option<bool>,
}

// ============================================================
// Constants
// ============================================================

/// Base system prompt for chat when no slash command provides a role definition.
const BASE_PREAMBLE: &str = "\
You are a product management assistant for Manifest, a feature documentation tool. \
This chat panel is for product management: writing specs, refining features, organizing \
release plans, and managing the feature tree. Use Manifest MCP tools to read and update \
features, projects, and versions.\
";

/// Behavioral guidelines appended to all system prompts (base preamble and slash commands).
const BEHAVIORAL_GUIDELINES: &str = "\
## Guidelines

PRODUCT MANAGEMENT SCOPE: This chat panel is for product management — feature specs, \
acceptance criteria, release planning, and feature tree organization. It is not a code \
editor or development environment.

IMPLEMENTATION REQUESTS: When a user asks you to \"implement\", \"build\", or \"code\" a feature:
1. Let them know this chat is for product management, not implementation
2. Offer to help prepare the feature for implementation instead — refine the spec, \
write acceptance criteria, suggest an implementation approach
3. Direct them to their terminal-based CLI agent or IDE for actual implementation, \
where they have full access to the codebase and development tools
Do not output code blocks pretending to create files. Do not generate project scaffolding \
(package.json, tsconfig, README, etc.). Short code snippets within a spec (interface \
definitions, usage examples) are fine.

SCOPE: Stay focused on the feature or version the user is viewing. Do not expand a single \
feature request into a project-wide plan unless asked.

FEATURE SETS: Feature sets (parents with children) do not have mutable state — their state is \
informational only. Never change a feature set's state. Never delete, reparent, or restructure \
a feature set's children unless the user explicitly asks you to.

WORKFLOW: If you call start_feature, follow through with complete_feature when done. \
When proposing spec changes, use update_feature with desired_details so the user sees \
a reviewable diff in the UI.

STYLE:
- Be direct. State what you will do, then do it. Do not ask \"Would you like me to...\" — act.
- No emoji in responses.
- Keep responses concise. Summarize tool results rather than dumping raw data.\
";

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
) -> Result<Response, ApiError> {
    // Check configured agent — only Claude is supported for now
    let agent = ServerConfig::load()
        .ok()
        .and_then(|c| c.default_agent)
        .unwrap_or_else(|| "claude".to_string());

    if agent != "claude" {
        let hint = match agent.as_str() {
            "gemini" => "Gemini CLI support coming soon",
            "copilot" => "Copilot CLI support coming soon",
            _ => "Unsupported agent",
        };
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            format!("{hint}. Switch to Claude in Settings to use chat."),
        )));
    }

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

    // Inject base preamble when no slash command provides a role definition.
    if system_prompt.is_empty() {
        system_prompt.push_str(BASE_PREAMBLE);
    }

    // Inject feature context: full details on first turn, lightweight anchor on resumed turns.
    if let Some(ref ctx) = input.context {
        let context_block = if input.session_id.is_none() {
            build_feature_context(ctx)
        } else {
            build_feature_anchor(ctx)
        };

        if !context_block.is_empty() {
            if !system_prompt.is_empty() {
                system_prompt.push_str("\n\n");
            }
            system_prompt.push_str(&context_block);
        }

        // Inject version context for plan view
        let version_block = build_version_context(ctx);
        if !version_block.is_empty() {
            if !system_prompt.is_empty() {
                system_prompt.push_str("\n\n");
            }
            system_prompt.push_str(&version_block);
        }
    }

    // Always append behavioral guidelines (applies to both base preamble and slash commands)
    system_prompt.push_str("\n\n");
    system_prompt.push_str(BEHAVIORAL_GUIDELINES);

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
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            "No user message provided".to_string(),
        )));
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

    // Whitelist all Manifest MCP tools so the chat agent has full access.
    args.extend([
        "--allowed-tools".into(),
        [
            "mcp__manifest__list_projects",
            "mcp__manifest__find_features",
            "mcp__manifest__get_feature",
            "mcp__manifest__render_feature_tree",
            "mcp__manifest__init_project",
            "mcp__manifest__add_project_directory",
            "mcp__manifest__generate_feature_tree",
            "mcp__manifest__plan",
            "mcp__manifest__create_feature",
            "mcp__manifest__start_feature",
            "mcp__manifest__complete_feature",
            "mcp__manifest__update_feature",
            "mcp__manifest__delete_feature",
            "mcp__manifest__get_next_feature",
            "mcp__manifest__list_versions",
            "mcp__manifest__create_version",
            "mcp__manifest__set_feature_version",
            "mcp__manifest__release_version",
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
            ApiError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to spawn claude: {}", e),
            ))
        })?;

    // Write the prompt to stdin and close it
    let mut stdin = child.stdin.take().ok_or_else(|| {
        ApiError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to open stdin".to_string(),
        ))
    })?;

    // Write prompt and close stdin in a background task
    tokio::spawn(async move {
        if let Err(e) = stdin.write_all(user_prompt.as_bytes()).await {
            tracing::error!("Failed to write to claude stdin: {}", e);
        }
        drop(stdin);
    });

    let stdout = child.stdout.take().ok_or_else(|| {
        ApiError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to capture stdout".to_string(),
        ))
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

/// Build a system prompt fragment from feature context.
///
/// Gives the AI clear awareness of which feature the user is viewing so it
/// stays on-topic and uses the correct feature ID for tool calls.
fn build_feature_context(ctx: &ChatContext) -> String {
    let title = match ctx.feature_title.as_deref() {
        Some(t) if !t.is_empty() => t,
        _ => return String::new(),
    };

    let mut parts = Vec::new();
    parts.push("## Active Feature Context\n".to_string());
    parts.push(format!(
        "The user is viewing the feature **\"{}\"**.",
        title
    ));

    if let Some(ref id) = ctx.feature_id {
        parts.push(format!("Feature ID: `{}`", id));
    }
    if let Some(ref pid) = ctx.project_id {
        parts.push(format!("Project ID: `{}`", pid));
    }
    if let Some(is_leaf) = ctx.is_leaf {
        if is_leaf {
            parts.push("Type: leaf feature".to_string());
        } else {
            parts.push("Type: feature set (has children)".to_string());
            parts.push(
                "IMPORTANT: Feature sets do not have mutable state. Do NOT change this feature's \
                 state. Do NOT delete, reparent, or restructure its children unless the user \
                 explicitly asks. You may only update its shared context (details/desired_details)."
                    .to_string(),
            );
        }
    }

    if let Some(ref details) = ctx.feature_details {
        if !details.is_empty() {
            parts.push(format!(
                "\nCurrent specification:\n<feature-spec>\n{}\n</feature-spec>",
                details
            ));
        }
    }

    parts.push(
        "\nWhen using Manifest tools, operate on this feature unless the user \
         explicitly asks about a different feature or project."
            .to_string(),
    );

    parts.join("\n")
}

/// Build a lightweight anchor for resumed turns.
///
/// Provides just the feature title and IDs so the AI stays scoped without
/// repeating the full specification (which Claude already has from turn 1).
fn build_feature_anchor(ctx: &ChatContext) -> String {
    let title = match ctx.feature_title.as_deref() {
        Some(t) if !t.is_empty() => t,
        _ => return String::new(),
    };

    let mut parts = Vec::new();
    parts.push("## Active Feature".to_string());
    parts.push(format!("Feature: **\"{}\"**", title));

    if let Some(ref id) = ctx.feature_id {
        parts.push(format!("Feature ID: `{}`", id));
    }
    if let Some(ref pid) = ctx.project_id {
        parts.push(format!("Project ID: `{}`", pid));
    }

    parts.push(
        "Stay focused on this feature. Use the IDs above for any Manifest tool calls. \
         Only switch context if the user explicitly asks about a different feature or project."
            .to_string(),
    );

    parts.join("\n")
}

/// Build a system prompt fragment from version context in plan view.
///
/// Gives the AI awareness of the release planning context so it can provide
/// version-aware analysis and recommendations.
fn build_version_context(ctx: &ChatContext) -> String {
    if ctx.is_version_view != Some(true) {
        return String::new();
    }

    let mut parts = Vec::new();
    parts.push("## Release Planning Context\n".to_string());
    parts.push("The user is in the Plan view, managing versions and release planning.".to_string());

    if let Some(ref summary) = ctx.version_summary {
        if !summary.is_empty() {
            parts.push(format!("\nVersion status:\n{}", summary));
        }
    }

    if let Some(ref pid) = ctx.project_id {
        parts.push(format!("\nProject ID: `{}`", pid));
    }

    parts.push(
        "\nUse Manifest MCP tools (list_versions, find_features, set_feature_version) \
         to gather data and make changes. Present analysis before acting."
            .to_string(),
    );

    parts.join("\n")
}

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
