use axum::{response::IntoResponse, Json};

/// Health check endpoint returning server status.
pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}
