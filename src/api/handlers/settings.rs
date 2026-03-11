//! Server configuration management endpoints.
//!
//! Exposes the current server settings (database path, default agent) and
//! supports partial updates. Changing the database path triggers a server
//! restart. Path restrictions prevent writing to sensitive directories.

use std::path::PathBuf;

use axum::{http::StatusCode, response::IntoResponse, Json};
use manifest_core::config::ServerConfig;
use serde::Deserialize;
use validator::Validate;

use super::ApiError;
use crate::api::config::PathRestrictions;
use crate::api::validation::ValidatedJson;

const ALLOWED_AGENTS: &[&str] = &["claude", "gemini", "copilot", "codex"];

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateSettingsInput {
    /// Optional database path update. Uses double-option to distinguish:
    /// - `None`: field omitted (no change)
    /// - `Some(None)`: clear configured value
    /// - `Some(Some(path))`: set value
    #[serde(default)]
    pub database_path: Option<Option<String>>,
    /// Optional default agent update. Uses double-option to support clearing.
    #[serde(default)]
    pub default_agent: Option<Option<String>>,
}

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
    }))
}

/// PUT /api/v1/settings — updates server configuration.
/// If the database path changes, triggers a server restart.
pub async fn update_settings(
    ValidatedJson(input): ValidatedJson<UpdateSettingsInput>,
) -> Result<impl IntoResponse, ApiError> {
    validate_database_path_update(input.database_path.as_ref())?;
    validate_default_agent_update(input.default_agent.as_ref())?;

    let mut config = ServerConfig::load().unwrap_or_default();
    let path_changed = apply_settings_updates(&mut config, input);

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
    let check_result = tokio::task::spawn_blocking(move || match agent_clone.as_str() {
        "claude" => check_claude_mcp_config(),
        _ => (
            false,
            String::new(),
            format!("MCP config check not supported for agent '{agent_clone}'"),
        ),
    })
    .await;

    let (configured, config_file, setup_hint) = match check_result {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Failed to check MCP configuration: {}", e);
            (
                false,
                String::new(),
                "Could not check MCP configuration".to_string(),
            )
        }
    };

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
    .map_err(|e| {
        tracing::error!("Failed to configure MCP: {}", e);
        ApiError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to configure MCP".to_string(),
        ))
    })?
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

    servers.values().any(is_manifest_mcp_server)
}

/// Check if a single MCP server entry points to Manifest.
fn is_manifest_mcp_server(server: &serde_json::Value) -> bool {
    // HTTP transport: url contains localhost:17010/mcp
    if let Some(url) = server.get("url").and_then(|u| u.as_str()) {
        if url.contains("localhost:17010/mcp") {
            return true;
        }
    }

    // Stdio transport (legacy): command is "manifest" and args contains "mcp"
    let cmd = match server.get("command").and_then(|c| c.as_str()) {
        Some(c) if c == "manifest" || c.ends_with("/manifest") => c,
        _ => return false,
    };
    let _ = cmd; // used only for the guard above
    server
        .get("args")
        .and_then(|a| a.as_array())
        .is_some_and(|args| args.iter().any(|a| a.as_str() == Some("mcp")))
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
            tracing::error!(
                path = %config_path.display(),
                error = %e,
                "Failed to read Claude config"
            );
            ApiError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to read Claude configuration".to_string(),
            ))
        })?;
        serde_json::from_str(&contents).map_err(|e| {
            tracing::warn!(
                path = %config_path.display(),
                error = %e,
                "Invalid JSON in Claude config"
            );
            ApiError::from((
                StatusCode::BAD_REQUEST,
                "Claude configuration file contains invalid JSON".to_string(),
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
            tracing::error!(path = %parent.display(), error = %e, "Failed to create config directory");
            ApiError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to prepare Claude configuration directory".to_string(),
            ))
        })?;
    }

    // Write back
    let contents = serde_json::to_string_pretty(&json).map_err(|e| {
        tracing::error!("Failed to serialize Claude config: {}", e);
        ApiError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to serialize Claude configuration".to_string(),
        ))
    })?;
    std::fs::write(&config_path, contents).map_err(|e| {
        tracing::error!(
            path = %config_path.display(),
            error = %e,
            "Failed to write Claude config"
        );
        ApiError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to write Claude configuration".to_string(),
        ))
    })?;

    Ok(Json(serde_json::json!({
        "success": true,
        "config_file": config_path.display().to_string(),
    })))
}

fn apply_settings_updates(config: &mut ServerConfig, input: UpdateSettingsInput) -> bool {
    let old_path = config.database_path.clone();

    if let Some(database_path) = input.database_path {
        config.database_path = database_path;
    }
    if let Some(default_agent) = input.default_agent {
        config.default_agent = default_agent;
    }

    old_path != config.database_path
}

fn validate_database_path_update(database_path: Option<&Option<String>>) -> Result<(), ApiError> {
    let Some(Some(path)) = database_path else {
        return Ok(());
    };

    let validate_path = path_for_restriction_check(path);
    let restrictions = PathRestrictions::from_env();
    if let Err(e) = restrictions.validate(&validate_path) {
        tracing::warn!(path, error = %e, "Rejected database path update");
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            "Invalid database path".to_string(),
        )));
    }

    Ok(())
}

fn validate_default_agent_update(default_agent: Option<&Option<String>>) -> Result<(), ApiError> {
    let Some(Some(agent)) = default_agent else {
        return Ok(());
    };

    if !ALLOWED_AGENTS.contains(&agent.as_str()) {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            format!(
                "default_agent must be one of: {}",
                ALLOWED_AGENTS.join(", ")
            ),
        )));
    }

    Ok(())
}

fn path_for_restriction_check(path: &str) -> PathBuf {
    let p = std::path::Path::new(path);
    if p.exists() {
        return p.to_path_buf();
    }

    if let Some(parent) = p.parent() {
        if parent.exists() {
            return parent.to_path_buf();
        }
    }

    p.to_path_buf()
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
