//! Backend-agnostic error types for the storage layer.

use chrono::{DateTime, Utc};

/// Errors returned by [`FeatureStore`](super::FeatureStore) implementations.
///
/// These errors are backend-agnostic: handler code matches on `StoreError::FeatureNotFound`,
/// never on `rusqlite::Error` or `reqwest::Error`. Backend-specific errors are wrapped in
/// the `Internal` variant.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("Feature not found: {0}")]
    FeatureNotFound(String),

    #[error("Version not found: {0}")]
    VersionNotFound(String),

    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Tree integrity violation: {0}")]
    TreeIntegrity(String),

    #[error("Conflict: {0}")]
    Conflict(ConflictDetail),

    #[error("Rate limit exceeded, resets at {reset_at}")]
    RateLimited { reset_at: DateTime<Utc> },

    #[error("Backend unavailable: {0}")]
    Unavailable(String),

    #[error("Operation not supported by this backend: {0}")]
    Unsupported(String),

    #[error("Authentication failed: {0}")]
    AuthError(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// Details about a field-level conflict during concurrent updates.
#[derive(Debug)]
pub struct ConflictDetail {
    pub feature_id: String,
    pub field: String,
    pub local_value: String,
    pub remote_value: String,
    pub remote_updated_at: DateTime<Utc>,
}

impl std::fmt::Display for ConflictDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "field '{}' on feature '{}' was modified remotely at {}",
            self.field, self.feature_id, self.remote_updated_at
        )
    }
}

/// Convenience alias for store operations.
pub type StoreResult<T> = Result<T, StoreError>;
