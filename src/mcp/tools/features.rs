use std::str::FromStr;

use rmcp::{
    model::{CallToolResult, Content},
    ErrorData as McpError,
};

use crate::mcp::{
    tree_render,
    types::{
        CommitInfo, CompleteFeatureRequest, CreateFeatureRequest, DeleteFeatureRequest,
        FeatureInfo, FeatureInfoWithContext, FeatureListSummaryResponse, FeatureSummaryInfo,
        FindFeaturesRequest, GetFeatureRequest, GetNextFeatureRequest, HistoryEntryInfo,
        PlanFeaturesRequest, RenderFeatureTreeRequest, StartFeatureRequest, UpdateFeatureRequest,
    },
    ManifestClient,
};
use crate::models::{
    CommitRef, CreateFeatureInput, FeatureId, FeatureState, UpdateFeatureInput, VersionId,
};

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
                id: f.id.into(),
                title: f.title,
                state: f.state.as_str().to_string(),
                priority: f.priority,
                parent_id: f.parent_id.map(Into::into),
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
                id: h.id.into(),
                version_id: h.version_id.map(Into::into),
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
        .bulk_create_features(
            req.project_id,
            req.target_version_id,
            &req.features,
            req.confirm,
        )
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
                "Invalid state '{}'. Must be: proposed, in_progress, implemented, or archived",
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
                parent_id: req.parent_id.map(FeatureId::from),
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

/// Update any field on a feature.
/// This is a general-purpose tool that replaces narrow state-transition tools.
pub async fn update_feature(
    client: &ManifestClient,
    req: UpdateFeatureRequest,
) -> Result<CallToolResult, McpError> {
    // Parse state if provided
    let state = if let Some(ref state_str) = req.state {
        Some(FeatureState::from_str(state_str).map_err(|_| {
            McpError::invalid_params(
                format!(
                    "Invalid state '{}'. Must be: proposed, in_progress, implemented, or archived",
                    state_str
                ),
                None,
            )
        })?)
    } else {
        None
    };

    // Build the update input
    // Handle target_version_id: if clear_version is true, set to Some(None) to clear it
    // Otherwise, if target_version_id is provided, set to Some(Some(id))
    let target_version_id = if req.clear_version {
        Some(None) // Explicitly clear the version
    } else {
        req.target_version_id.map(|v| Some(VersionId::from(v))) // Set to provided value, or None if not provided
    };

    let input = UpdateFeatureInput {
        parent_id: req.parent_id.map(FeatureId::from),
        title: req.title,
        details: req.details,
        desired_details: req.desired_details.map(Some),
        state,
        priority: req.priority,
        target_version_id,
    };

    let feature = client
        .update_feature(req.feature_id, &input)
        .await
        .map_err(client_err)?;

    let result: FeatureInfo = (&feature).into();

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Permanently delete a feature and its descendants.
pub async fn delete_feature(
    client: &ManifestClient,
    req: DeleteFeatureRequest,
) -> Result<CallToolResult, McpError> {
    // Fetch the feature first so we can confirm what was deleted
    let feature = client
        .get_feature(req.feature_id)
        .await
        .map_err(client_err)?;

    client
        .delete_feature(req.feature_id)
        .await
        .map_err(client_err)?;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "Deleted feature '{}' ({})",
        feature.title, feature.id
    ))]))
}

/// Signal start of work on a feature.
/// Returns full context including breadcrumb with ancestor details.
/// Also transitions all proposed children to in_progress (cascading start).
/// Blocks if a leaf feature has no details; warns if details lack acceptance criteria.
pub async fn start_feature(
    client: &ManifestClient,
    req: StartFeatureRequest,
) -> Result<CallToolResult, McpError> {
    // Get current feature with context (includes children)
    let feature_with_context = client
        .get_feature_with_context(req.feature_id)
        .await
        .map_err(client_err)?;

    // Spec gate: analyze specification completeness
    let is_root = feature_with_context.feature.parent_id.is_none();
    let spec_status = super::spec::analyze_spec(
        feature_with_context.feature.details.as_deref(),
        !feature_with_context.children.is_empty(),
        is_root,
    );

    if spec_status.should_block() {
        let guidance = spec_status.guidance().unwrap_or_default();
        return Ok(CallToolResult::error(vec![Content::text(format!(
            "Cannot start '{}' — specification required.\n\n{}",
            feature_with_context.feature.title, guidance
        ))]));
    }

    // Transition to in_progress if proposed
    if feature_with_context.feature.state == FeatureState::Proposed {
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
            .map_err(client_err)?;
    }

    // Cascade: also start all proposed children
    start_children_recursive(client, &feature_with_context.children).await?;

    // Re-fetch context to get updated states
    let feature_with_context = client
        .get_feature_with_context(req.feature_id)
        .await
        .map_err(client_err)?;

    let result: FeatureInfoWithContext = (&feature_with_context).into();

    // Build response JSON with spec_status injected
    let mut result_json =
        serde_json::to_value(&result).map_err(|e| McpError::internal_error(e.to_string(), None))?;
    result_json["spec_status"] = serde_json::json!(spec_status.summary());
    result_json["feature_tier"] = serde_json::json!(spec_status.tier.as_str());

    let json = serde_json::to_string_pretty(&result_json)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    // If spec has warnings, prepend a warning text block
    if spec_status.has_warnings() {
        let warning = spec_status.guidance().unwrap_or_default();
        return Ok(CallToolResult::success(vec![
            Content::text(format!("⚠ {}", warning)),
            Content::text(json),
        ]));
    }

    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Recursively transition proposed children to in_progress.
async fn start_children_recursive(
    client: &ManifestClient,
    children: &[crate::models::FeatureSummaryContext],
) -> Result<(), McpError> {
    use futures_util::future::try_join_all;

    let update_futures: Vec<_> = children
        .iter()
        .filter(|child| child.state == FeatureState::Proposed)
        .map(|child| async move {
            let child_uuid: uuid::Uuid = child.id.into();
            // Update this child to in_progress
            client
                .update_feature(
                    child_uuid,
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
                .map_err(client_err)?;

            // Recursively start this child's children
            let child_context = client
                .get_feature_with_context(child_uuid)
                .await
                .map_err(client_err)?;

            Box::pin(start_children_recursive(client, &child_context.children)).await
        })
        .collect();

    try_join_all(update_futures).await?;
    Ok(())
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
    let history_id: uuid::Uuid = history.id.into();
    let result = serde_json::json!({
        "feature": feature_info,
        "history_entry": {
            "id": history_id,
            "summary": history.details.summary,
            "created_at": history.created_at.to_rfc3339()
        }
    });

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Get the next workable feature for a project.
/// Includes spec status so the agent knows if specification is needed first.
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
            let is_root = feature_ctx.feature.parent_id.is_none();
            let spec_status = super::spec::analyze_spec(
                feature_ctx.feature.details.as_deref(),
                !feature_ctx.children.is_empty(),
                is_root,
            );

            let info: FeatureInfoWithContext = (&feature_ctx).into();
            let mut result_json = serde_json::to_value(&info)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            result_json["spec_status"] = serde_json::json!(spec_status.summary());
            result_json["feature_tier"] = serde_json::json!(spec_status.tier.as_str());
            if let Some(guidance) = spec_status.guidance() {
                result_json["spec_guidance"] = serde_json::json!(guidance);
            }
            serde_json::to_string_pretty(&result_json)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?
        }
        None => "null".to_string(),
    };

    Ok(CallToolResult::success(vec![Content::text(json)]))
}
