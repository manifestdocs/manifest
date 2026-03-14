//! Handlers for remote backend management.
//!
//! Provides HTTP endpoints for listing, adding, and querying sync status
//! of Turso remote backends.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::db::Database;
use crate::models::*;

use super::{internal_error, ApiError};
use crate::api::validation::ValidatedJson;

// ── Response Types ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct RemoteResponse {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub url: String,
    pub sync_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Remote> for RemoteResponse {
    fn from(r: Remote) -> Self {
        Self {
            id: r.id.to_string(),
            name: r.name,
            provider: r.provider,
            url: r.url,
            sync_enabled: r.sync_enabled,
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RemoteSyncStatusResponse {
    pub remote: RemoteResponse,
    pub projects: Vec<ProjectSyncEntry>,
}

#[derive(Debug, Serialize)]
pub struct ProjectSyncEntry {
    pub project_id: String,
    pub sync_state: String,
    pub last_synced_at: Option<String>,
}

impl From<ProjectRemote> for ProjectSyncEntry {
    fn from(pr: ProjectRemote) -> Self {
        Self {
            project_id: pr.project_id.to_string(),
            sync_state: pr.sync_state.as_str().to_string(),
            last_synced_at: pr.last_synced_at.map(|d| d.to_rfc3339()),
        }
    }
}

// ── Request Types ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Validate)]
pub struct CreateRemoteRequest {
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    #[validate(length(min = 1))]
    pub url: String,
    pub token: String,
    pub provider: Option<String>,
}

// ── Handlers ──────────────────────────────────────────────────────────

/// GET /api/v1/remotes
pub async fn list_remotes(
    State(db): State<Database>,
) -> Result<Json<Vec<RemoteResponse>>, ApiError> {
    let remotes = db.list_remotes().await.map_err(internal_error)?;
    Ok(Json(
        remotes.into_iter().map(RemoteResponse::from).collect(),
    ))
}

/// POST /api/v1/remotes
pub async fn create_remote(
    State(db): State<Database>,
    ValidatedJson(req): ValidatedJson<CreateRemoteRequest>,
) -> Result<(StatusCode, Json<RemoteResponse>), ApiError> {
    let input = CreateRemoteInput {
        name: req.name,
        provider: req.provider,
        url: req.url,
        token: req.token,
    };
    let remote = db.create_remote(&input).await.map_err(internal_error)?;
    Ok((StatusCode::CREATED, Json(RemoteResponse::from(remote))))
}

/// GET /api/v1/remotes/{name}/status
pub async fn get_remote_status(
    State(db): State<Database>,
    Path(name): Path<String>,
) -> Result<Json<RemoteSyncStatusResponse>, ApiError> {
    let remote = db
        .get_remote_by_name(&name)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| ApiError::not_found("Remote"))?;

    let links = db
        .get_remote_projects(remote.id)
        .await
        .map_err(internal_error)?;

    Ok(Json(RemoteSyncStatusResponse {
        remote: RemoteResponse::from(remote),
        projects: links.into_iter().map(ProjectSyncEntry::from).collect(),
    }))
}
