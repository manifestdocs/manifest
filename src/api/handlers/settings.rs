use axum::{http::StatusCode, response::IntoResponse, Json};
use manifest_core::config::ServerConfig;

use super::ApiError;
use crate::api::config::PathRestrictions;

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
    const ALLOWED_AGENTS: &[&str] = &["claude", "gemini", "copilot"];
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
