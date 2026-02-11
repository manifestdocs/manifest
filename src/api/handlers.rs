mod analysis;
mod features;
mod filesystem;
mod health;
mod projects;
mod settings;
pub mod terminal;
mod versions;

pub use analysis::*;
pub use features::*;
pub use filesystem::*;
pub use health::*;
pub use projects::*;
pub use settings::*;
pub use terminal::ws_terminal_handler;
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
/// Returns `{"error": "message"}` instead of plain text.
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    /// Create a 404 Not Found error for the given entity name.
    pub fn not_found(entity: &str) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: format!("{} not found", entity),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({ "error": self.message });
        (self.status, Json(body)).into_response()
    }
}

/// Allow `(StatusCode, String)` to convert to `ApiError` so that existing
/// inline error returns like `Err((StatusCode::NOT_FOUND, "...".into()))` still work.
impl From<(StatusCode, String)> for ApiError {
    fn from((status, message): (StatusCode, String)) -> Self {
        Self { status, message }
    }
}

fn manifest_error(e: ManifestError) -> ApiError {
    let status = match &e {
        ManifestError::NotFound(_) => StatusCode::NOT_FOUND,
        ManifestError::Validation(_) => StatusCode::BAD_REQUEST,
        ManifestError::InvalidState(_) => StatusCode::CONFLICT,
    };
    tracing::warn!("Client error: {}", e);
    ApiError {
        status,
        message: e.to_string(),
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
    }
}
