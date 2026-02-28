use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use crate::db::Database;
use crate::models::{CreateProofInput, Proof};

use super::{internal_error, ApiError};

// ============================================================
// Proofs
// ============================================================

/// Create a new proof for a feature.
pub async fn create_proof_for_feature(
    State(db): State<Database>,
    Path(feature_id): Path<Uuid>,
    Json(mut input): Json<CreateProofInput>,
) -> Result<Json<Proof>, ApiError> {
    input.feature_id = feature_id.into();
    db.create_proof(input)
        .await
        .map(Json)
        .map_err(internal_error)
}

/// List all proofs for a feature, ordered by most recent first.
pub async fn list_proofs_for_feature(
    State(db): State<Database>,
    Path(feature_id): Path<Uuid>,
) -> Result<Json<Vec<Proof>>, ApiError> {
    db.get_proofs_for_feature(feature_id.into())
        .await
        .map(Json)
        .map_err(internal_error)
}

/// Get a single proof by ID.
pub async fn get_proof(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<Proof>, ApiError> {
    db.get_proof(id.into())
        .await
        .map_err(internal_error)?
        .map(Json)
        .ok_or(ApiError::not_found("Proof"))
}
