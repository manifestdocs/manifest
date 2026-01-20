use rmcp::{
    model::{CallToolResult, Content},
    ErrorData as McpError,
};
use uuid::Uuid;

use crate::mcp::{
    types::{
        CreateVersionRequest, FeatureInfo, ListVersionsRequest, ReleaseVersionRequest,
        SetFeatureVersionRequest, VersionInfo, VersionListResponse,
    },
    ManifestClient,
};
use crate::models::{CreateVersionInput, UpdateFeatureInput, UpdateVersionInput};

use super::client_err;

/// List versions for a project, including 'now' and 'next' indicators.
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
        .list_features(Some(req.project_id), None, None, None)
        .await
        .map_err(client_err)?;

    // Count features per version
    let mut version_feature_counts: std::collections::HashMap<Uuid, u32> =
        std::collections::HashMap::new();
    for feature in &features {
        if let Some(vid) = feature.target_version_id {
            *version_feature_counts.entry(vid).or_insert(0) += 1;
        }
    }

    // Find unreleased versions for Now/Next
    let mut unreleased: Vec<_> = versions
        .iter()
        .filter(|v| v.released_at.is_none())
        .collect();
    unreleased.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    let now = unreleased.first().map(|v| v.id);
    let next = unreleased.get(1).map(|v| v.id);

    // Build response
    let result = VersionListResponse {
        versions: versions
            .into_iter()
            .map(|v| VersionInfo {
                id: v.id,
                name: v.name,
                description: v.description,
                released_at: v.released_at.map(|dt| dt.to_rfc3339()),
                feature_count: version_feature_counts.get(&v.id).copied().unwrap_or(0),
            })
            .collect(),
        now,
        next,
    };

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
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

    let result = VersionInfo {
        id: version.id,
        name: version.name,
        description: version.description,
        released_at: version.released_at.map(|dt| dt.to_rfc3339()),
        feature_count: 0, // New version has no features yet
    };

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Assign a feature to a target version.
pub async fn set_feature_version(
    client: &ManifestClient,
    req: SetFeatureVersionRequest,
) -> Result<CallToolResult, McpError> {
    // Convert Option<Uuid> to Option<Option<Uuid>> for the update input
    // Some(Some(vid)) = set version, Some(None) = explicitly unassign
    let version_id = Some(req.version_id);

    let feature = client
        .update_feature(
            req.feature_id,
            &UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                state: None,
                priority: None,
                target_version_id: version_id,
            },
        )
        .await
        .map_err(client_err)?;

    let result: FeatureInfo = (&feature).into();

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
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

    let result = VersionInfo {
        id: version.id,
        name: version.name,
        description: version.description,
        released_at: version.released_at.map(|dt| dt.to_rfc3339()),
        feature_count: 0, // Would need to fetch features to count
    };

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
}
