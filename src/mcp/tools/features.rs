use std::str::FromStr;

use rmcp::{
    model::{CallToolResult, Content},
    ErrorData as McpError,
};

use crate::mcp::{
    tree_render,
    types::{
        CommitInfo, CompleteFeatureRequest, CreateFeatureRequest, FeatureInfo,
        FeatureInfoWithContext, FeatureListSummaryResponse, FeatureSummaryInfo,
        FindFeaturesRequest, GetFeatureRequest, GetNextFeatureRequest, HistoryEntryInfo,
        PlanFeaturesRequest, RenderFeatureTreeRequest, StartFeatureRequest,
    },
    ManifestClient,
};
use crate::models::{CommitRef, CreateFeatureInput, FeatureState, UpdateFeatureInput};

use super::client_err;

/// Find features by project, state, or search query.
pub async fn find_features(
    client: &ManifestClient,
    req: FindFeaturesRequest,
) -> Result<CallToolResult, McpError> {
    // If query is provided, use search; otherwise use list
    let features = if let Some(ref query) = req.query {
        client
            .search_features(query, req.project_id, req.limit)
            .await
            .map_err(client_err)?
    } else {
        client
            .list_features(req.project_id, req.state.as_deref(), req.limit, req.offset)
            .await
            .map_err(client_err)?
    };

    let result = FeatureListSummaryResponse {
        features: features
            .into_iter()
            .map(|f| FeatureSummaryInfo {
                id: f.id,
                title: f.title,
                state: f.state.as_str().to_string(),
                priority: f.priority,
                parent_id: f.parent_id,
            })
            .collect(),
    };

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Get detailed information about a specific feature.
pub async fn get_feature(
    client: &ManifestClient,
    req: GetFeatureRequest,
) -> Result<CallToolResult, McpError> {
    let feature_with_context = client
        .get_feature_with_context(req.feature_id)
        .await
        .map_err(client_err)?;

    let feature_info: FeatureInfoWithContext = (&feature_with_context).into();

    // Convert to JSON Value so we can optionally add history
    let mut result = serde_json::to_value(&feature_info)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    // Optionally include history
    if req.include_history {
        let history = client
            .get_feature_history(req.feature_id)
            .await
            .map_err(client_err)?;

        let history_entries: Vec<HistoryEntryInfo> = history
            .into_iter()
            .map(|h| HistoryEntryInfo {
                id: h.id,
                version_id: h.version_id,
                version_name: None,
                summary: h.details.summary,
                commits: h
                    .details
                    .commits
                    .into_iter()
                    .map(|c| CommitInfo {
                        sha: c.sha,
                        message: c.message,
                        author: c.author,
                    })
                    .collect(),
                created_at: h.created_at.to_rfc3339(),
            })
            .collect();

        result["history"] = serde_json::to_value(history_entries)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    }

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Render a project's feature tree as ASCII art.
pub async fn render_feature_tree(
    client: &ManifestClient,
    req: RenderFeatureTreeRequest,
) -> Result<CallToolResult, McpError> {
    let tree = client
        .get_feature_tree(req.project_id)
        .await
        .map_err(client_err)?;

    let rendered = tree_render::render_tree_with_depth(&tree, req.max_depth);

    Ok(CallToolResult::success(vec![Content::text(rendered)]))
}

/// Turn a PRD, spec, or product vision into a feature tree.
pub async fn plan(
    client: &ManifestClient,
    req: PlanFeaturesRequest,
) -> Result<CallToolResult, McpError> {
    let response = client
        .bulk_create_features(req.project_id, &req.features, req.confirm)
        .await
        .map_err(client_err)?;

    let json = serde_json::to_string_pretty(&response)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Create a feature within a project.
pub async fn create_feature(
    client: &ManifestClient,
    req: CreateFeatureRequest,
) -> Result<CallToolResult, McpError> {
    let state = FeatureState::from_str(&req.state).map_err(|_| {
        McpError::invalid_params(
            format!(
                "Invalid state '{}'. Must be: proposed, in_progress, implemented, or deprecated",
                req.state
            ),
            None,
        )
    })?;

    let feature = client
        .create_feature(
            req.project_id,
            &CreateFeatureInput {
                id: None,
                parent_id: req.parent_id,
                title: req.title,
                details: req.details,
                state: Some(state),
                priority: req.priority,
                target_version_id: None,
            },
        )
        .await
        .map_err(client_err)?;

    let result: FeatureInfo = (&feature).into();

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Signal start of work on a feature.
pub async fn start_feature(
    client: &ManifestClient,
    req: StartFeatureRequest,
) -> Result<CallToolResult, McpError> {
    // Get current feature
    let feature = client
        .get_feature(req.feature_id)
        .await
        .map_err(client_err)?;

    // Transition to in_progress if proposed
    let feature = if feature.state == FeatureState::Proposed {
        client
            .update_feature(
                req.feature_id,
                &UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: None,
                    state: Some(FeatureState::InProgress),
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .map_err(client_err)?
    } else {
        feature
    };

    let result: FeatureInfo = (&feature).into();

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Complete work on a feature.
pub async fn complete_feature(
    client: &ManifestClient,
    req: CompleteFeatureRequest,
) -> Result<CallToolResult, McpError> {
    // Convert commits
    let commits: Vec<CommitRef> = req
        .commits
        .into_iter()
        .map(|c| CommitRef {
            sha: c.sha,
            message: c.message,
            author: c.author,
        })
        .collect();

    // Create history entry directly (no session)
    let history = client
        .create_feature_history(req.feature_id, &req.summary, &commits, req.mark_implemented)
        .await
        .map_err(client_err)?;

    // Get updated feature
    let feature = client
        .get_feature(req.feature_id)
        .await
        .map_err(client_err)?;

    let feature_info: FeatureInfo = (&feature).into();
    let result = serde_json::json!({
        "feature": feature_info,
        "history_entry": {
            "id": history.id,
            "summary": history.details.summary,
            "created_at": history.created_at.to_rfc3339()
        }
    });

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Get the next workable feature for a project.
pub async fn get_next_feature(
    client: &ManifestClient,
    req: GetNextFeatureRequest,
) -> Result<CallToolResult, McpError> {
    let result = client
        .get_next_feature(req.project_id, req.version_id)
        .await
        .map_err(client_err)?;

    let json = match result {
        Some(feature_ctx) => {
            let info: FeatureInfoWithContext = (&feature_ctx).into();
            serde_json::to_string_pretty(&info)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?
        }
        None => "null".to_string(),
    };

    Ok(CallToolResult::success(vec![Content::text(json)]))
}
