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
        GetNextFeatureRequest, HistoryEntryInfo, PlanFeaturesRequest, ProveFeatureRequest,
        RecordVerificationRequest, RenderFeatureTreeRequest, StartFeatureRequest,
        UpdateFeatureRequest, VerifyFeatureRequest,
    },
    ManifestClient,
};

use chrono::Utc;

use super::format;
use crate::models::{
    CommitRef, CreateFeatureInput, Feature, FeatureId, FeatureState, UpdateFeatureInput, VersionId,
};

use super::spec::SpecConfig;

use super::client_err;

/// Check if an in_progress feature is stale (no update for >24h).
/// Uses `claimed_at` when available (more accurate — not reset by metadata changes),
/// falls back to `updated_at` for features claimed before this field existed.
pub(crate) fn stale_warning(feature: &Feature) -> Option<String> {
    if feature.state != FeatureState::InProgress {
        return None;
    }
    let reference_time = feature.claimed_at.unwrap_or(feature.updated_at);
    let elapsed = Utc::now() - reference_time;
    if elapsed > chrono::Duration::hours(24) {
        let hours = elapsed.num_hours();
        let display = if hours >= 48 {
            format!("{} days", hours / 24)
        } else {
            format!("{} hours", hours)
        };
        let claimed_by_info = feature
            .claimed_by
            .as_deref()
            .map(|agent| format!(" (claimed by '{}')", agent))
            .unwrap_or_default();
        Some(format!(
            "WARNING: This feature has been in_progress for {}{} with no updates. \
             If work is complete, call complete_feature to record what was done. \
             If work was abandoned, update state back to 'proposed'.",
            display, claimed_by_info
        ))
    } else {
        None
    }
}

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
                req.version_id,
                req.state.as_deref(),
                effective_limit,
                req.offset,
            )
            .await
            .map_err(client_err)?
    };

    let total = features.len();
    let was_capped = req.limit.is_none() && total as u32 == default_limit;

    // Look up project key_prefix for display IDs (if scoped to one project)
    let key_prefix = if let Some(pid) = req.project_id {
        client
            .get_project(pid)
            .await
            .ok()
            .map(|p| p.project.key_prefix)
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Build lookup for parent display IDs
    let number_lookup: std::collections::HashMap<crate::models::FeatureId, i32> = features
        .iter()
        .filter_map(|f| f.feature_number.map(|n| (f.id, n)))
        .collect();

    // Render as markdown table
    let headers = &["ID", "State", "P", "Parent", "Title"];
    let rows: Vec<Vec<String>> = features
        .iter()
        .map(|f| {
            let id: uuid::Uuid = f.id.into();
            vec![
                format::display_id(f.feature_number, &key_prefix, &id),
                format::state_symbol(f.state.as_str()).to_string(),
                f.priority.to_string(),
                f.parent_id
                    .map(|pid| {
                        let pid_uuid: uuid::Uuid = pid.into();
                        let parent_number = number_lookup.get(&pid).copied();
                        format::display_id(parent_number, &key_prefix, &pid_uuid)
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

    // Populate display IDs
    let project_id: uuid::Uuid = feature_with_context.feature.project_id.into();
    let key_prefix = client
        .get_project(project_id)
        .await
        .ok()
        .map(|p| p.project.key_prefix)
        .unwrap_or_default();
    format::populate_display_ids(&mut feature_info, &feature_with_context, &key_prefix);

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

    let is_root = feature_with_context.feature.parent_id.is_none();
    let has_children = !feature_with_context.children.is_empty();
    let tier = if is_root {
        super::spec::FeatureTier::Project
    } else if has_children {
        super::spec::FeatureTier::FeatureSet
    } else {
        super::spec::FeatureTier::Leaf
    };

    let response = crate::mcp::types::GetFeatureResponse {
        feature: feature_info,
        feature_tier: tier.as_str().to_string(),
        history,
    };

    let yaml = format::to_yaml(&response).map_err(|e| McpError::internal_error(e, None))?;

    let summary = format!(
        "Feature: '{}' ({}, {})",
        feature_with_context.feature.title,
        feature_with_context.feature.state.as_str(),
        tier.as_str(),
    );
    let mut content = vec![Content::text(summary)];
    if let Some(warning) = stale_warning(&feature_with_context.feature) {
        content.push(Content::text(warning));
    }
    content.push(Content::text(yaml));
    Ok(CallToolResult::success(content))
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

    // Look up project key_prefix for display IDs
    let key_prefix = client
        .get_project(req.project_id)
        .await
        .ok()
        .map(|p| p.project.key_prefix)
        .unwrap_or_default();

    let rendered = tree_render::render_tree_with_depth(&tree, req.max_depth, &key_prefix);

    // Wrap in code fences so IDEs that render markdown preserve whitespace and indentation
    let formatted = format!("```\n{}```", rendered);

    Ok(CallToolResult::success(vec![Content::text(formatted)]))
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

    let count = response.proposed_features.len();
    let verb = if response.created {
        "Created"
    } else {
        "Proposed"
    };
    let summary = format!(
        "{} {} feature{}",
        verb,
        count,
        if count == 1 { "" } else { "s" }
    );

    Ok(CallToolResult::success(vec![
        Content::text(summary),
        Content::text(json),
    ]))
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

    let summary = format!("Created '{}' ({})", feature.title, feature.state.as_str());
    let result: FeatureInfo = (&feature).into();

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![
        Content::text(summary),
        Content::text(json),
    ]))
}

/// Update any field on a feature.
/// This is a general-purpose tool that replaces narrow state-transition tools.
pub async fn update_feature(
    client: &ManifestClient,
    req: UpdateFeatureRequest,
) -> Result<CallToolResult, McpError> {
    // Resolve feature ID (supports UUID, display ID like MAN-42, or UUID prefix)
    let feature_id = client
        .resolve_feature_id(&req.feature_id, None)
        .await
        .map_err(client_err)?;

    // Parse state if provided
    let state = if let Some(ref state_str) = req.state {
        Some(FeatureState::from_str(state_str).map_err(|_| {
            McpError::invalid_params(
                format!(
                    "Invalid state '{}'. Must be: proposed, blocked, in_progress, implemented, or archived",
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
        blocked_by: req
            .blocked_by
            .map(|ids| ids.into_iter().map(FeatureId::from).collect()),
    };

    let feature = client
        .update_feature(feature_id, &input)
        .await
        .map_err(client_err)?;

    let summary = format!("Updated '{}' ({})", feature.title, feature.state.as_str());
    let result: FeatureInfo = (&feature).into();

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![
        Content::text(summary),
        Content::text(json),
    ]))
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

    // Guard: blocked features cannot be started
    if feature_with_context.feature.state == FeatureState::Blocked {
        let blockers = client
            .get_feature_blockers(feature_id)
            .await
            .map_err(client_err)?;
        let blocker_names: Vec<String> = blockers
            .iter()
            .map(|b| format!("  - {} ({})", b.title, b.state.as_str()))
            .collect();
        return Ok(CallToolResult::error(vec![Content::text(format!(
            "Cannot start '{}' — it is blocked by:\n{}\n\nThese features must reach 'implemented' before this feature can be started.",
            feature_with_context.feature.title,
            blocker_names.join("\n")
        ))]));
    }

    // Guard: blocked ancestor feature set prevents starting descendants
    if let Some((blocked_id, blocked_title)) = client
        .find_blocked_ancestor(feature_id)
        .await
        .map_err(client_err)?
    {
        return Ok(CallToolResult::error(vec![Content::text(format!(
            "Cannot start '{}' — ancestor feature set '{}' ({}) is blocked. Unblock the ancestor first.",
            feature_with_context.feature.title,
            blocked_title,
            blocked_id,
        ))]));
    }

    // Guard: feature sets cannot be started — only leaf features
    if !feature_with_context.children.is_empty() {
        let child_list: Vec<String> = feature_with_context
            .children
            .iter()
            .map(|c| format!("  - {} ({})", c.title, c.state.as_str()))
            .collect();
        return Ok(CallToolResult::error(vec![Content::text(format!(
            "Cannot start '{}' — it is a feature set with {} children:\n{}\n\nUse start_feature on a specific child instead.",
            feature_with_context.feature.title,
            feature_with_context.children.len(),
            child_list.join("\n")
        ))]));
    }

    // Fetch project settings for configurable guidance levels
    let project_id: uuid::Uuid = feature_with_context.feature.project_id.into();
    let project_with_dirs = client.get_project(project_id).await.map_err(client_err)?;
    let config = SpecConfig {
        detail_level: project_with_dirs.project.detail_level,
        ac_level: project_with_dirs.project.ac_level,
        ac_format: project_with_dirs.project.ac_format,
        testing_policy: project_with_dirs.project.testing_policy,
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

    // Atomic claim: transition state to in_progress + set claim fields in one
    // transaction. Returns a clear conflict error if another agent already
    // holds a claim (unless force=true).
    if let Err(e) = client
        .set_feature_claim(
            feature_id,
            &req.agent_type,
            req.claim_metadata.as_deref(),
            req.force,
        )
        .await
    {
        if let crate::mcp::client::ClientError::Conflict(ref body) = e {
            // Parse the structured conflict response for a clear agent message
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) {
                if parsed.get("error").and_then(|v| v.as_str()) == Some("claim_conflict") {
                    let conflict = &parsed["conflict"];
                    let agent = conflict["agent_type"].as_str().unwrap_or("unknown");
                    let claimed_at = conflict["claimed_at"].as_str().unwrap_or("unknown time");
                    let feature_id_str = conflict["feature_id"].as_str().unwrap_or("unknown");
                    let metadata_info = conflict["claim_metadata"]
                        .as_str()
                        .map(|m| format!("\nClaim metadata: {}", m))
                        .unwrap_or_default();

                    return Ok(CallToolResult::error(vec![Content::text(format!(
                        "CONFLICT: Feature '{}' is already claimed by '{}' (since {}, feature_id: {}).{}\n\
                         To override, call start_feature with force=true.\n\
                         Otherwise, pick a different feature — use get_next_feature or find_features with state='proposed'.",
                        feature_with_context.feature.title,
                        agent,
                        claimed_at,
                        feature_id_str,
                        metadata_info,
                    ))]));
                }
            }
        }
        return Err(client_err(e));
    }

    // Cascade: also start all proposed children (max 5 levels deep)
    start_children_recursive(client, &feature_with_context.children, 0, 5).await?;

    // Git: create and checkout feature branch (best-effort)
    let mut branch_message: Option<String> = None;
    let primary_dir = project_with_dirs
        .directories
        .iter()
        .find(|d| d.is_primary)
        .or_else(|| project_with_dirs.directories.first());
    if let Some(dir) = primary_dir {
        if crate::mcp::git::is_git_repo(&dir.path) {
            let slug = crate::mcp::git::slugify(&feature_with_context.feature.title);
            let branch_name = format!("feature/{slug}");
            match crate::mcp::git::create_and_checkout(&dir.path, &branch_name) {
                Ok(()) => {
                    branch_message =
                        Some(format!("Branch: created and switched to `{branch_name}`"));
                }
                Err(e) => {
                    branch_message = Some(format!("warning: could not create branch — {e}"));
                }
            }
        }
    }

    // Re-fetch context to get updated states
    let feature_with_context = client
        .get_feature_with_context(feature_id)
        .await
        .map_err(client_err)?;

    let mut feature_info: FeatureInfoWithContext = (&feature_with_context).into();

    // Populate display IDs (project_with_dirs already fetched above)
    format::populate_display_ids(
        &mut feature_info,
        &feature_with_context,
        &project_with_dirs.project.key_prefix,
    );

    // Apply LOD to breadcrumb
    feature_info.breadcrumb = format::lod_breadcrumb(&feature_info.breadcrumb, 1);

    let response = crate::mcp::types::StartFeatureResponse {
        feature: feature_info,
        spec_status: spec_status.summary().to_string(),
        feature_tier: spec_status.tier.as_str().to_string(),
        ac_level: config.ac_level.as_str().to_string(),
        ac_format: config.ac_format.as_str().to_string(),
        testing_policy: config.testing_policy.as_str().to_string(),
        testing_guidance: config.testing_guidance(),
        testable_criteria_count: spec_status.testable_criteria_count,
        detail_level: config.detail_level.as_str().to_string(),
        workflow_reminder: "After implementing, you MUST complete these steps:\n1. prove_feature — record test evidence (command, results, evidence files)\n2. update_feature — set details to what was actually built\n3. complete_feature — summary + commit SHAs\nSkipping these steps leaves a stale spec that misleads future agents.".to_string(),
    };

    let yaml = format::to_yaml(&response).map_err(|e| McpError::internal_error(e, None))?;

    // Build content blocks
    let mut content = Vec::new();

    // Human-readable summary line
    content.push(Content::text(format!(
        "Started '{}' — now in_progress",
        feature_with_context.feature.title,
    )));

    // Git branch info (if applicable)
    if let Some(msg) = branch_message {
        content.push(Content::text(msg));
    }

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

    // If spec is too sparse or too verbose, prepend a warning text block
    if spec_status.has_warnings(&config) {
        let warning = spec_status.guidance(&config).unwrap_or_default();
        content.push(Content::text(format!("\u{26a0} {}", warning)));
    }

    content.push(Content::text(yaml));

    content.push(Content::text(
        "COMPLETION CONTRACT: After implementing this feature, you MUST:\n\
         1. prove_feature — record test evidence (command, structured results, evidence files)\n\
         2. update_feature — set `details` to describe what was actually built\n\
         3. complete_feature — provide summary of work + commit SHAs\n\
         Skipping these steps leaves stale documentation that misleads future agents.",
    ));

    Ok(CallToolResult::success(content))
}

/// Recursively transition proposed children to in_progress.
///
/// `depth` tracks current recursion level, `max_depth` caps it to prevent
/// unbounded recursion on deep trees.
async fn start_children_recursive(
    client: &ManifestClient,
    children: &[crate::models::FeatureSummaryContext],
    depth: u32,
    max_depth: u32,
) -> Result<(), McpError> {
    if depth >= max_depth {
        return Ok(());
    }

    use futures_util::future::try_join_all;

    let update_futures: Vec<_> = children
        .iter()
        .filter(|child| child.state == FeatureState::Proposed)
        .map(|child| async move {
            let child_uuid: uuid::Uuid = child.id.into();

            // Get child context to check if it has its own children
            let child_context = client
                .get_feature_with_context(child_uuid)
                .await
                .map_err(client_err)?;

            // Only update state on leaf children; skip feature sets
            if child_context.children.is_empty() {
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
                            blocked_by: None,
                        },
                    )
                    .await
                    .map_err(client_err)?;
            }

            // Recurse into children (bounded)
            Box::pin(start_children_recursive(
                client,
                &child_context.children,
                depth + 1,
                max_depth,
            ))
            .await
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
    // Resolve feature ID (supports UUID, display ID like MAN-42, or UUID prefix)
    let feature_id = client
        .resolve_feature_id(&req.feature_id, None)
        .await
        .map_err(client_err)?;

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

    // Complete feature (creates history + updates state + clears claims + emits event)
    let response = client
        .complete_feature(feature_id, &req.summary, &commits)
        .await
        .map_err(client_err)?;

    let feature = response.feature;
    let history = response.history;

    // Git: merge feature branch back into default branch (best-effort)
    let mut merge_message: Option<String> = None;
    let project_id: uuid::Uuid = feature.project_id.into();
    if let Ok(project_with_dirs) = client.get_project(project_id).await {
        let primary_dir = project_with_dirs
            .directories
            .iter()
            .find(|d| d.is_primary)
            .or_else(|| project_with_dirs.directories.first());
        if let Some(dir) = primary_dir {
            if crate::mcp::git::is_git_repo(&dir.path) {
                if let Ok(current) = crate::mcp::git::current_branch(&dir.path) {
                    if current.starts_with("feature/") {
                        if let Ok(default) = crate::mcp::git::default_branch(&dir.path) {
                            if current != default {
                                match crate::mcp::git::checkout(&dir.path, &default) {
                                    Ok(()) => {
                                        match crate::mcp::git::merge_branch(&dir.path, &current) {
                                            Ok(()) => {
                                                let _ = crate::mcp::git::delete_branch(
                                                    &dir.path, &current,
                                                );
                                                merge_message = Some(format!(
                                                    "Merged `{current}` into `{default}` and deleted branch"
                                                ));
                                            }
                                            Err(e) => {
                                                // Merge failed — go back to feature branch
                                                let _ =
                                                    crate::mcp::git::checkout(&dir.path, &current);
                                                merge_message = Some(format!(
                                                    "warning: merge failed — {e}. Still on `{current}`"
                                                ));
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        merge_message = Some(format!(
                                            "warning: could not checkout {default} — {e}. Still on `{current}`"
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let commit_count = history.details.commits.len();
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

    let summary = format!(
        "Completed '{}' — recorded {} commit{}",
        feature.title,
        commit_count,
        if commit_count == 1 { "" } else { "s" },
    );
    let mut content = vec![Content::text(summary)];
    if let Some(msg) = merge_message {
        content.push(Content::text(msg));
    }
    content.push(Content::text(json));
    Ok(CallToolResult::success(content))
}

/// Record test evidence for a feature.
pub async fn prove_feature(
    client: &ManifestClient,
    req: ProveFeatureRequest,
) -> Result<CallToolResult, McpError> {
    use crate::models::{CreateProofInput, Evidence, TestResult, TestState};

    // Resolve feature ID
    let feature_id = client
        .resolve_feature_id(&req.feature_id, None)
        .await
        .map_err(client_err)?;

    // Convert test results
    let tests = req.tests.map(|test_inputs| {
        test_inputs
            .into_iter()
            .map(|t| TestResult {
                name: t.name,
                suite: t.suite,
                state: TestState::from_str(&t.state).unwrap_or(TestState::Errored),
                file: t.file,
                line: t.line,
                duration_ms: t.duration_ms,
                message: t.message,
            })
            .collect()
    });

    // Convert evidence
    let evidence: Vec<Evidence> = req
        .evidence
        .into_iter()
        .map(|e| Evidence {
            path: e.path,
            note: e.note,
        })
        .collect();

    let input = CreateProofInput {
        feature_id: feature_id.into(),
        history_id: None,
        command: req.command,
        exit_code: req.exit_code,
        output: req.output,
        tests,
        evidence,
        commit_sha: req.commit_sha,
        agent_type: Some("claude".to_string()),
    };

    let proof = client
        .create_proof(feature_id, &input)
        .await
        .map_err(client_err)?;

    // Build summary
    let test_summary = if let Some(ref tests) = proof.tests {
        let passed = tests
            .iter()
            .filter(|t| t.state == TestState::Passed)
            .count();
        let failed = tests
            .iter()
            .filter(|t| t.state == TestState::Failed)
            .count();
        let errored = tests
            .iter()
            .filter(|t| t.state == TestState::Errored)
            .count();
        let skipped = tests
            .iter()
            .filter(|t| t.state == TestState::Skipped)
            .count();
        let mut parts = Vec::new();
        if passed > 0 {
            parts.push(format!("{passed} passed"));
        }
        if failed > 0 {
            parts.push(format!("{failed} failed"));
        }
        if errored > 0 {
            parts.push(format!("{errored} errored"));
        }
        if skipped > 0 {
            parts.push(format!("{skipped} skipped"));
        }
        parts.join(", ")
    } else {
        format!("exit code {}", proof.exit_code)
    };

    let summary = format!("Proof recorded — {test_summary}");
    Ok(CallToolResult::success(vec![Content::text(summary)]))
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

    match result {
        Some(feature_ctx) => {
            // Fetch project settings for configurable guidance levels
            let project_id: uuid::Uuid = feature_ctx.feature.project_id.into();
            let project_with_dirs = client.get_project(project_id).await.map_err(client_err)?;
            let config = SpecConfig {
                detail_level: project_with_dirs.project.detail_level,
                ac_level: project_with_dirs.project.ac_level,
                ac_format: project_with_dirs.project.ac_format,
                testing_policy: project_with_dirs.project.testing_policy,
            };

            let is_root = feature_ctx.feature.parent_id.is_none();
            let spec_status = super::spec::analyze_spec(
                feature_ctx.feature.details.as_deref(),
                !feature_ctx.children.is_empty(),
                is_root,
            );

            let summary = format!(
                "Next: '{}' ({}, {})",
                feature_ctx.feature.title,
                feature_ctx.feature.state.as_str(),
                spec_status.tier.as_str(),
            );

            let mut info: FeatureInfoWithContext = (&feature_ctx).into();

            // Populate display IDs
            format::populate_display_ids(
                &mut info,
                &feature_ctx,
                &project_with_dirs.project.key_prefix,
            );

            // Apply LOD to breadcrumb
            info.breadcrumb = format::lod_breadcrumb(&info.breadcrumb, 1);

            let response = crate::mcp::types::NextFeatureResponse {
                feature: info,
                spec_status: spec_status.summary().to_string(),
                feature_tier: spec_status.tier.as_str().to_string(),
                ac_level: config.ac_level.as_str().to_string(),
                ac_format: config.ac_format.as_str().to_string(),
                testing_policy: config.testing_policy.as_str().to_string(),
                testing_guidance: config.testing_guidance(),
                testable_criteria_count: spec_status.testable_criteria_count,
                detail_level: config.detail_level.as_str().to_string(),
                spec_guidance: spec_status.guidance(&config),
            };

            let yaml = format::to_yaml(&response).map_err(|e| McpError::internal_error(e, None))?;
            let mut content = vec![Content::text(summary)];
            if let Some(warning) = stale_warning(&feature_ctx.feature) {
                content.push(Content::text(warning));
            }
            content.push(Content::text(yaml));
            Ok(CallToolResult::success(content))
        }
        None => Ok(CallToolResult::success(vec![Content::text(
            "No workable features found.",
        )])),
    }
}

/// Assemble the feature spec + implementation diff for the calling agent to analyze.
///
/// This tool does NOT call an LLM — the calling agent IS the LLM. It:
/// 1. Resolves the feature and assembles its spec + ancestor breadcrumb
/// 2. Gets a git diff (from commit_range, or uncommitted changes from the project directory)
/// 3. Returns assembled context with instructions for the agent to analyze and call record_verification
pub async fn verify_feature(
    client: &ManifestClient,
    req: VerifyFeatureRequest,
) -> Result<CallToolResult, McpError> {
    let feature_id = client
        .resolve_feature_id(&req.feature_id, None)
        .await
        .map_err(client_err)?;

    // Get the feature to find its project for the git directory
    let feature = client.get_feature(feature_id).await.map_err(client_err)?;
    let project_id: uuid::Uuid = feature.project_id.into();

    // Attempt to get a diff from the project's primary directory
    let diff = if let Ok(project_with_dirs) = client.get_project(project_id).await {
        let primary_dir = project_with_dirs
            .directories
            .iter()
            .find(|d| d.is_primary)
            .or_else(|| project_with_dirs.directories.first());

        if let Some(dir) = primary_dir {
            if crate::mcp::git::is_git_repo(&dir.path) {
                let commit_range = req.commit_range.as_deref();
                match crate::mcp::git::get_diff(&dir.path, commit_range) {
                    Ok(d) if !d.is_empty() => Some(d),
                    _ => None,
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Call the API to assemble spec context + filter diff
    let ctx = client
        .get_verify_context(feature_id, diff)
        .await
        .map_err(client_err)?;

    let mut content = vec![Content::text(format!(
        "Verification context assembled for '{}'. Analyze the spec against the diff and call record_verification with your findings.",
        feature.title,
    ))];

    content.push(Content::text(format!("## Spec\n\n{}", ctx.spec)));

    match &ctx.diff {
        Some(diff) => {
            let truncation_note = if ctx.diff_truncated {
                " (truncated at 50K characters)"
            } else {
                ""
            };
            content.push(Content::text(format!(
                "## Diff{}\n\n```diff\n{}\n```",
                truncation_note, diff
            )));
        }
        None => {
            content.push(Content::text(
                "## Diff\n\nNo diff available (no uncommitted changes or commit range found).",
            ));
        }
    }

    content.push(Content::text(ctx.instructions));

    Ok(CallToolResult::success(content))
}

/// Store verification comments produced by your analysis of `verify_feature` output.
///
/// Call this after analyzing the spec + diff context returned by `verify_feature`.
/// Pass an empty `comments` array if the implementation fully satisfies the spec.
pub async fn record_verification(
    client: &ManifestClient,
    req: RecordVerificationRequest,
) -> Result<CallToolResult, McpError> {
    let feature_id = client
        .resolve_feature_id(&req.feature_id, None)
        .await
        .map_err(client_err)?;

    // Convert comment inputs to JSON values for the API
    let comments: Vec<serde_json::Value> = req
        .comments
        .iter()
        .map(|c| {
            serde_json::json!({
                "severity": c.severity,
                "title": c.title,
                "body": c.body,
                "file": c.file,
            })
        })
        .collect();

    let feature = client
        .record_verification(feature_id, comments)
        .await
        .map_err(client_err)?;

    let count = req.comments.len();
    let summary = if count == 0 {
        format!(
            "Verification recorded for '{}' — implementation satisfies the spec.",
            feature.title
        )
    } else {
        let critical = req
            .comments
            .iter()
            .filter(|c| c.severity == "critical")
            .count();
        let major = req
            .comments
            .iter()
            .filter(|c| c.severity == "major")
            .count();
        let minor = req
            .comments
            .iter()
            .filter(|c| c.severity == "minor")
            .count();
        format!(
            "Verification recorded for '{}' — {} comment{} ({} critical, {} major, {} minor).",
            feature.title,
            count,
            if count == 1 { "" } else { "s" },
            critical,
            major,
            minor,
        )
    };

    Ok(CallToolResult::success(vec![Content::text(summary)]))
}
