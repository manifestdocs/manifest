use std::path::PathBuf;
use std::sync::atomic::Ordering;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use manifest_core::config::ServerConfig;

use super::ApiError;
use crate::api::config::PathRestrictions;
use crate::api::AppState;

/// GET /api/v1/settings — returns current server configuration.
pub async fn get_settings() -> impl IntoResponse {
    let config = ServerConfig::load().unwrap_or_default();
    let config_file = ServerConfig::config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    // Resolve the effective database path for display
    let resolved = resolve_effective_db_path(&config);

    // Resolve default_agent: None → "claude"
    let default_agent = config.default_agent.as_deref().unwrap_or("claude");

    Json(serde_json::json!({
        "database_path": config.database_path,
        "database_path_resolved": resolved,
        "config_file": config_file,
        "default_agent": default_agent,
        "terminal_enabled": config.is_terminal_enabled(),
    }))
}

/// PUT /api/v1/settings — updates server configuration.
/// If the database path changes, triggers a server restart.
pub async fn update_settings(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<impl IntoResponse, ApiError> {
    let new_db_path = body
        .get("database_path")
        .and_then(|v| {
            if v.is_null() {
                Some(None)
            } else {
                v.as_str().map(|s| Some(s.to_string()))
            }
        })
        .unwrap_or(None);

    // Validate default_agent if provided
    const ALLOWED_AGENTS: &[&str] = &["claude", "gemini", "copilot", "codex"];
    let new_agent = if let Some(agent_val) = body.get("default_agent") {
        if agent_val.is_null() {
            Some(None) // Reset to default
        } else if let Some(agent) = agent_val.as_str() {
            if !ALLOWED_AGENTS.contains(&agent) {
                return Err(ApiError::from((
                    StatusCode::BAD_REQUEST,
                    format!(
                        "Invalid default_agent '{}'. Allowed values: {}",
                        agent,
                        ALLOWED_AGENTS.join(", ")
                    ),
                )));
            }
            Some(Some(agent.to_string()))
        } else {
            return Err(ApiError::from((
                StatusCode::BAD_REQUEST,
                "default_agent must be a string".to_string(),
            )));
        }
    } else {
        None // Not provided in request, leave unchanged
    };

    // Parse terminal_enabled if provided
    let new_terminal_enabled = if let Some(val) = body.get("terminal_enabled") {
        if val.is_null() {
            Some(None) // Reset to default (disabled)
        } else if let Some(enabled) = val.as_bool() {
            Some(Some(enabled))
        } else {
            return Err(ApiError::from((
                StatusCode::BAD_REQUEST,
                "terminal_enabled must be a boolean".to_string(),
            )));
        }
    } else {
        None // Not provided in request, leave unchanged
    };

    // Validate database path is not in a restricted directory
    if let Some(ref path) = new_db_path {
        let p = std::path::Path::new(path);
        // Validate the parent directory (the file itself may not exist yet)
        let validate_path = if p.exists() {
            p.to_path_buf()
        } else if let Some(parent) = p.parent() {
            if parent.exists() {
                parent.to_path_buf()
            } else {
                p.to_path_buf()
            }
        } else {
            p.to_path_buf()
        };
        let restrictions = PathRestrictions::from_env();
        if let Err(e) = restrictions.validate(&validate_path) {
            return Err(ApiError::from((
                StatusCode::BAD_REQUEST,
                format!("Invalid database path: {e}"),
            )));
        }
    }

    let mut config = ServerConfig::load().unwrap_or_default();
    let old_path = config.database_path.clone();

    // Update config if database_path was provided in the request
    let path_changed = if body.get("database_path").is_some() {
        let changed = old_path != new_db_path;
        config.database_path = new_db_path;
        changed
    } else {
        false
    };

    // Update default_agent if provided in the request
    if let Some(agent) = new_agent {
        config.default_agent = agent;
    }

    // Update terminal_enabled if provided in the request
    if let Some(enabled) = new_terminal_enabled {
        config.terminal_enabled = enabled;
        // Flip the runtime flag so it takes effect immediately (no restart needed)
        state
            .terminal_enabled
            .store(config.is_terminal_enabled(), Ordering::Relaxed);
    }

    config.save().map_err(|e| {
        tracing::error!("Failed to save config: {:?}", e);
        ApiError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to save configuration".to_string(),
        ))
    })?;

    let config_file = ServerConfig::config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let resolved = resolve_effective_db_path(&config);

    let default_agent = config.default_agent.as_deref().unwrap_or("claude");

    let response = serde_json::json!({
        "database_path": config.database_path,
        "database_path_resolved": resolved,
        "config_file": config_file,
        "default_agent": default_agent,
        "terminal_enabled": config.is_terminal_enabled(),
        "restart_required": path_changed,
    });

    // Schedule restart after responding if the DB path changed
    if path_changed {
        tokio::spawn(async {
            // Brief delay to let the response flush
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            super::super::trigger_shutdown();
        });
    }

    Ok(Json(response))
}

/// GET /api/v1/settings/mcp-status — check if the default agent has MCP configured.
pub async fn check_mcp_status() -> impl IntoResponse {
    let config = ServerConfig::load().unwrap_or_default();
    let agent = config
        .default_agent
        .as_deref()
        .unwrap_or("claude")
        .to_string();

    let agent_clone = agent.clone();
    let (configured, config_file, setup_hint) =
        tokio::task::spawn_blocking(move || match agent_clone.as_str() {
            "claude" => check_claude_mcp_config(),
            _ => (
                false,
                String::new(),
                format!("MCP config check not supported for agent '{agent_clone}'"),
            ),
        })
        .await
        .unwrap_or((false, String::new(), "Internal error".to_string()));

    Json(serde_json::json!({
        "agent": agent,
        "configured": configured,
        "config_file": config_file,
        "setup_hint": setup_hint,
    }))
}

/// POST /api/v1/settings/configure-mcp — auto-configure MCP for the default agent.
pub async fn configure_mcp() -> Result<impl IntoResponse, ApiError> {
    let config = ServerConfig::load().unwrap_or_default();
    let agent = config
        .default_agent
        .as_deref()
        .unwrap_or("claude")
        .to_string();

    // Filesystem I/O runs off the async runtime
    tokio::task::spawn_blocking(move || match agent.as_str() {
        "claude" => configure_claude_mcp(),
        _ => Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            format!("Auto-configure not supported for agent '{agent}'"),
        ))),
    })
    .await
    .map_err(|e| ApiError::from((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())))?
}

/// Check if Claude Code has a manifest MCP server configured.
///
/// Looks in two locations:
/// 1. `~/.claude/config.json` (global MCP config)
/// 2. `~/.claude/plugins/marketplace/*/plugins/manifest/.claude-plugin/plugin.json`
fn check_claude_mcp_config() -> (bool, String, String) {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            return (
                false,
                String::new(),
                "Could not determine home directory".into(),
            )
        }
    };

    let config_path = home.join(".claude").join("config.json");
    let config_path_str = config_path.display().to_string();

    // Check global config
    if has_manifest_mcp_entry(&config_path) {
        return (true, config_path_str, String::new());
    }

    // Check marketplace plugin directories
    let plugins_dir = home.join(".claude").join("plugins").join("marketplace");
    if let Ok(entries) = std::fs::read_dir(&plugins_dir) {
        for entry in entries.flatten() {
            let plugin_json = entry
                .path()
                .join("plugins")
                .join("manifest")
                .join(".claude-plugin")
                .join("plugin.json");
            if has_manifest_mcp_entry(&plugin_json) {
                return (true, plugin_json.display().to_string(), String::new());
            }
        }
    }

    let hint = format!(
        "Add manifest MCP server to {}. Click Configure to set this up automatically.",
        config_path_str
    );
    (false, config_path_str, hint)
}

/// Check if a JSON config file contains a manifest MCP server entry.
fn has_manifest_mcp_entry(path: &PathBuf) -> bool {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    let json: serde_json::Value = match serde_json::from_str(&contents) {
        Ok(j) => j,
        Err(_) => return false,
    };

    let servers = match json.get("mcpServers").and_then(|s| s.as_object()) {
        Some(s) => s,
        None => return false,
    };

    for (_name, server) in servers {
        // HTTP transport: url contains localhost:17010/mcp
        if let Some(url) = server.get("url").and_then(|u| u.as_str()) {
            if url.contains("localhost:17010/mcp") {
                return true;
            }
        }

        // Stdio transport (legacy): command is "manifest" and args contains "mcp"
        if let Some(cmd) = server.get("command").and_then(|c| c.as_str()) {
            if cmd == "manifest" || cmd.ends_with("/manifest") {
                if let Some(args) = server.get("args").and_then(|a| a.as_array()) {
                    if args.iter().any(|a| a.as_str() == Some("mcp")) {
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Write the manifest MCP entry into Claude Code's global config.
fn configure_claude_mcp() -> Result<impl IntoResponse, ApiError> {
    let home = dirs::home_dir().ok_or_else(|| {
        ApiError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not determine home directory".into(),
        ))
    })?;

    let config_path = home.join(".claude").join("config.json");

    // Read existing config or start with empty object
    let mut json: serde_json::Value = if config_path.exists() {
        let contents = std::fs::read_to_string(&config_path).map_err(|e| {
            ApiError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to read {}: {e}", config_path.display()),
            ))
        })?;
        serde_json::from_str(&contents).map_err(|e| {
            ApiError::from((
                StatusCode::BAD_REQUEST,
                format!("Invalid JSON in {}: {e}", config_path.display()),
            ))
        })?
    } else {
        serde_json::json!({})
    };

    // Ensure mcpServers object exists
    if json.get("mcpServers").is_none() {
        json["mcpServers"] = serde_json::json!({});
    }

    // Add/update the manifest entry
    json["mcpServers"]["manifest"] = serde_json::json!({
        "type": "http",
        "url": "http://localhost:17010/mcp"
    });

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ApiError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create directory: {e}"),
            ))
        })?;
    }

    // Write back
    let contents = serde_json::to_string_pretty(&json).map_err(|e| {
        ApiError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to serialize config: {e}"),
        ))
    })?;
    std::fs::write(&config_path, contents).map_err(|e| {
        ApiError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to write {}: {e}", config_path.display()),
        ))
    })?;

    Ok(Json(serde_json::json!({
        "success": true,
        "config_file": config_path.display().to_string(),
    })))
}

/// Resolve the effective database path given the current config and environment.
fn resolve_effective_db_path(config: &ServerConfig) -> String {
    // Check CLI/env override first
    if let Ok(db) = std::env::var("MANIFEST_DB") {
        return db;
    }

    // Config file
    if let Some(ref path) = config.database_path {
        return path.clone();
    }

    // Env var fallbacks
    if let Ok(data_dir) = std::env::var("MANIFEST_DATA_DIR") {
        return format!("{}/manifest.db", data_dir);
    }
    if let Ok(url) = std::env::var("DATABASE_URL") {
        return url;
    }

    // Platform default
    directories::ProjectDirs::from("", "", "manifest")
        .map(|dirs| dirs.data_dir().join("manifest.db").display().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
