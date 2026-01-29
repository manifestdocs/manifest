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

use super::internal_error;

// ============================================================
// Versions
// ============================================================

/// List all versions for a project.
pub async fn list_project_versions(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<Version>>, (StatusCode, String)> {
    db.get_versions_by_project(project_id)
        .await
        .map(Json)
        .map_err(internal_error)
}

/// Create a new version for a project.
pub async fn create_version(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateVersionInput>,
) -> Result<(StatusCode, Json<Version>), (StatusCode, String)> {
    db.create_version(project_id, input)
        .await
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(internal_error)
}

/// Get a version by ID.
pub async fn get_version(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<Version>, (StatusCode, String)> {
    db.get_version(id)
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Version not found".to_string()))
}

/// Update a version. Creates a release history entry when marking as released.
pub async fn update_version(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateVersionInput>,
) -> Result<Json<Version>, (StatusCode, String)> {
    // Get existing version to check if this is a release (released_at: None -> Some)
    let existing = db
        .get_version(id)
        .await
        .map_err(internal_error)?
        .ok_or((StatusCode::NOT_FOUND, "Version not found".to_string()))?;
    let was_unreleased = existing.released_at.is_none();

    // Update the version
    let updated = db
        .update_version(id, input)
        .await
        .map_err(internal_error)?
        .ok_or((StatusCode::NOT_FOUND, "Version not found".to_string()))?;

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
) -> Result<StatusCode, (StatusCode, String)> {
    if db.delete_version(id).await.map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "Version not found".to_string()))
    }
}
