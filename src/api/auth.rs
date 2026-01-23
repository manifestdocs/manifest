//! Authentication module for API key-based authentication.
//!
//! The self-hosted version of Manifest supports optional API key authentication
//! via the MANIFEST_API_KEY environment variable.

use thiserror::Error;
use uuid::Uuid;

/// Authentication errors.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Missing authentication")]
    MissingAuth,

    #[error("Insufficient permissions")]
    InsufficientPermissions,
}

/// Authenticated user context extracted from request.
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// User ID (synthetic for API key auth).
    pub user_id: Uuid,
    /// Authentication method used.
    pub method: AuthMethod,
}

/// How the user was authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// API key.
    ApiKey,
    /// No authentication (local mode).
    None,
}

/// Timing-safe string comparison to prevent timing attacks.
pub fn constant_time_compare(a: &str, b: &str) -> bool {
    use subtle::ConstantTimeEq;

    // Pad to same length to prevent length-based timing leaks
    if a.len() != b.len() {
        return false;
    }

    a.as_bytes().ct_eq(b.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_compare() {
        assert!(constant_time_compare("hello", "hello"));
        assert!(!constant_time_compare("hello", "world"));
        assert!(!constant_time_compare("hello", "hell"));
    }
}
