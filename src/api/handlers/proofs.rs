//! Test evidence storage endpoints.
//!
//! Records and retrieves [`Proof`](manifest_core::models::Proof) entries that
//! capture test results for a feature. Proofs gate feature completion — the MCP
//! layer requires a passing proof before marking a feature as implemented.

use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

use crate::db::Database;
use crate::models::{CreateProofInput, Evidence, FeatureId, HistoryId, Proof, TestSuite};

use super::{internal_error, ApiError};
use crate::api::validation::ValidatedJson;

// ============================================================
// Proofs
// ============================================================

#[derive(Debug, Deserialize, Validate)]
pub struct CreateProofRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_id: Option<HistoryId>,
    #[validate(length(min = 1, max = 2_000))]
    pub command: String,
    pub exit_code: i32,
    #[validate(length(max = 10_000))]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_suites: Option<Vec<TestSuite>>,
    #[serde(default)]
    #[validate(length(max = 200))]
    pub evidence: Vec<Evidence>,
    #[validate(length(max = 100))]
    pub commit_sha: Option<String>,
    #[validate(length(max = 50))]
    pub agent_type: Option<String>,
}

/// Create a new proof for a feature.
pub async fn create_proof_for_feature(
    State(db): State<Database>,
    Path(feature_id): Path<Uuid>,
    ValidatedJson(input): ValidatedJson<CreateProofRequest>,
) -> Result<Json<Proof>, ApiError> {
    let input = CreateProofInput {
        feature_id: FeatureId::from(feature_id),
        history_id: input.history_id,
        command: input.command,
        exit_code: input.exit_code,
        output: input.output,
        test_suites: input.test_suites,
        evidence: input.evidence,
        commit_sha: input.commit_sha,
        agent_type: input.agent_type,
    };
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
