//! Deployment mode configuration for local vs cloud environments.
//!
//! Local mode: No authentication required, single user, development use.
//! Cloud mode: Full authentication, multi-tenancy, production security.

use thiserror::Error;

/// Errors that can occur during configuration validation.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Invalid deployment mode: {0}. Expected 'local' or 'cloud'")]
    InvalidMode(String),

    #[error("Cloud mode requires the following environment variables: {}", .0.join(", "))]
    MissingCloudConfig(Vec<&'static str>),

    #[error("Invalid configuration: {0}")]
    Invalid(String),
}

/// Deployment mode determines authentication and security requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentMode {
    /// Local development mode - no authentication required.
    Local,
    /// Cloud production mode - full authentication and security.
    Cloud,
}

impl DeploymentMode {
    /// Load deployment mode from environment variables.
    ///
    /// - `MANIFEST_MODE=cloud` requires all OAuth and JWT configuration.
    /// - `MANIFEST_MODE=local` or unset defaults to local mode.
    ///
    /// In cloud mode, the server will refuse to start if required config is missing.
    pub fn from_env() -> Result<Self, ConfigError> {
        match std::env::var("MANIFEST_MODE").as_deref() {
            Ok("cloud") => {
                // REQUIRE all OAuth vars - fail startup if missing
                Self::validate_cloud_config()?;
                Ok(Self::Cloud)
            }
            Ok("local") | Err(_) => {
                // Warn if OAuth vars present but mode is local
                Self::warn_if_partial_config();
                Ok(Self::Local)
            }
            Ok(other) => Err(ConfigError::InvalidMode(other.to_string())),
        }
    }

    /// Validate that all required cloud configuration is present.
    fn validate_cloud_config() -> Result<(), ConfigError> {
        // Clerk authentication is required in cloud mode
        let required = ["CLERK_DOMAIN", "CLERK_AUTHORIZED_PARTIES"];
        let missing: Vec<_> = required
            .iter()
            .copied()
            .filter(|v| std::env::var(v).is_err())
            .collect();

        if !missing.is_empty() {
            return Err(ConfigError::MissingCloudConfig(missing));
        }

        // Validate CLERK_AUTHORIZED_PARTIES is not empty
        let parties = std::env::var("CLERK_AUTHORIZED_PARTIES").unwrap();
        if parties.trim().is_empty() {
            return Err(ConfigError::Invalid(
                "CLERK_AUTHORIZED_PARTIES cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    /// Warn if OAuth configuration is partially present in local mode.
    fn warn_if_partial_config() {
        let oauth_vars = [
            "GOOGLE_CLIENT_ID",
            "GOOGLE_CLIENT_SECRET",
            "GITHUB_CLIENT_ID",
            "GITHUB_CLIENT_SECRET",
            "JWT_SECRET",
            "SESSION_SECRET",
        ];

        let present: Vec<_> = oauth_vars
            .iter()
            .filter(|v| std::env::var(v).is_ok())
            .collect();

        if !present.is_empty() {
            tracing::warn!(
                "OAuth configuration detected but running in local mode. \
                 Set MANIFEST_MODE=cloud to enable authentication. \
                 Found: {}",
                present
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    /// Check if authentication is required.
    pub fn requires_auth(&self) -> bool {
        matches!(self, Self::Cloud)
    }

    /// Check if multi-tenancy is enabled.
    pub fn is_multi_tenant(&self) -> bool {
        matches!(self, Self::Cloud)
    }

    /// Get the mode as a string for logging.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Cloud => "cloud",
        }
    }
}

/// JWT configuration for token signing and verification.
#[derive(Debug, Clone)]
pub struct JwtConfig {
    /// Secret key for HS256 signing (minimum 256 bits).
    pub secret: String,
    /// Access token lifetime in seconds (default: 3600 = 1 hour).
    pub access_ttl_secs: u64,
    /// Refresh token lifetime in seconds (default: 2592000 = 30 days).
    pub refresh_ttl_secs: u64,
}

impl JwtConfig {
    /// Load JWT configuration from environment variables.
    pub fn from_env() -> Option<Self> {
        let secret = std::env::var("JWT_SECRET").ok()?;

        // Validate secret length (minimum 32 bytes for HS256)
        if secret.len() < 32 {
            tracing::error!("JWT_SECRET must be at least 32 characters for security");
            return None;
        }

        let access_ttl_secs = std::env::var("JWT_ACCESS_TTL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600); // 1 hour default

        let refresh_ttl_secs = std::env::var("JWT_REFRESH_TTL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(2592000); // 30 days default

        Some(Self {
            secret,
            access_ttl_secs,
            refresh_ttl_secs,
        })
    }
}

/// OAuth provider configuration.
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl OAuthConfig {
    /// Load Google OAuth configuration from environment.
    pub fn google_from_env() -> Option<Self> {
        Some(Self {
            client_id: std::env::var("GOOGLE_CLIENT_ID").ok()?,
            client_secret: std::env::var("GOOGLE_CLIENT_SECRET").ok()?,
            redirect_uri: std::env::var("GOOGLE_REDIRECT_URI").unwrap_or_else(|_| {
                "http://localhost:17010/auth/oauth/google/callback".to_string()
            }),
        })
    }

    /// Load GitHub OAuth configuration from environment.
    pub fn github_from_env() -> Option<Self> {
        Some(Self {
            client_id: std::env::var("GITHUB_CLIENT_ID").ok()?,
            client_secret: std::env::var("GITHUB_CLIENT_SECRET").ok()?,
            redirect_uri: std::env::var("GITHUB_REDIRECT_URI").unwrap_or_else(|_| {
                "http://localhost:17010/auth/oauth/github/callback".to_string()
            }),
        })
    }
}

/// Session configuration for web authentication.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Secret for signing session cookies.
    pub secret: String,
    /// Session lifetime in seconds (default: 604800 = 7 days).
    pub lifetime_secs: u64,
    /// Cookie name for the session.
    pub cookie_name: String,
}

impl SessionConfig {
    /// Load session configuration from environment.
    pub fn from_env() -> Option<Self> {
        let secret = std::env::var("SESSION_SECRET").ok()?;

        // Validate secret length
        if secret.len() < 32 {
            tracing::error!("SESSION_SECRET must be at least 32 characters for security");
            return None;
        }

        Some(Self {
            secret,
            lifetime_secs: std::env::var("SESSION_LIFETIME")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(604800), // 7 days
            cookie_name: std::env::var("SESSION_COOKIE_NAME")
                .unwrap_or_else(|_| "manifest_session".to_string()),
        })
    }
}

/// Allowed directory roots for path validation.
#[derive(Debug, Clone)]
pub struct PathRestrictions {
    /// Allowed root directories for analyze_project endpoint.
    pub allowed_roots: Option<Vec<String>>,
    /// Explicitly denied paths (e.g., /etc, /var).
    pub denied_paths: Vec<String>,
}

impl PathRestrictions {
    /// Load path restrictions from environment.
    pub fn from_env() -> Self {
        let allowed_roots = std::env::var("MANIFEST_ALLOWED_ROOTS")
            .ok()
            .map(|s| s.split(',').map(|p| p.trim().to_string()).collect());

        Self {
            allowed_roots,
            denied_paths: vec![
                "/etc".to_string(),
                "/var".to_string(),
                "/tmp".to_string(),
                "/root".to_string(),
                "/proc".to_string(),
                "/sys".to_string(),
            ],
        }
    }

    /// Validate that a path is allowed.
    pub fn validate(&self, path: &std::path::Path) -> Result<(), ConfigError> {
        // Must be absolute
        if !path.is_absolute() {
            return Err(ConfigError::Invalid("Path must be absolute".to_string()));
        }

        // Canonicalize to resolve symlinks and ..
        let canonical = path
            .canonicalize()
            .map_err(|e| ConfigError::Invalid(format!("Cannot resolve path: {}", e)))?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Mutex to ensure tests that modify MANIFEST_MODE run serially
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_deployment_mode_default_is_local() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // Clear any existing env vars
        std::env::remove_var("MANIFEST_MODE");
        let mode = DeploymentMode::from_env().unwrap();
        assert_eq!(mode, DeploymentMode::Local);
        assert!(!mode.requires_auth());
    }

    #[test]
    fn test_deployment_mode_invalid() {
        let _guard = ENV_MUTEX.lock().unwrap();
        std::env::set_var("MANIFEST_MODE", "invalid");
        let result = DeploymentMode::from_env();
        std::env::remove_var("MANIFEST_MODE"); // Cleanup first
        assert!(result.is_err());
    }

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
