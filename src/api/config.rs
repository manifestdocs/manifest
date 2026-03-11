//! Configuration for the Manifest server.
//!
//! This module provides path validation and other configuration for the local
//! self-hosted version of Manifest.

use std::sync::LazyLock;

use thiserror::Error;

/// Errors that can occur during configuration validation.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Invalid configuration: {0}")]
    Invalid(String),
}

/// Allowed directory roots for path validation.
#[derive(Debug, Clone)]
pub struct PathRestrictions {
    /// Allowed root directories for analyze_project endpoint.
    pub allowed_roots: Option<Vec<String>>,
    /// Explicitly denied paths (e.g., /etc, /var).
    pub denied_paths: Vec<String>,
}

/// Global path restrictions, constructed once from environment variables.
static PATH_RESTRICTIONS: LazyLock<PathRestrictions> = LazyLock::new(PathRestrictions::init);

impl PathRestrictions {
    /// Get the global path restrictions (constructed once, cached).
    pub fn from_env() -> &'static Self {
        &PATH_RESTRICTIONS
    }

    /// Initialize path restrictions from environment variables.
    fn init() -> Self {
        let allowed_roots = std::env::var("MANIFEST_ALLOWED_ROOTS")
            .ok()
            .map(|s| s.split(',').map(|p| p.trim().to_string()).collect());

        let mut denied_paths = vec![
            "/etc".to_string(),
            "/var".to_string(),
            "/tmp".to_string(),
            "/root".to_string(),
            "/proc".to_string(),
            "/sys".to_string(),
        ];
        #[cfg(target_os = "macos")]
        denied_paths.extend(
            ["/private/etc", "/private/var", "/private/tmp"]
                .into_iter()
                .map(str::to_string),
        );

        Self {
            allowed_roots,
            denied_paths,
        }
    }

    /// Validate that a path is allowed.
    pub fn validate(&self, path: &std::path::Path) -> Result<(), ConfigError> {
        // Must be absolute
        if !path.is_absolute() {
            return Err(ConfigError::Invalid("Path must be absolute".to_string()));
        }

        // Reject explicit parent traversal segments.
        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(ConfigError::Invalid(
                "Path must not contain '..' components".to_string(),
            ));
        }

        // Canonicalize to resolve symlinks and normalize paths. For non-existent paths,
        // canonicalize the nearest existing ancestor and append the remaining suffix.
        let canonical = canonicalize_for_validation(path);

        // Check denied paths
        for denied in &self.denied_paths {
            if canonical.starts_with(denied) {
                return Err(ConfigError::Invalid(format!(
                    "Access to {} is not allowed",
                    denied
                )));
            }
        }

        // If allowed roots are configured, path must be under one of them
        if let Some(ref roots) = self.allowed_roots {
            let allowed = roots.iter().any(|root| canonical.starts_with(root));
            if !allowed {
                return Err(ConfigError::Invalid(format!(
                    "Path must be under one of: {}",
                    roots.join(", ")
                )));
            }
        }

        Ok(())
    }
}

fn canonicalize_for_validation(path: &std::path::Path) -> std::path::PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }

    let mut suffix = Vec::new();
    let mut cursor = path;

    while !cursor.exists() {
        let Some(name) = cursor.file_name() else {
            return path.to_path_buf();
        };
        suffix.push(name.to_os_string());
        let Some(parent) = cursor.parent() else {
            return path.to_path_buf();
        };
        cursor = parent;
    }

    let mut canonical = cursor
        .canonicalize()
        .unwrap_or_else(|_| cursor.to_path_buf());
    for segment in suffix.iter().rev() {
        canonical.push(segment);
    }
    canonical
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_restrictions_denies_etc() {
        // On macOS, /etc is a symlink to /private/etc, so we need to deny the canonical path
        let denied = if cfg!(target_os = "macos") {
            "/private/etc".to_string()
        } else {
            "/etc".to_string()
        };
        let restrictions = PathRestrictions {
            allowed_roots: None,
            denied_paths: vec![denied],
        };
        // Use /etc directly as it exists on all Unix systems
        let result = restrictions.validate(std::path::Path::new("/etc"));
        assert!(result.is_err());
    }

    #[test]
    fn test_path_restrictions_requires_absolute() {
        let restrictions = PathRestrictions::from_env();
        let result = restrictions.validate(std::path::Path::new("relative/path"));
        assert!(result.is_err());
    }
}
