//! Input validation for API requests.
//!
//! Validation rules live on the model input types via `#[derive(Validate)]` in manifest-core.
//! This module provides the `ValidatedJson` extractor that automatically runs validation
//! when deserializing request bodies, and utility functions used across handlers.

use axum::extract::rejection::JsonRejection;
use axum::extract::FromRequest;
use axum::http::StatusCode;
use axum::Json;
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::api::handlers::ApiError;

/// Maximum number of features allowed in a single bulk creation request.
pub const MAX_BULK_FEATURES: usize = 200;

/// An Axum extractor that deserializes JSON and runs `validator::Validate`.
///
/// Drop-in replacement for `Json<T>` — just swap `Json<T>` for `ValidatedJson<T>`
/// in handler signatures. The inner type must implement both `DeserializeOwned` and `Validate`.
pub struct ValidatedJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|e: JsonRejection| ApiError::from((StatusCode::BAD_REQUEST, e.body_text())))?;
        value.validate().map_err(|errors| {
            ApiError::from((StatusCode::BAD_REQUEST, format_validation_errors(&errors)))
        })?;
        Ok(ValidatedJson(value))
    }
}

/// Format validation errors into a human-readable string.
fn format_validation_errors(errors: &validator::ValidationErrors) -> String {
    let messages: Vec<String> = errors
        .field_errors()
        .iter()
        .flat_map(|(field, errs)| {
            errs.iter().map(move |e| {
                format!(
                    "{}: {}",
                    field,
                    e.message
                        .as_ref()
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| format!("{:?}", e.code))
                )
            })
        })
        .collect();
    messages.join("; ")
}

/// Escape special characters in LIKE patterns to prevent SQL injection.
///
/// SQLite LIKE uses % and _ as wildcards. This function escapes them
/// using \ as the escape character.
pub fn escape_like_pattern(query: &str) -> String {
    query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_like_pattern() {
        assert_eq!(escape_like_pattern("hello"), "hello");
        assert_eq!(escape_like_pattern("hello%world"), "hello\\%world");
        assert_eq!(escape_like_pattern("hello_world"), "hello\\_world");
        assert_eq!(escape_like_pattern("50% off"), "50\\% off");
        assert_eq!(escape_like_pattern("a\\b"), "a\\\\b");
    }
}
