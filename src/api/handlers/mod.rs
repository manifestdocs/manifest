use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::db::{Database, ManifestError};
use crate::models::{
    AddDirectoryInput, CommitRef, CreateFeatureInput, CreateHistoryInput, CreateProjectInput,
    CreateVersionInput, Feature, FeatureDiff, FeatureHistory, FeatureState, FeatureSummary,
    FeatureTreeNode, HistoryDetails, ListFeaturesQuery, Project, ProjectDirectory,
    ProjectHistoryEntry, ProjectWithDirectories, UpdateFeatureInput, UpdateProjectInput,
    UpdateVersionInput, Version,
};

// Import MCP types for bulk feature creation (re-exported from mcp module)
use crate::mcp::{PlanFeaturesResponse, ProposedFeature};

// ============================================================
// Error Handling
// ============================================================

/// Convert a ManifestError to an HTTP response.
/// These are domain errors that should be exposed to the client.
fn manifest_error(e: ManifestError) -> (StatusCode, String) {
    let status = match &e {
        ManifestError::NotFound(_) => StatusCode::NOT_FOUND,
        ManifestError::Validation(_) => StatusCode::BAD_REQUEST,
        ManifestError::InvalidState(_) => StatusCode::CONFLICT,
    };
    tracing::warn!("Client error: {}", e);
    (status, e.to_string())
}

/// Convert an anyhow::Error to an HTTP response.
/// Checks if the error is a ManifestError (domain error) and handles it appropriately.
/// Other errors are treated as internal server errors.
fn internal_error(e: anyhow::Error) -> (StatusCode, String) {
    // Check if this is a wrapped ManifestError (domain error)
    if let Some(manifest_err) = e.downcast_ref::<ManifestError>() {
        return manifest_error(manifest_err.clone());
    }

    // True internal error - log full details but return generic message
    tracing::error!("Internal error: {:?}", e);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal server error".to_string(),
    )
}

// ============================================================
// Health
// ============================================================

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

// ============================================================
// Projects
// ============================================================

pub async fn list_projects(
    State(db): State<Database>,
) -> Result<Json<Vec<Project>>, (StatusCode, String)> {
    db.get_all_projects().map(Json).map_err(internal_error)
}

pub async fn get_project(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProjectWithDirectories>, (StatusCode, String)> {
    db.get_project_with_directories(id)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))
}

pub async fn create_project(
    State(db): State<Database>,
    Json(input): Json<CreateProjectInput>,
) -> Result<(StatusCode, Json<Project>), (StatusCode, String)> {
    db.create_project(input)
        .map(|p| (StatusCode::CREATED, Json(p)))
        .map_err(internal_error)
}

pub async fn update_project(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateProjectInput>,
) -> Result<Json<Project>, (StatusCode, String)> {
    db.update_project(id, input)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))
}

pub async fn delete_project(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    if db.delete_project(id).map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "Project not found".to_string()))
    }
}

/// Query parameters for project history.
#[derive(Debug, Deserialize)]
pub struct ProjectHistoryQuery {
    /// Filter to entries for a specific version (useful for release notes).
    pub version_id: Option<Uuid>,
    /// Maximum number of entries to return. Defaults to 50.
    pub limit: Option<u32>,
    /// Number of entries to skip for pagination. Defaults to 0.
    pub offset: Option<u32>,
    /// Optional ISO datetime to filter entries created after this time.
    pub since: Option<String>,
}

/// Get project-wide history across all features.
///
/// Returns history entries for all features in the project, ordered by
/// creation date (newest first). Can be filtered by version_id to generate
/// release notes. Useful for PMs to see recent changes.
pub async fn get_project_history(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<ProjectHistoryQuery>,
) -> Result<Json<Vec<ProjectHistoryEntry>>, (StatusCode, String)> {
    // Parse optional since datetime
    let since = query
        .since
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    db.get_project_history(
        project_id,
        query.version_id,
        query.limit,
        query.offset,
        since,
    )
    .map(Json)
    .map_err(internal_error)
}

// ============================================================
// Project Directories
// ============================================================

pub async fn list_project_directories(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<ProjectDirectory>>, (StatusCode, String)> {
    db.get_project_directories(project_id)
        .map(Json)
        .map_err(internal_error)
}

pub async fn add_project_directory(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<AddDirectoryInput>,
) -> Result<(StatusCode, Json<ProjectDirectory>), (StatusCode, String)> {
    db.add_project_directory(project_id, input)
        .map(|d| (StatusCode::CREATED, Json(d)))
        .map_err(internal_error)
}

pub async fn remove_project_directory(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    if db.remove_project_directory(id).map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "Directory not found".to_string()))
    }
}

// ============================================================
// Versions
// ============================================================

pub async fn list_project_versions(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<Version>>, (StatusCode, String)> {
    db.get_versions_by_project(project_id)
        .map(Json)
        .map_err(internal_error)
}

pub async fn create_version(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateVersionInput>,
) -> Result<(StatusCode, Json<Version>), (StatusCode, String)> {
    db.create_version(project_id, input)
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(internal_error)
}

pub async fn get_version(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<Version>, (StatusCode, String)> {
    db.get_version(id)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Version not found".to_string()))
}

pub async fn update_version(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateVersionInput>,
) -> Result<Json<Version>, (StatusCode, String)> {
    db.update_version(id, input)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Version not found".to_string()))
}

pub async fn delete_version(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    if db.delete_version(id).map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "Version not found".to_string()))
    }
}

// ============================================================
// Features
// ============================================================

pub async fn list_features(
    State(db): State<Database>,
    Query(query): Query<ListFeaturesQuery>,
) -> Result<Json<Vec<FeatureSummary>>, (StatusCode, String)> {
    // Use SQL-based pagination for efficiency
    let features = db
        .get_all_features_paginated(query.limit, query.offset)
        .map_err(internal_error)?;

    // Always return summaries only - use get_feature for full details
    let summaries: Vec<FeatureSummary> = features.into_iter().map(Into::into).collect();
    Ok(Json(summaries))
}

pub async fn list_project_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<ListFeaturesQuery>,
) -> Result<Json<Vec<FeatureSummary>>, (StatusCode, String)> {
    // Use SQL-based pagination for efficiency
    let features = db
        .get_features_by_project_paginated(project_id, query.limit, query.offset)
        .map_err(internal_error)?;
    // Always return summaries only - use get_feature for full details
    let summaries: Vec<FeatureSummary> = features.into_iter().map(Into::into).collect();
    Ok(Json(summaries))
}

pub async fn list_root_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<Feature>>, (StatusCode, String)> {
    db.get_root_features(project_id)
        .map(Json)
        .map_err(internal_error)
}

pub async fn get_feature_tree(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<FeatureTreeNode>>, (StatusCode, String)> {
    db.get_feature_tree(project_id)
        .map(Json)
        .map_err(internal_error)
}

pub async fn list_children(
    State(db): State<Database>,
    Path(parent_id): Path<Uuid>,
) -> Result<Json<Vec<Feature>>, (StatusCode, String)> {
    db.get_children(parent_id).map(Json).map_err(internal_error)
}

pub async fn get_feature_history(
    State(db): State<Database>,
    Path(feature_id): Path<Uuid>,
) -> Result<Json<Vec<FeatureHistory>>, (StatusCode, String)> {
    db.get_feature_history(feature_id)
        .map(Json)
        .map_err(internal_error)
}

/// Input for creating a history entry directly on a feature (CLI mode).
#[derive(Debug, Deserialize)]
pub struct CreateFeatureHistoryInput {
    pub summary: String,
    #[serde(default)]
    pub commits: Vec<CommitRef>,
    /// Version this work was done for.
    /// If not specified, defaults to the feature's target_version_id.
    pub version_id: Option<Uuid>,
    /// If true, also update feature state to 'implemented'. Defaults to true.
    #[serde(default = "default_true")]
    pub mark_implemented: bool,
}

fn default_true() -> bool {
    true
}

pub async fn create_feature_history(
    State(db): State<Database>,
    Path(feature_id): Path<Uuid>,
    Json(input): Json<CreateFeatureHistoryInput>,
) -> Result<(StatusCode, Json<FeatureHistory>), (StatusCode, String)> {
    // Verify feature exists
    let feature = db
        .get_feature(feature_id)
        .map_err(internal_error)?
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))?;

    // Verify it's a leaf feature
    if !db.is_leaf(feature_id).map_err(internal_error)? {
        return Err((
            StatusCode::BAD_REQUEST,
            "Cannot create history on a non-leaf feature".to_string(),
        ));
    }

    // Create history entry directly
    // If version_id not provided, database layer defaults to feature's target_version_id
    let history = db
        .create_history_entry(CreateHistoryInput {
            feature_id,
            version_id: input.version_id,
            details: HistoryDetails {
                summary: input.summary,
                commits: input.commits,
            },
        })
        .map_err(internal_error)?;

    // Optionally update feature state to implemented
    if input.mark_implemented && feature.state != FeatureState::Implemented {
        db.update_feature(
            feature_id,
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                state: Some(FeatureState::Implemented),
                priority: None,
                target_version_id: None,
            },
        )
        .map_err(internal_error)?;
    }

    Ok((StatusCode::CREATED, Json(history)))
}

pub async fn get_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<Feature>, (StatusCode, String)> {
    db.get_feature(id)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))
}

pub async fn get_feature_diff(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<FeatureDiff>, (StatusCode, String)> {
    db.get_feature_diff(id)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))
}

pub async fn create_feature(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateFeatureInput>,
) -> Result<(StatusCode, Json<Feature>), (StatusCode, String)> {
    db.create_feature(project_id, input)
        .map(|f| (StatusCode::CREATED, Json(f)))
        .map_err(internal_error)
}

pub async fn update_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateFeatureInput>,
) -> Result<Json<Feature>, (StatusCode, String)> {
    db.update_feature(id, input)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))
}

pub async fn delete_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    if db.delete_feature(id).map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "Feature not found".to_string()))
    }
}

/// Query parameters for searching features.
#[derive(Debug, Deserialize)]
pub struct SearchFeaturesQuery {
    /// Search term to match against title and details.
    pub q: String,
    /// Optional project UUID to limit search to.
    pub project_id: Option<Uuid>,
    /// Maximum number of results to return. Defaults to 10.
    pub limit: Option<u32>,
}

/// Search features by title and details.
/// Returns summaries ranked by relevance.
pub async fn search_features(
    State(db): State<Database>,
    Query(query): Query<SearchFeaturesQuery>,
) -> Result<Json<Vec<FeatureSummary>>, (StatusCode, String)> {
    db.search_features(&query.q, query.project_id, query.limit)
        .map(Json)
        .map_err(internal_error)
}

// ============================================================
// Project by Directory (for MCP get_project_context)
// ============================================================

/// Query parameters for getting a project by directory path.
#[derive(Debug, Deserialize)]
pub struct GetProjectByDirectoryQuery {
    pub path: String,
}

/// Find a project by directory path.
///
/// Returns the project and matching directory if the path matches exactly,
/// or if the path is a subdirectory of a registered project directory.
pub async fn get_project_by_directory(
    State(db): State<Database>,
    Query(query): Query<GetProjectByDirectoryQuery>,
) -> Result<Json<ProjectWithDirectories>, (StatusCode, String)> {
    db.get_project_by_directory(&query.path)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("No project found for directory: {}", query.path),
        ))
}

// ============================================================
// Bulk Feature Creation (for MCP plan_features)
// ============================================================

/// Input for bulk feature creation.
#[derive(Debug, Deserialize)]
pub struct BulkCreateFeaturesInput {
    /// The proposed feature tree.
    pub features: Vec<ProposedFeature>,
    /// If true, creates the features in the database. If false, returns preview only.
    #[serde(default)]
    pub confirm: bool,
}

/// Create multiple features at once with hierarchical structure.
///
/// When confirm=false (default), returns the proposed features without creating them.
/// When confirm=true, creates all features and returns their IDs.
pub async fn bulk_create_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<BulkCreateFeaturesInput>,
) -> Result<Json<PlanFeaturesResponse>, (StatusCode, String)> {
    // Verify project exists
    db.get_project(project_id)
        .map_err(internal_error)?
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;

    let mut created_ids = Vec::new();

    if input.confirm {
        // Flatten the tree into a list of inputs with pre-generated UUIDs
        // This allows us to use the transactional bulk insert
        let mut feature_inputs = Vec::new();
        for feature in &input.features {
            flatten_feature_tree(None, feature, &mut feature_inputs, &mut created_ids);
        }

        // Create all features in a single transaction
        db.create_features_bulk(project_id, feature_inputs)
            .map_err(internal_error)?;
    }

    Ok(Json(PlanFeaturesResponse {
        proposed_features: input.features,
        created: input.confirm,
        created_feature_ids: created_ids,
    }))
}

/// Flatten a ProposedFeature tree into a list of CreateFeatureInput.
/// Pre-generates UUIDs so parent-child relationships can be established.
fn flatten_feature_tree(
    parent_id: Option<Uuid>,
    proposed: &ProposedFeature,
    inputs: &mut Vec<CreateFeatureInput>,
    created_ids: &mut Vec<String>,
) -> Uuid {
    let id = Uuid::new_v4();
    created_ids.push(id.to_string());

    inputs.push(CreateFeatureInput {
        id: Some(id),
        parent_id,
        title: proposed.title.clone(),
        details: proposed.details.clone(),
        state: Some(FeatureState::Specified),
        priority: Some(proposed.priority),
        target_version_id: None,
    });

    // Recursively flatten children with this feature's ID as parent
    for child in &proposed.children {
        flatten_feature_tree(Some(id), child, inputs, created_ids);
    }

    id
}
