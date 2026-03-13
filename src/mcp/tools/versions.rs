//! Version lifecycle tools for AI agents.
//!
//! Manages semantic versions: listing with feature counts, creating milestones,
//! assigning features to versions, and releasing. Renders version tables as
//! markdown for agent-friendly output.

use rmcp::{
    model::{CallToolResult, Content, Role},
    ErrorData as McpError,
};

use crate::mcp::{
    types::{
        CreateVersionRequest, FeatureInfo, ListVersionsRequest, ReleaseVersionRequest,
        SetFeatureVersionRequest, VersionInfo,
    },
    ManifestClient,
};
use crate::models::{CreateVersionInput, UpdateFeatureInput, UpdateVersionInput, VersionId};

use super::client_err;
use super::format;

/// List versions for a project, including 'next' indicator.
pub async fn list_versions(
    client: &ManifestClient,
    req: ListVersionsRequest,
) -> Result<CallToolResult, McpError> {
    // Get versions and features in parallel
    let versions = client
        .list_versions(req.project_id)
        .await
        .map_err(client_err)?;

    let features = client
        .list_features(Some(req.project_id), None, None, None, None)
        .await
        .map_err(client_err)?;

    // Count features per version and backlog (excluding archived)
    let mut version_feature_counts: std::collections::HashMap<VersionId, u32> =
        std::collections::HashMap::new();
    let mut backlog_count: u32 = 0;
    for feature in &features {
        if feature.state == manifest_core::models::FeatureState::Archived {
            continue;
        }
        if let Some(vid) = feature.target_version_id {
            *version_feature_counts.entry(vid).or_insert(0) += 1;
        } else {
            backlog_count += 1;
        }
    }

    // Find first unreleased version (next to ship)
    let mut unreleased: Vec<_> = versions
        .iter()
        .filter(|v| v.released_at.is_none())
        .collect();
    unreleased.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    let next_version_id = unreleased.first().map(|v| v.id);

    // Render as markdown table
    let headers = &["ID", "Name", "Status", "Features", "Description"];
    let rows: Vec<Vec<String>> = versions
        .iter()
        .map(|v| {
            let status = if v.released_at.is_some() {
                "released"
            } else if Some(v.id) == next_version_id {
                "next"
            } else {
                "planned"
            };
            let id: uuid::Uuid = v.id.into();
            let feat_count = version_feature_counts.get(&v.id).copied().unwrap_or(0);
            vec![
                id.to_string(),
                v.name.clone(),
                status.to_string(),
                feat_count.to_string(),
                v.description.clone().unwrap_or_default(),
            ]
        })
        .collect();

    let mut output = format::markdown_table(headers, &rows);

    // Append metadata line
    let next_name = versions
        .iter()
        .find(|v| Some(v.id) == next_version_id)
        .map(|v| v.name.as_str())
        .unwrap_or("none");
    output.push_str(&format!(
        "\nNext: {} | Backlog: {} features",
        next_name, backlog_count
    ));

    Ok(CallToolResult::success(vec![Content::text(output)]))
}

/// Create a new version.
pub async fn create_version(
    client: &ManifestClient,
    req: CreateVersionRequest,
) -> Result<CallToolResult, McpError> {
    let version = client
        .create_version(
            req.project_id,
            &CreateVersionInput {
                name: req.name,
                description: req.description,
            },
        )
        .await
        .map_err(client_err)?;

    let summary = format!("Created version '{}'", version.name);
    let result = VersionInfo {
        id: version.id.into(),
        name: version.name,
        description: version.description,
        released_at: version.released_at.map(|dt| dt.to_rfc3339()),
        feature_count: 0,              // New version has no features yet
        status: "planned".to_string(), // New versions start as unreleased
    };

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![
        Content::text(summary).with_audience(vec![Role::User]),
        Content::text(json).with_audience(vec![Role::Assistant]),
    ]))
}

/// Assign a feature to a target version.
/// Notes when a feature assigned to a version has no details.
pub async fn set_feature_version(
    client: &ManifestClient,
    req: SetFeatureVersionRequest,
) -> Result<CallToolResult, McpError> {
    // Resolve feature ID (supports UUID, display ID like MAN-42, or UUID prefix)
    let feature_id = client
        .resolve_feature_id(&req.feature_id, None)
        .await
        .map_err(client_err)?;

    // Convert Option<Uuid> to Option<Option<VersionId>> for the update input
    // Some(Some(vid)) = set version, Some(None) = explicitly unassign
    let version_id = Some(req.version_id.map(VersionId::from));

    let feature = client
        .update_feature(
            feature_id,
            &UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                details_summary: None,
                state: None,
                priority: None,
                target_version_id: version_id,
                blocked_by: None,
            },
        )
        .await
        .map_err(client_err)?;

    let summary = if req.version_id.is_some() {
        format!("Assigned '{}' to version", feature.title)
    } else {
        format!("Unassigned '{}' from version", feature.title)
    };

    let result: FeatureInfo = (&feature).into();

    let mut result_json =
        serde_json::to_value(&result).map_err(|e| McpError::internal_error(e.to_string(), None))?;

    // Note when assigning an unspecified feature to a version
    if req.version_id.is_some() && feature.details.is_none() {
        result_json["spec_note"] = serde_json::json!(
            "This feature has no details. Consider adding a specification with update_feature before starting work."
        );
    }

    let json = serde_json::to_string_pretty(&result_json)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![
        Content::text(summary).with_audience(vec![Role::User]),
        Content::text(json).with_audience(vec![Role::Assistant]),
    ]))
}

/// Mark a version as released.
pub async fn release_version(
    client: &ManifestClient,
    req: ReleaseVersionRequest,
) -> Result<CallToolResult, McpError> {
    let version = client
        .update_version(
            req.version_id,
            &UpdateVersionInput {
                name: None,
                description: None,
                released_at: Some(chrono::Utc::now()),
            },
        )
        .await
        .map_err(client_err)?;

    let summary = format!("Released '{}'", version.name);
    let result = VersionInfo {
        id: version.id.into(),
        name: version.name,
        description: version.description,
        released_at: version.released_at.map(|dt| dt.to_rfc3339()),
        feature_count: 0, // Would need to fetch features to count
        status: "released".to_string(),
    };

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![
        Content::text(summary).with_audience(vec![Role::User]),
        Content::text(json).with_audience(vec![Role::Assistant]),
    ]))
}
