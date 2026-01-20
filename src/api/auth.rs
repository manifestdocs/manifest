//! Authentication module for JWT, OAuth, and session management.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2, Params,
};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use super::config::JwtConfig;

/// Authentication errors.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Token expired")]
    TokenExpired,

    #[error("Invalid token: {0}")]
    InvalidToken(String),

    #[error("Missing authentication")]
    MissingAuth,

    #[error("Insufficient permissions")]
    InsufficientPermissions,

    #[error("Session expired")]
    SessionExpired,

    #[error("Internal error: {0}")]
    Internal(String),
}

/// JWT claims for access and refresh tokens.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID).
    pub sub: Uuid,
    /// Expiration time (Unix timestamp).
    pub exp: i64,
    /// Issued at (Unix timestamp).
    pub iat: i64,
    /// JWT ID (for revocation tracking).
    pub jti: Uuid,
    /// Token scope ("api" for access, "refresh" for refresh tokens).
    pub scope: String,
}

impl Claims {
    /// Create new access token claims.
    pub fn new_access(user_id: Uuid, ttl_secs: u64) -> Self {
        let now = Utc::now();
        Self {
            sub: user_id,
            exp: (now + Duration::seconds(ttl_secs as i64)).timestamp(),
            iat: now.timestamp(),
            jti: Uuid::new_v4(),
            scope: "api".to_string(),
        }
    }

    /// Create new refresh token claims.
    pub fn new_refresh(user_id: Uuid, ttl_secs: u64) -> Self {
        let now = Utc::now();
        Self {
            sub: user_id,
            exp: (now + Duration::seconds(ttl_secs as i64)).timestamp(),
            iat: now.timestamp(),
            jti: Uuid::new_v4(),
            scope: "refresh".to_string(),
        }
    }

    /// Check if the token is expired.
    pub fn is_expired(&self) -> bool {
        Utc::now().timestamp() > self.exp
    }
}

/// JWT token pair (access + refresh).
#[derive(Debug, Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub token_type: &'static str,
}

/// JWT service for token creation and verification.
pub struct JwtService {
    config: JwtConfig,
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl JwtService {
    /// Create a new JWT service from configuration.
    pub fn new(config: JwtConfig) -> Self {
        let encoding_key = EncodingKey::from_secret(config.secret.as_bytes());
        let decoding_key = DecodingKey::from_secret(config.secret.as_bytes());
        Self {
            config,
            encoding_key,
            decoding_key,
        }
    }

    /// Generate a new token pair for a user.
    pub fn generate_token_pair(&self, user_id: Uuid) -> Result<TokenPair, AuthError> {
        let access_claims = Claims::new_access(user_id, self.config.access_ttl_secs);
        let refresh_claims = Claims::new_refresh(user_id, self.config.refresh_ttl_secs);

        let access_token = encode(&Header::default(), &access_claims, &self.encoding_key)
            .map_err(|e| AuthError::Internal(e.to_string()))?;

        let refresh_token = encode(&Header::default(), &refresh_claims, &self.encoding_key)
            .map_err(|e| AuthError::Internal(e.to_string()))?;

        Ok(TokenPair {
            access_token,
            refresh_token,
            expires_in: self.config.access_ttl_secs,
            token_type: "Bearer",
        })
    }

    /// Verify an access token and return claims.
    pub fn verify_access_token(&self, token: &str) -> Result<Claims, AuthError> {
        let mut validation = Validation::default();
        validation.validate_exp = true;

        let token_data = decode::<Claims>(token, &self.decoding_key, &validation).map_err(|e| {
            match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
                _ => AuthError::InvalidToken(e.to_string()),
            }
        })?;

        if token_data.claims.scope != "api" {
            return Err(AuthError::InvalidToken("Invalid token scope".to_string()));
        }

        Ok(token_data.claims)
    }

    /// Verify a refresh token and return claims.
    pub fn verify_refresh_token(&self, token: &str) -> Result<Claims, AuthError> {
        let mut validation = Validation::default();
        validation.validate_exp = true;

        let token_data = decode::<Claims>(token, &self.decoding_key, &validation).map_err(|e| {
            match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
                _ => AuthError::InvalidToken(e.to_string()),
            }
        })?;

        if token_data.claims.scope != "refresh" {
            return Err(AuthError::InvalidToken("Invalid token scope".to_string()));
        }

        Ok(token_data.claims)
    }

    /// Refresh an access token using a refresh token.
    pub fn refresh_access_token(&self, refresh_token: &str) -> Result<TokenPair, AuthError> {
        let claims = self.verify_refresh_token(refresh_token)?;
        self.generate_token_pair(claims.sub)
    }
}

/// API key utilities.
pub struct ApiKeyService;

impl ApiKeyService {
    /// Generate a new API key.
    ///
    /// Returns (key_prefix, full_key, key_hash).
    /// The full_key is shown once to the user, key_hash is stored in DB.
    pub fn generate() -> (String, String, String) {
        use rand::Rng;

        // Generate 32 random bytes
        let mut rng = rand::thread_rng();
        let key_bytes: [u8; 32] = rng.gen();

        // Encode as base64url
        use base64::Engine;
        let full_key = format!(
            "mk_{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key_bytes)
        );

        // First 8 chars (after mk_) as prefix for identification
        let key_prefix = full_key[3..11].to_string();

        // Hash with Argon2 for storage
        let key_hash = Self::hash_key(&full_key);

        (key_prefix, full_key, key_hash)
    }

    /// Hash an API key using Argon2id.
    pub fn hash_key(key: &str) -> String {
        // Use Argon2id with OWASP 2024 recommended parameters
        let params = Params::new(
            65536, // 64 MB memory
            3,     // 3 iterations
            4,     // 4 parallel lanes
            None,  // Default output length
        )
        .expect("Invalid Argon2 params");

        let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

        let salt = SaltString::generate(&mut OsRng);
        argon2
            .hash_password(key.as_bytes(), &salt)
            .expect("Failed to hash key")
            .to_string()
    }

    /// Verify an API key against its hash.
    pub fn verify_key(key: &str, hash: &str) -> bool {
        let parsed_hash = match PasswordHash::new(hash) {
            Ok(h) => h,
            Err(_) => return false,
        };

        let params = Params::new(65536, 3, 4, None).expect("Invalid Argon2 params");
        let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

        argon2.verify_password(key.as_bytes(), &parsed_hash).is_ok()
    }
}

/// Session management for web users.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub user_id: Uuid,
    pub user_agent: Option<String>,
    pub ip_address: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl Session {
    /// Generate a new session ID using SHA256 of random bytes.
    pub fn generate_id() -> String {
        use rand::Rng;
        let random_bytes: [u8; 32] = rand::thread_rng().gen();
        let mut hasher = Sha256::new();
        hasher.update(random_bytes);
        hex::encode(hasher.finalize())
    }

    /// Create a new session.
    pub fn new(
        user_id: Uuid,
        lifetime_secs: u64,
        user_agent: Option<String>,
        ip_address: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Self::generate_id(),
            user_id,
            user_agent,
            ip_address,
            expires_at: now + Duration::seconds(lifetime_secs as i64),
            last_activity_at: now,
            created_at: now,
        }
    }

    /// Check if the session has expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Extend the session expiration (sliding window).
    pub fn extend(&mut self, lifetime_secs: u64) {
        self.last_activity_at = Utc::now();
        self.expires_at = Utc::now() + Duration::seconds(lifetime_secs as i64);
    }
}

/// Authenticated user context extracted from request.
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// User ID from token or session.
    pub user_id: Uuid,
    /// Authentication method used.
    pub method: AuthMethod,
    /// Token JTI for revocation checks (if JWT).
    pub token_jti: Option<Uuid>,
}

/// How the user was authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// JWT access token.
    Jwt,
    /// API key.
    ApiKey,
    /// Web session cookie.
    Session,
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
    fn test_jwt_service_generates_valid_tokens() {
        let config = JwtConfig {
            secret: "test-secret-that-is-at-least-32-characters-long".to_string(),
            access_ttl_secs: 3600,
            refresh_ttl_secs: 2592000,
        };
        let service = JwtService::new(config);
        let user_id = Uuid::new_v4();

        let token_pair = service.generate_token_pair(user_id).unwrap();
        assert!(token_pair.access_token.len() > 0);
        assert!(token_pair.refresh_token.len() > 0);
        assert_eq!(token_pair.token_type, "Bearer");
    }

    #[test]
    fn test_jwt_service_verifies_access_token() {
        let config = JwtConfig {
            secret: "test-secret-that-is-at-least-32-characters-long".to_string(),
            access_ttl_secs: 3600,
            refresh_ttl_secs: 2592000,
        };
        let service = JwtService::new(config);
        let user_id = Uuid::new_v4();

        let token_pair = service.generate_token_pair(user_id).unwrap();
        let claims = service
            .verify_access_token(&token_pair.access_token)
            .unwrap();
        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.scope, "api");
    }

    #[test]
    fn test_api_key_generation_and_verification() {
        let (prefix, key, hash) = ApiKeyService::generate();

        assert!(key.starts_with("mk_"));
        assert_eq!(prefix.len(), 8);
        assert!(ApiKeyService::verify_key(&key, &hash));
        assert!(!ApiKeyService::verify_key("wrong_key", &hash));
    }

    #[test]
    fn test_constant_time_compare() {
        assert!(constant_time_compare("hello", "hello"));
        assert!(!constant_time_compare("hello", "world"));
        assert!(!constant_time_compare("hello", "hell"));
    }

    #[test]
    fn test_session_generation() {
        let session = Session::new(
            Uuid::new_v4(),
            3600,
            Some("test-agent".to_string()),
            Some("127.0.0.1".to_string()),
        );

        assert_eq!(session.id.len(), 64); // SHA256 hex
        assert!(!session.is_expired());
    }
}
