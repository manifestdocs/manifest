//! Feature tree generation MCP tool.

use std::path::Path;

use rmcp::{
    model::{CallToolResult, Content},
    ErrorData as McpError,
};

use crate::analysis;
use crate::mcp::types::{
    GenerateFeatureTreeRequest, GenerateFeatureTreeResponse, GenerateFeatureTreeStats,
};

/// Generate a feature tree from codebase analysis.
pub async fn generate_feature_tree(
    req: GenerateFeatureTreeRequest,
) -> Result<CallToolResult, McpError> {
    let path = Path::new(&req.directory_path);

    // Validate path exists
    if !path.exists() {
        return Err(McpError::invalid_params(
            format!("Directory does not exist: {}", req.directory_path),
            None,
        ));
    }

    if !path.is_dir() {
        return Err(McpError::invalid_params(
            format!("Path is not a directory: {}", req.directory_path),
            None,
        ));
    }

    // Validate path is not in a restricted directory
    let restrictions = crate::api::config::PathRestrictions::from_env();
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Err(e) = restrictions.validate(&canonical) {
        return Err(McpError::invalid_params(
            format!("Path restricted: {e}"),
            None,
        ));
    }

    // Check if it's a git repository
    if !path.join(".git").exists() {
        return Err(McpError::invalid_params(
            format!(
                "Directory is not a git repository (no .git folder): {}",
                req.directory_path
            ),
            None,
        ));
    }

    // Get project name from directory
    let project_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Project")
        .to_string();

    // Generate the feature tree
    let (document, stats) = analysis::generate_feature_tree(
        path,
        &project_name,
        req.since.as_deref(),
        None, // No external symbol data
    );

    let response = GenerateFeatureTreeResponse {
        document,
        stats: GenerateFeatureTreeStats {
            total_chapters: stats.total_chapters,
            total_features: stats.total_features,
            implemented_count: stats.implemented_count,
            proposed_count: stats.proposed_count,
            deprecated_count: stats.deprecated_count,
            commits_analyzed: stats.commits_analyzed,
        },
        warnings: Vec::new(),
    };

    let json = serde_json::to_string_pretty(&response)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    let summary = format!(
        "Extracted {} features from codebase ({} commits analyzed)",
        stats.total_features, stats.commits_analyzed,
    );
    Ok(CallToolResult::success(vec![
        Content::text(summary),
        Content::text(json),
    ]))
}
