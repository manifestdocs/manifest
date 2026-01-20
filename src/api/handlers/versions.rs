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
    // Get existing version to check if this is a release (released_at: None -> Some)
    let existing = db
        .get_version(id)
        .map_err(internal_error)?
        .ok_or((StatusCode::NOT_FOUND, "Version not found".to_string()))?;
    let was_unreleased = existing.released_at.is_none();

    // Update the version
    let updated = db
        .update_version(id, input)
        .map_err(internal_error)?
        .ok_or((StatusCode::NOT_FOUND, "Version not found".to_string()))?;

    // If version was just released, create history entry on root feature
    if was_unreleased && updated.released_at.is_some() {
        // Get project to find root_feature_id
        if let Ok(Some(project)) = db.get_project(updated.project_id) {
            if let Some(root_feature_id) = project.root_feature_id {
                let _ = db.create_history_entry(CreateHistoryInput {
                    feature_id: root_feature_id,
                    version_id: Some(updated.id),
                    details: HistoryDetails {
                        summary: format!("Released {}", updated.name),
                        commits: vec![],
                    },
                });
            }
        }
    }

    Ok(Json(updated))
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
