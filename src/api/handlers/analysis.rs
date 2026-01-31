use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::path::Path;

use super::ApiError;
use crate::db::Database;
use crate::mcp::ProjectAnalysis;
use crate::serde_helpers::default_true;

fn default_depth() -> u32 {
    3
}

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
    let root = Path::new(&query.path);

    // Validate path is absolute (security requirement)
    if !root.is_absolute() {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            "Path must be absolute".to_string(),
        )));
    }

    // Validate path against security restrictions
    let restrictions = crate::api::config::PathRestrictions::from_env();
    if let Err(e) = restrictions.validate(root) {
        return Err(ApiError::from((
            StatusCode::FORBIDDEN,
            format!("Access denied: {}", e),
        )));
    }

    // Validate directory exists
    if !root.exists() {
        return Err(ApiError::from((
            StatusCode::NOT_FOUND,
            format!("Directory not found: {}", query.path),
        )));
    }
    if !root.is_dir() {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            format!("Path is not a directory: {}", query.path),
        )));
    }

    // Delegate to analysis module
    let analysis = crate::analysis::analyze(root, query.include_docs, query.max_depth);

    Ok(Json(analysis))
}
