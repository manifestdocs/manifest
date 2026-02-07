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
///
/// Hashes both inputs to a fixed-size digest before comparing, so the
/// comparison time is constant regardless of input lengths.
pub fn constant_time_compare(a: &str, b: &str) -> bool {
    use sha2::{Digest, Sha256};
    use subtle::ConstantTimeEq;

    let hash_a = Sha256::digest(a.as_bytes());
    let hash_b = Sha256::digest(b.as_bytes());

    hash_a.ct_eq(&hash_b).into()
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
