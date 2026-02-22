use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;

use crate::db::Database;
use crate::models::{CreateMemoryInput, MemoryId, ProjectId, ProjectMemory, SearchMemoriesQuery};

use super::{internal_error, ApiError};

// ============================================================
// Project Memories
// ============================================================

/// Create a new project memory entry.
pub async fn create_memory(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateMemoryInput>,
) -> Result<(StatusCode, Json<ProjectMemory>), ApiError> {
    let memory = db
        .create_memory(ProjectId::from(project_id), &input)
        .await
        .map_err(internal_error)?;
    Ok((StatusCode::CREATED, Json(memory)))
}

/// Search or list project memories.
pub async fn list_memories(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Query(params): Query<SearchMemoriesQuery>,
) -> Result<Json<Vec<ProjectMemory>>, ApiError> {
    let limit = params.limit.unwrap_or(10).min(100);
    let memories = db
        .search_memories(ProjectId::from(project_id), params.q.as_deref(), limit)
        .await
        .map_err(internal_error)?;
    Ok(Json(memories))
}

/// Delete a project memory entry.
pub async fn delete_memory(
    State(db): State<Database>,
    Path((project_id, memory_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let deleted = db
        .delete_memory(ProjectId::from(project_id), MemoryId::from(memory_id))
        .await
        .map_err(internal_error)?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("Memory"))
    }
}
