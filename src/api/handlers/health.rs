//! Health check and version information endpoints.
//!
//! Stateless endpoints that require no authentication or database access.

use axum::{response::IntoResponse, Json};

/// Health check endpoint returning server status.
pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

/// Version endpoint returning current server version and releases URL.
pub async fn version() -> impl IntoResponse {
    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "releases_url": "https://api.github.com/repos/manifestdocs/manifest/releases/latest",
    }))
}
