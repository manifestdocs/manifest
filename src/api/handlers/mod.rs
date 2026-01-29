mod analysis;
mod assist;
mod features;
mod health;
mod projects;
mod versions;

pub use analysis::*;
pub use assist::*;
pub use features::*;
pub use health::*;
pub use projects::*;
pub use versions::*;

use crate::db::ManifestError;
use axum::http::StatusCode;

// ============================================================
// Shared Error Handling
// ============================================================

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
///
/// This is used by submodules via `super::internal_error`.
pub(crate) fn internal_error(e: anyhow::Error) -> (StatusCode, String) {
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
