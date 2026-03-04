//! Request handlers for the HTTP API.
//!
//! Each submodule groups handlers by resource (features, projects, versions, etc.)
//! and is wildcard re-exported so callers see a flat namespace. This module also
//! defines [`ApiError`] and the [`internal_error`] helper used across all handlers.

mod analysis;
mod features;
mod filesystem;
mod health;
mod portfolio;
mod projects;
mod proofs;
mod settings;
mod templates;
mod versions;

pub use analysis::*;
pub use features::*;
pub use filesystem::*;
pub use health::*;
pub use portfolio::*;
pub use projects::*;
pub use proofs::*;
pub use settings::*;
pub use templates::*;
pub use versions::*;

use crate::db::ManifestError;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

// ============================================================
// Shared Error Handling
// ============================================================

/// Structured JSON error response.
///
/// Returns `{"error": "message"}` by default, or a richer structured body
/// for specific error types (e.g. claim conflicts).
pub struct ApiError {
    status: StatusCode,
    message: String,
    /// Optional structured body that replaces the default `{"error": "..."}` format.
    body: Option<serde_json::Value>,
}

impl ApiError {
    /// Create a 404 Not Found error for the given entity name.
    pub fn not_found(entity: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: format!("{} not found", entity),
            body: None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = self
            .body
            .unwrap_or_else(|| serde_json::json!({ "error": self.message }));
        (self.status, Json(body)).into_response()
    }
}

/// Allow `(StatusCode, String)` to convert to `ApiError` so that existing
/// inline error returns like `Err((StatusCode::NOT_FOUND, "...".into()))` still work.
impl From<(StatusCode, String)> for ApiError {
    fn from((status, message): (StatusCode, String)) -> Self {
        Self {
            status,
            message,
            body: None,
        }
    }
}

fn manifest_error(e: ManifestError) -> ApiError {
    match &e {
        ManifestError::ClaimConflict(info) => {
            tracing::warn!("Claim conflict: {}", e);
            ApiError {
                status: StatusCode::CONFLICT,
                message: e.to_string(),
                body: Some(serde_json::json!({
                    "error": "claim_conflict",
                    "message": e.to_string(),
                    "conflict": info,
                })),
            }
        }
        _ => {
            let status = match &e {
                ManifestError::NotFound(_) => StatusCode::NOT_FOUND,
                ManifestError::Validation(_) => StatusCode::BAD_REQUEST,
                ManifestError::InvalidState(_) => StatusCode::CONFLICT,
                ManifestError::ClaimConflict(_) => unreachable!(),
            };
            tracing::warn!("Client error: {}", e);
            ApiError {
                status,
                message: e.to_string(),
                body: None,
            }
        }
    }
}

/// Convert an anyhow::Error to an HTTP response.
/// Checks if the error is a ManifestError (domain error) and handles it appropriately.
/// Other errors are treated as internal server errors.
///
/// This is used by submodules via `super::internal_error`.
pub(crate) fn internal_error(e: anyhow::Error) -> ApiError {
    // Check if this is a wrapped ManifestError (domain error)
    if let Some(manifest_err) = e.downcast_ref::<ManifestError>() {
        return manifest_error(manifest_err.clone());
    }

    // True internal error - log full details but return generic message
    tracing::error!("Internal error: {:?}", e);
    ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: "Internal server error".to_string(),
        body: None,
    }
}
