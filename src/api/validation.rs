//! Input validation for API requests.
//!
//! Uses the `validator` crate for declarative validation rules.

use serde::Deserialize;
use validator::Validate;

/// Maximum length for short text fields (titles, names).
pub const MAX_SHORT_TEXT: u64 = 500;

/// Maximum length for long text fields (details, descriptions).
pub const MAX_LONG_TEXT: u64 = 100_000;

/// Maximum length for email addresses.
pub const MAX_EMAIL: u64 = 254;

/// Validated input for creating a feature.
#[derive(Debug, Deserialize, Validate)]
pub struct CreateFeatureInput {
    #[validate(length(min = 1, max = 500))]
    pub title: String,

    #[validate(length(max = 100000))]
    pub details: Option<String>,

    pub parent_id: Option<String>,

    pub priority: Option<i32>,

    #[validate(custom(function = "validate_feature_state"))]
    pub state: Option<String>,
}

/// Validated input for updating a feature.
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateFeatureInput {
    #[validate(length(min = 1, max = 500))]
    pub title: Option<String>,

    #[validate(length(max = 100000))]
    pub details: Option<String>,

    pub parent_id: Option<String>,

    pub priority: Option<i32>,

    #[validate(custom(function = "validate_feature_state"))]
    pub state: Option<String>,

    pub version_id: Option<String>,
}

/// Validated input for creating a project.
#[derive(Debug, Deserialize, Validate)]
pub struct CreateProjectInput {
    #[validate(length(min = 1, max = 200))]
    pub name: String,

    #[validate(length(max = 10000))]
    pub description: Option<String>,

    #[validate(length(max = 50000))]
    pub instructions: Option<String>,
}

/// Validated input for updating a project.
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateProjectInput {
    #[validate(length(min = 1, max = 200))]
    pub name: Option<String>,

    #[validate(length(max = 10000))]
    pub description: Option<String>,

    #[validate(length(max = 50000))]
    pub instructions: Option<String>,

    /// Where new features go by default: "backlog" or "next".
    #[validate(custom(function = "validate_feature_destination"))]
    pub default_feature_destination: Option<String>,
}

/// Validated input for creating a version.
#[derive(Debug, Deserialize, Validate)]
pub struct CreateVersionInput {
    #[validate(length(min = 1, max = 100))]
    pub name: String,

    #[validate(length(max = 5000))]
    pub description: Option<String>,
}

/// Validated input for search queries.
#[derive(Debug, Deserialize, Validate)]
pub struct SearchInput {
    #[validate(length(min = 1, max = 500))]
    pub query: String,

    #[validate(range(min = 1, max = 1000))]
    pub limit: Option<u32>,

    #[validate(range(min = 0))]
    pub offset: Option<u32>,
}

/// Validated input for adding a project directory.
#[derive(Debug, Deserialize, Validate)]
pub struct AddDirectoryInput {
    #[validate(length(min = 1, max = 4096))]
    pub path: String,

    #[validate(length(max = 1000))]
    pub git_remote: Option<String>,

    pub is_primary: Option<bool>,

    #[validate(length(max = 10000))]
    pub instructions: Option<String>,
}

/// Validate default_feature_destination is one of the allowed values.
fn validate_feature_destination(dest: &str) -> Result<(), validator::ValidationError> {
    match dest {
        "backlog" | "next" => Ok(()),
        _ => {
            let mut err = validator::ValidationError::new("invalid_destination");
            err.message = Some("default_feature_destination must be one of: backlog, next".into());
            Err(err)
        }
    }
}

/// Validate feature state is one of the allowed values.
fn validate_feature_state(state: &str) -> Result<(), validator::ValidationError> {
    match state {
        "proposed" | "in_progress" | "implemented" | "archived" => Ok(()),
        _ => {
            let mut err = validator::ValidationError::new("invalid_state");
            err.message =
                Some("State must be one of: proposed, in_progress, implemented, archived".into());
            Err(err)
        }
    }
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

/// Trait extension for validation results.
pub trait ValidateExt {
    /// Validate and return a user-friendly error message.
    fn validate_input(&self) -> Result<(), String>;
}

impl<T: Validate> ValidateExt for T {
    fn validate_input(&self) -> Result<(), String> {
        self.validate().map_err(|errors| {
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
                                .unwrap_or_else(|| { format!("{:?}", e.code) })
                        )
                    })
                })
                .collect();
            messages.join("; ")
        })
    }
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

    #[test]
    fn test_validate_create_feature_valid() {
        let input = CreateFeatureInput {
            title: "Valid title".to_string(),
            details: Some("Some details".to_string()),
            parent_id: None,
            priority: Some(1),
            state: Some("proposed".to_string()),
        };
        assert!(input.validate().is_ok());
    }

    #[test]
    fn test_validate_create_feature_empty_title() {
        let input = CreateFeatureInput {
            title: "".to_string(),
            details: None,
            parent_id: None,
            priority: None,
            state: None,
        };
        assert!(input.validate().is_err());
    }

    #[test]
    fn test_validate_create_feature_invalid_state() {
        let input = CreateFeatureInput {
            title: "Valid title".to_string(),
            details: None,
            parent_id: None,
            priority: None,
            state: Some("invalid".to_string()),
        };
        assert!(input.validate().is_err());
    }

    #[test]
    fn test_validate_search_input() {
        let input = SearchInput {
            query: "test".to_string(),
            limit: Some(100),
            offset: Some(0),
        };
        assert!(input.validate().is_ok());
    }

    #[test]
    fn test_validate_search_input_limit_too_high() {
        let input = SearchInput {
            query: "test".to_string(),
            limit: Some(5000),
            offset: None,
        };
        assert!(input.validate().is_err());
    }
}
