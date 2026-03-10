//! Version CRUD and release lifecycle endpoints.
//!
//! Manages semantic versions for projects. Detecting a release transition
//! (unreleased → released) records a history entry and ensures a minimum pool
//! of unreleased versions remains available.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use crate::db::Database;
use crate::models::{
    CreateHistoryInput, CreateVersionInput, HistoryDetails, UpdateVersionInput, Version,
};

use super::{internal_error, ApiError};
use crate::api::validation::ValidatedJson;

// ============================================================
// Versions
// ============================================================

/// List all versions for a project.
pub async fn list_project_versions(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<Version>>, ApiError> {
    db.get_versions_by_project(project_id.into())
        .await
        .map(Json)
        .map_err(internal_error)
}

/// Create a new version for a project.
pub async fn create_version(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    ValidatedJson(input): ValidatedJson<CreateVersionInput>,
) -> Result<(StatusCode, Json<Version>), ApiError> {
    db.create_version(project_id.into(), input)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(internal_error)
}

/// Get a version by ID.
pub async fn get_version(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<Version>, ApiError> {
    db.get_version(id.into())
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or(ApiError::not_found("Version"))
}

/// Update a version. Creates a release history entry when marking as released.
pub async fn update_version(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    ValidatedJson(input): ValidatedJson<UpdateVersionInput>,
) -> Result<Json<Version>, ApiError> {
    // Get existing version to check if this is a release (released_at: None -> Some)
    let existing = db
        .get_version(id.into())
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::not_found("Version"))?;
    let was_unreleased = existing.released_at.is_none();

    // Update the version
    let updated = db
        .update_version(id.into(), input)
        .await
        .map_err(internal_error)?
        .ok_or(ApiError::not_found("Version"))?;

    // If version was just released, create history entry and ensure minimum versions
    if was_unreleased && updated.released_at.is_some() {
        // Get project to find root_feature_id
        if let Ok(Some(project)) = db.get_project(updated.project_id).await {
            if let Some(root_feature_id) = project.root_feature_id {
                let _ = db
                    .create_history_entry(CreateHistoryInput {
                        feature_id: root_feature_id,
                        version_id: Some(updated.id),
                        details: HistoryDetails {
                            summary: format!("Released {}", updated.name),
                            commits: vec![],
                            ..Default::default()
                        },
                    })
                    .await;
            }
        }

        // Ensure at least 4 unreleased versions exist after release
        let _ = db.ensure_minimum_versions(updated.project_id, 4).await;
    }

    Ok(Json(updated))
}

/// Delete a version.
pub async fn delete_version(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    if db.delete_version(id.into()).await.map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("Version"))
    }
}
