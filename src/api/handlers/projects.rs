use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::db::Database;
use crate::models::{
    AddDirectoryInput, CreateProjectInput, Project, ProjectDirectory, ProjectHistoryEntry,
    ProjectWithDirectories, UpdateProjectInput, VersionId,
};

use super::{internal_error, ApiError};
use crate::api::validation::ValidatedJson;

// ============================================================
// Projects
// ============================================================

/// List all projects.
pub async fn list_projects(State(db): State<Database>) -> Result<Json<Vec<Project>>, ApiError> {
    db.get_all_projects()
        .await
        .map(Json)
        .map_err(internal_error)
}

/// Get a project by ID with its associated directories.
pub async fn get_project(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProjectWithDirectories>, ApiError> {
    db.get_project_with_directories(id.into())
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or(ApiError::from((
            StatusCode::NOT_FOUND,
            "Project not found".to_string(),
        )))
}

/// Get a project by slug with its associated directories.
pub async fn get_project_by_slug(
    State(db): State<Database>,
    Path(slug): Path<String>,
) -> Result<Json<ProjectWithDirectories>, ApiError> {
    let project = db
        .get_project_by_slug(&slug)
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::from((
            StatusCode::NOT_FOUND,
            "Project not found".to_string(),
        )))?;

    let directories = db
        .get_project_directories(project.id)
        .await
        .map_err(internal_error)?;

    Ok(Json(ProjectWithDirectories {
        project,
        directories,
    }))
}

/// Create a new project.
pub async fn create_project(
    State(db): State<Database>,
    ValidatedJson(input): ValidatedJson<CreateProjectInput>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    let project = db.create_project(input).await.map_err(internal_error)?;

    // Bootstrap initial versions so agents and the web UI have versions to target
    let _ = db.ensure_minimum_versions(project.id, 4).await;

    Ok((StatusCode::CREATED, Json(project)))
}

/// Update an existing project.
pub async fn update_project(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    ValidatedJson(input): ValidatedJson<UpdateProjectInput>,
) -> Result<Json<Project>, ApiError> {
    db.update_project(id.into(), input)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or(ApiError::from((
            StatusCode::NOT_FOUND,
            "Project not found".to_string(),
        )))
}

/// Delete a project and all associated data.
pub async fn delete_project(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if db.delete_project(id.into()).await.map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::from((
            StatusCode::NOT_FOUND,
            "Project not found".to_string(),
        )))
    }
}

// ============================================================
// Project History
// ============================================================

/// Query parameters for filtering project-wide history.
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
) -> Result<Json<Vec<ProjectHistoryEntry>>, ApiError> {
    // Parse optional since datetime
    let since = query
        .since
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    db.get_project_history(
        project_id.into(),
        query.version_id.map(VersionId::from),
        query.limit,
        query.offset,
        since,
    )
    .await
    .map(Json)
    .map_err(internal_error)
}

// ============================================================
// Project Directories
// ============================================================

/// List all directories associated with a project.
pub async fn list_project_directories(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<ProjectDirectory>>, ApiError> {
    db.get_project_directories(project_id.into())
        .await
        .map(Json)
        .map_err(internal_error)
}

/// Associate a directory with a project.
pub async fn add_project_directory(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    ValidatedJson(input): ValidatedJson<AddDirectoryInput>,
) -> Result<(StatusCode, Json<ProjectDirectory>), ApiError> {
    db.add_project_directory(project_id.into(), input)
        .await
        .map(|d| (StatusCode::CREATED, Json(d)))
        .map_err(internal_error)
}

/// Remove a directory association from a project.
pub async fn remove_project_directory(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if db
        .remove_project_directory(id.into())
        .await
        .map_err(internal_error)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::from((
            StatusCode::NOT_FOUND,
            "Directory not found".to_string(),
        )))
    }
}

/// Query parameters for looking up a project by directory path.
#[derive(Debug, Deserialize)]
pub struct GetProjectByDirectoryQuery {
    /// Absolute file system path to match against registered project directories.
    pub path: String,
}

/// Find a project by directory path.
///
/// Returns the project and matching directory if the path matches exactly,
/// or if the path is a subdirectory of a registered project directory.
pub async fn get_project_by_directory(
    State(db): State<Database>,
    Query(query): Query<GetProjectByDirectoryQuery>,
) -> Result<Json<ProjectWithDirectories>, ApiError> {
    db.get_project_by_directory(&query.path)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or(ApiError::from((
            StatusCode::NOT_FOUND,
            format!("No project found for directory: {}", query.path),
        )))
}
