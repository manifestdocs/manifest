//! Chat completions endpoint using ACP (Agent Client Protocol).
//!
//! Receives a ChatRequest (messages + context), delegates to the AcpRouter
//! which spawns and manages agent processes via JSON-RPC 2.0 over stdio,
//! and streams the response as SSE events matching the web client's format.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response, Sse},
    Json,
};
use serde::Deserialize;
use std::sync::LazyLock;

use manifest_core::config::ServerConfig;

use super::ApiError;
use crate::acp::AcpRouter;
use crate::db::Database;

/// Global ACP router instance (session pool + agent lifecycle).
static ACP_ROUTER: LazyLock<AcpRouter> = LazyLock::new(AcpRouter::new);

/// Start the idle session reaper. Call once on server startup.
pub fn start_session_reaper() {
    let router = &*ACP_ROUTER;
    let sessions = router.sessions_ref();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            // Build a temporary router view just for reaping
            AcpRouter::reap_idle_sessions(&sessions, std::time::Duration::from_secs(1800)).await;
        }
    });
}

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
    /// Model to use (e.g., "sonnet", "opus"). Reserved for future use.
    #[allow(dead_code)]
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
/// Accepts messages + optional feature context, delegates to the ACP router
/// which spawns the configured agent, and streams the response as SSE events.
pub async fn chat_completions(
    State(_db): State<Database>,
    Json(input): Json<ChatRequest>,
) -> Result<Response, ApiError> {
    // Resolve configured agent
    let agent = ServerConfig::load()
        .ok()
        .and_then(|c| c.default_agent)
        .unwrap_or_else(|| "claude".to_string());

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

    // Build the user prompt — when resuming a session, the agent already has
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

    tracing::info!("Chat completion via ACP agent={}", agent);

    // Delegate to the ACP router
    let (_, stream) = ACP_ROUTER
        .chat_completions(
            &agent,
            input.session_id.as_deref(),
            system_prompt,
            user_prompt,
        )
        .await
        .map_err(|e| {
            use crate::acp::router::RouterError;
            tracing::error!("ACP router error: {:?}", e);
            match &e {
                RouterError::UnsupportedAgent(_) => {
                    ApiError::from((StatusCode::BAD_REQUEST, e.to_string()))
                }
                RouterError::AgentNotInstalled { .. } => {
                    ApiError::from((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
                }
                RouterError::InitializeTimeout => {
                    ApiError::from((StatusCode::GATEWAY_TIMEOUT, e.to_string()))
                }
                RouterError::Process(_) => {
                    ApiError::from((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
                }
            }
        })?;

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
/// repeating the full specification (which the agent already has from turn 1).
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

/// Format conversation messages into a single prompt string.
///
/// Multi-turn conversations are formatted as labeled turns so the agent can
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
