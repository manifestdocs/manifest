//! Codebase analysis endpoint for project discovery.
//!
//! Scans a directory to detect languages, frameworks, and modules. Used by
//! AI agents before `plan_features` to understand what capabilities exist
//! in a codebase.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use super::ApiError;
use crate::db::Database;
use crate::mcp::ProjectAnalysis;
use crate::serde_helpers::default_true;

fn default_depth() -> u32 {
    3
}

const MAX_ANALYZE_DEPTH: u32 = 8;

#[derive(Debug, Deserialize)]
pub struct AnalyzeProjectQuery {
    /// Absolute path to the directory to analyze.
    pub path: String,
    /// Include documentation content (README, CLAUDE.md). Defaults to true.
    #[serde(default = "default_true")]
    pub include_docs: bool,
    /// Maximum directory depth to scan. Defaults to 3.
    #[serde(default = "default_depth")]
    pub max_depth: u32,
}

/// Analyze a codebase directory to discover project structure.
///
/// Returns detected language, frameworks, modules, and documentation.
/// Used by AI agents before plan_features to understand what capabilities exist.
pub async fn analyze_project(
    State(_db): State<Database>,
    Query(query): Query<AnalyzeProjectQuery>,
) -> Result<Json<ProjectAnalysis>, ApiError> {
    if query.max_depth > MAX_ANALYZE_DEPTH {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            format!("max_depth cannot exceed {}", MAX_ANALYZE_DEPTH),
        )));
    }

    let root = validate_analysis_root(&query.path)?;

    // Delegate to analysis module — runs synchronous I/O, so offload to blocking pool
    let root = root.to_path_buf();
    let include_docs = query.include_docs;
    let max_depth = query.max_depth;
    let analysis = tokio::task::spawn_blocking(move || {
        crate::analysis::analyze(&root, include_docs, max_depth)
    })
    .await
    .map_err(|e| ApiError::from((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())))?;

    Ok(Json(analysis))
}

fn validate_analysis_root(path: &str) -> Result<PathBuf, ApiError> {
    let root = Path::new(path);

    if !root.is_absolute() {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            "Path must be absolute".to_string(),
        )));
    }

    let restrictions = crate::api::config::PathRestrictions::from_env();
    if let Err(e) = restrictions.validate(root) {
        return Err(ApiError::from((
            StatusCode::FORBIDDEN,
            format!("Access denied: {}", e),
        )));
    }

    if !root.exists() {
        return Err(ApiError::from((
            StatusCode::NOT_FOUND,
            "Directory not found".to_string(),
        )));
    }
    if !root.is_dir() {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            "Path is not a directory".to_string(),
        )));
    }

    Ok(root.to_path_buf())
}
