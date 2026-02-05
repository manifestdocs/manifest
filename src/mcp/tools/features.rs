use std::str::FromStr;

use rmcp::{
    model::{CallToolResult, Content},
    ErrorData as McpError,
};

use crate::mcp::{
    tree_render,
    types::{
        CommitInfo, CompleteFeatureRequest, CreateFeatureRequest, DeleteFeatureRequest,
        FeatureInfo, FeatureInfoWithContext, FindFeaturesRequest, GetFeatureRequest,
        GetNextFeatureRequest, HistoryEntryInfo, PlanFeaturesRequest, RenderFeatureTreeRequest,
        StartFeatureRequest, UpdateFeatureRequest,
    },
    ManifestClient,
};

use super::format;
use crate::models::{
    CommitRef, CreateFeatureInput, FeatureId, FeatureState, UpdateFeatureInput, VersionId,
};

use super::spec::SpecConfig;

use super::client_err;

/// Find features by project, state, or search query.
pub async fn find_features(
    client: &ManifestClient,
    req: FindFeaturesRequest,
) -> Result<CallToolResult, McpError> {
    // Default limit of 50 when none specified
    let default_limit = 50u32;
    let effective_limit = req.limit.or(Some(default_limit));

    // If query is provided, use search; otherwise use list
    let features = if let Some(ref query) = req.query {
        client
            .search_features(query, req.project_id, effective_limit)
            .await
            .map_err(client_err)?
    } else {
        client
            .list_features(
                req.project_id,
                req.state.as_deref(),
                effective_limit,
                req.offset,
            )
            .await
            .map_err(client_err)?
    };

    let total = features.len();
    let was_capped = req.limit.is_none() && total as u32 == default_limit;

    // Render as markdown table
    let headers = &["ID", "State", "P", "Parent", "Title"];
    let rows: Vec<Vec<String>> = features
        .iter()
        .map(|f| {
            let id: uuid::Uuid = f.id.into();
            vec![
                format::short_id(&id),
                format::state_symbol(&f.state.as_str()).to_string(),
                f.priority.to_string(),
                f.parent_id
                    .map(|pid| {
                        let pid_uuid: uuid::Uuid = pid.into();
                        format::short_id(&pid_uuid)
                    })
                    .unwrap_or_default(),
                f.title.clone(),
            ]
        })
        .collect();

    let mut output = format::markdown_table(headers, &rows);

    if was_capped {
        output.push_str(&format!(
            "\nShowing first {} features. Use `limit` and `offset` to paginate.",
            default_limit
        ));
    }

    Ok(CallToolResult::success(vec![Content::text(output)]))
}

/// Get detailed information about a specific feature.
pub async fn get_feature(
    client: &ManifestClient,
    req: GetFeatureRequest,
) -> Result<CallToolResult, McpError> {
    let feature_id = client
        .resolve_feature_id(&req.feature_id, None)
        .await
        .map_err(client_err)?;

    let feature_with_context = client
        .get_feature_with_context(feature_id)
        .await
        .map_err(client_err)?;

    let mut feature_info: FeatureInfoWithContext = (&feature_with_context).into();

    // Apply LOD to breadcrumb: direct parent gets full details, more distant ancestors truncated
    feature_info.breadcrumb = format::lod_breadcrumb(&feature_info.breadcrumb, 1);

    // Optionally include history
    let history = if req.include_history {
        let history = client
            .get_feature_history(feature_id)
            .await
            .map_err(client_err)?;

        Some(
            history
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
                .collect(),
        )
    } else {
        None
    };

    let response = crate::mcp::types::GetFeatureResponse {
        feature: feature_info,
        history,
    };

    let yaml = format::to_yaml(&response).map_err(|e| McpError::internal_error(e, None))?;

    Ok(CallToolResult::success(vec![Content::text(yaml)]))
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
        details_summary: req.details_summary.map(Some),
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
    let feature_id = client
        .resolve_feature_id(&req.feature_id, None)
        .await
        .map_err(client_err)?;

    // Fetch the feature first so we can confirm what was deleted
    let feature = client.get_feature(feature_id).await.map_err(client_err)?;

    client
        .delete_feature(feature_id)
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
    let feature_id = client
        .resolve_feature_id(&req.feature_id, None)
        .await
        .map_err(client_err)?;

    // Get current feature with context (includes children)
    let feature_with_context = client
        .get_feature_with_context(feature_id)
        .await
        .map_err(client_err)?;

    // Fetch project settings for configurable guidance levels
    let project_id: uuid::Uuid = feature_with_context.feature.project_id.into();
    let project_with_dirs = client.get_project(project_id).await.map_err(client_err)?;
    let config = SpecConfig {
        detail_level: project_with_dirs.project.detail_level,
        ac_level: project_with_dirs.project.ac_level,
        ac_format: project_with_dirs.project.ac_format,
    };

    // Spec gate: analyze specification completeness
    let is_root = feature_with_context.feature.parent_id.is_none();
    let spec_status = super::spec::analyze_spec(
        feature_with_context.feature.details.as_deref(),
        !feature_with_context.children.is_empty(),
        is_root,
    );

    if spec_status.should_block() {
        let guidance = spec_status.guidance(&config).unwrap_or_default();
        return Ok(CallToolResult::error(vec![Content::text(format!(
            "Cannot start '{}' — specification required.\n\n{}",
            feature_with_context.feature.title, guidance
        ))]));
    }

    // Track whether this is a change request (implemented feature with desired_details)
    let has_change_request = feature_with_context.feature.desired_details.is_some();

    // Transition to in_progress if proposed, or if implemented with pending changes
    let should_transition = match feature_with_context.feature.state {
        FeatureState::Proposed => true,
        FeatureState::Implemented if feature_with_context.feature.desired_details.is_some() => true,
        _ => false,
    };
    if should_transition {
        client
            .update_feature(
                feature_id,
                &UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: None,
                    details_summary: None,
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
        .get_feature_with_context(feature_id)
        .await
        .map_err(client_err)?;

    let mut feature_info: FeatureInfoWithContext = (&feature_with_context).into();

    // Apply LOD to breadcrumb
    feature_info.breadcrumb = format::lod_breadcrumb(&feature_info.breadcrumb, 1);

    let response = crate::mcp::types::StartFeatureResponse {
        feature: feature_info,
        spec_status: spec_status.summary().to_string(),
        feature_tier: spec_status.tier.as_str().to_string(),
        ac_level: config.ac_level.as_str().to_string(),
        ac_format: config.ac_format.as_str().to_string(),
        detail_level: config.detail_level.as_str().to_string(),
        workflow_reminder: "IMPORTANT: When you finish this work, you MUST call complete_feature with a summary of what you did and any commit SHAs. This records history for future agents.".to_string(),
    };

    let yaml = format::to_yaml(&response).map_err(|e| McpError::internal_error(e, None))?;

    // Build content blocks
    let mut content = Vec::new();

    // If this is a change request, prepend guidance
    if has_change_request {
        content.push(Content::text(
            "This feature was previously implemented and has pending changes. \
             The `desired_details` field shows the desired spec. Compare with \
             `details` (current state) to understand what needs to change. \
             After implementing, update `details` to match what was built."
                .to_string(),
        ));
    }

    // If spec has warnings, prepend a warning text block
    if spec_status.has_warnings(&config) {
        let warning = spec_status.guidance(&config).unwrap_or_default();
        content.push(Content::text(format!("\u{26a0} {}", warning)));
    }

    content.push(Content::text(yaml));

    Ok(CallToolResult::success(content))
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
                        details_summary: None,
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

    let output = match result {
        Some(feature_ctx) => {
            // Fetch project settings for configurable guidance levels
            let project_id: uuid::Uuid = feature_ctx.feature.project_id.into();
            let project_with_dirs = client.get_project(project_id).await.map_err(client_err)?;
            let config = SpecConfig {
                detail_level: project_with_dirs.project.detail_level,
                ac_level: project_with_dirs.project.ac_level,
                ac_format: project_with_dirs.project.ac_format,
            };

            let is_root = feature_ctx.feature.parent_id.is_none();
            let spec_status = super::spec::analyze_spec(
                feature_ctx.feature.details.as_deref(),
                !feature_ctx.children.is_empty(),
                is_root,
            );

            let mut info: FeatureInfoWithContext = (&feature_ctx).into();

            // Apply LOD to breadcrumb
            info.breadcrumb = format::lod_breadcrumb(&info.breadcrumb, 1);

            let response = crate::mcp::types::NextFeatureResponse {
                feature: info,
                spec_status: spec_status.summary().to_string(),
                feature_tier: spec_status.tier.as_str().to_string(),
                ac_level: config.ac_level.as_str().to_string(),
                ac_format: config.ac_format.as_str().to_string(),
                detail_level: config.detail_level.as_str().to_string(),
                spec_guidance: spec_status.guidance(&config),
            };

            format::to_yaml(&response).map_err(|e| McpError::internal_error(e, None))?
        }
        None => "null".to_string(),
    };

    Ok(CallToolResult::success(vec![Content::text(output)]))
}
