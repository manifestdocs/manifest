//! Clerk JWT verification for cloud mode authentication.
//!
//! This module handles verification of JWTs issued by Clerk, using JWKS (JSON Web Key Sets)
//! to validate RS256 signatures. JWK keys are cached for 1 hour to minimize network requests.

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;

/// Errors that can occur during Clerk JWT verification.
#[derive(Debug, Error)]
pub enum ClerkError {
    #[error("Failed to fetch JWKS: {0}")]
    JwksFetch(String),

    #[error("No matching key found for kid: {0}")]
    KeyNotFound(String),

    #[error("Invalid token: {0}")]
    InvalidToken(String),

    #[error("Token expired")]
    TokenExpired,

    #[error("Invalid audience (azp): expected one of {expected:?}, got {actual}")]
    InvalidAudience {
        expected: Vec<String>,
        actual: String,
    },

    #[error("Missing required claim: {0}")]
    MissingClaim(String),
}

/// JWKS (JSON Web Key Set) response from Clerk.
#[derive(Debug, Deserialize)]
struct JwksResponse {
    keys: Vec<Jwk>,
}

/// A single JSON Web Key.
#[derive(Debug, Clone, Deserialize)]
struct Jwk {
    #[serde(rename = "kid")]
    key_id: String,
    #[serde(rename = "kty")]
    key_type: String,
    #[serde(rename = "alg")]
    algorithm: Option<String>,
    #[serde(rename = "use")]
    key_use: Option<String>,
    /// RSA modulus (base64url encoded)
    n: String,
    /// RSA exponent (base64url encoded)
    e: String,
}

/// Cached JWKS with expiration time.
#[derive(Debug)]
struct CachedJwks {
    keys: HashMap<String, Jwk>,
    fetched_at: Instant,
}

impl CachedJwks {
    fn new(keys: Vec<Jwk>) -> Self {
        let keys_map = keys.into_iter().map(|k| (k.key_id.clone(), k)).collect();
        Self {
            keys: keys_map,
            fetched_at: Instant::now(),
        }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.fetched_at.elapsed() > ttl
    }
}

/// Claims extracted from a verified Clerk JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClerkClaims {
    /// Subject - the Clerk user ID (e.g., "user_abc123")
    pub sub: String,
    /// Issued at (Unix timestamp)
    pub iat: i64,
    /// Expiration time (Unix timestamp)
    pub exp: i64,
    /// Authorized party - the frontend domain that issued this token
    #[serde(default)]
    pub azp: Option<String>,
    /// Session ID
    #[serde(default)]
    pub sid: Option<String>,
    /// User's primary email (from Clerk's custom claims)
    #[serde(default)]
    pub email: Option<String>,
    /// User's first name
    #[serde(default)]
    pub first_name: Option<String>,
    /// User's last name
    #[serde(default)]
    pub last_name: Option<String>,
    /// User's full name
    #[serde(default)]
    pub name: Option<String>,
    /// User's profile image URL
    #[serde(default)]
    pub image_url: Option<String>,
}

impl ClerkClaims {
    /// Get the display name, falling back to email or user ID.
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .or_else(|| match (&self.first_name, &self.last_name) {
                (Some(first), Some(last)) => Some(format!("{} {}", first, last)),
                (Some(first), None) => Some(first.clone()),
                (None, Some(last)) => Some(last.clone()),
                (None, None) => None,
            })
            .or_else(|| self.email.clone())
            .unwrap_or_else(|| self.sub.clone())
    }
}

/// Clerk JWT verifier with JWKS caching.
pub struct ClerkVerifier {
    /// JWKS endpoint URL (e.g., https://your-app.clerk.accounts.dev/.well-known/jwks.json)
    jwks_url: String,
    /// Cached JWKS keys
    cache: Arc<RwLock<Option<CachedJwks>>>,
    /// Cache TTL (default: 1 hour)
    cache_ttl: Duration,
    /// Authorized parties (frontend origins that can issue tokens)
    authorized_parties: Vec<String>,
    /// HTTP client for fetching JWKS
    client: reqwest::Client,
}

impl ClerkVerifier {
    /// Create a new Clerk verifier.
    ///
    /// # Arguments
    ///
    /// * `clerk_domain` - Your Clerk domain (e.g., "your-app.clerk.accounts.dev")
    /// * `authorized_parties` - List of frontend origins that can issue tokens
    pub fn new(clerk_domain: &str, authorized_parties: Vec<String>) -> Self {
        let jwks_url = format!("https://{}/.well-known/jwks.json", clerk_domain);

        Self {
            jwks_url,
            cache: Arc::new(RwLock::new(None)),
            cache_ttl: Duration::from_secs(3600), // 1 hour
            authorized_parties,
            client: reqwest::Client::new(),
        }
    }

    /// Create a verifier from environment variables.
    ///
    /// Required env vars:
    /// - `CLERK_DOMAIN`: Your Clerk domain
    /// - `CLERK_AUTHORIZED_PARTIES`: Comma-separated list of authorized origins
    pub fn from_env() -> Result<Self, ClerkError> {
        let domain = std::env::var("CLERK_DOMAIN")
            .map_err(|_| ClerkError::MissingClaim("CLERK_DOMAIN env var".to_string()))?;

        let parties = std::env::var("CLERK_AUTHORIZED_PARTIES")
            .map(|s| s.split(',').map(|p| p.trim().to_string()).collect())
            .unwrap_or_default();

        Ok(Self::new(&domain, parties))
    }

    /// Verify a JWT and return the claims.
    pub async fn verify(&self, token: &str) -> Result<ClerkClaims, ClerkError> {
        // Decode header to get the key ID (kid)
        let header = decode_header(token)
            .map_err(|e| ClerkError::InvalidToken(format!("Invalid header: {}", e)))?;

        let kid = header
            .kid
            .ok_or_else(|| ClerkError::InvalidToken("Missing kid in header".to_string()))?;

        // Get the signing key
        let jwk = self.get_key(&kid).await?;

        // Create decoding key from JWK
        let decoding_key = jwk_to_decoding_key(&jwk)?;

        // Verify the token
        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_exp = true;
        validation.validate_nbf = false;

        // Disable aud validation - Clerk doesn't use standard aud claim
        validation.validate_aud = false;

        let token_data = decode::<ClerkClaims>(token, &decoding_key, &validation).map_err(|e| {
            match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => ClerkError::TokenExpired,
                _ => ClerkError::InvalidToken(e.to_string()),
            }
        })?;

        let claims = token_data.claims;

        // Validate authorized party (azp) if configured
        if !self.authorized_parties.is_empty() {
            if let Some(ref azp) = claims.azp {
                if !self.authorized_parties.contains(azp) {
                    return Err(ClerkError::InvalidAudience {
                        expected: self.authorized_parties.clone(),
                        actual: azp.clone(),
                    });
                }
            }
            // Note: azp is optional, so we don't error if it's missing
        }

        Ok(claims)
    }

    /// Get a key from the cache or fetch from JWKS endpoint.
    async fn get_key(&self, kid: &str) -> Result<Jwk, ClerkError> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(ref cached) = *cache {
                if !cached.is_expired(self.cache_ttl) {
                    if let Some(jwk) = cached.keys.get(kid) {
                        return Ok(jwk.clone());
                    }
                }
            }
        }

        // Fetch fresh JWKS
        self.refresh_cache().await?;

        // Try again from refreshed cache
        let cache = self.cache.read().await;
        if let Some(ref cached) = *cache {
            if let Some(jwk) = cached.keys.get(kid) {
                return Ok(jwk.clone());
            }
        }

        Err(ClerkError::KeyNotFound(kid.to_string()))
    }

    /// Fetch JWKS and update the cache.
    async fn refresh_cache(&self) -> Result<(), ClerkError> {
        let response = self
            .client
            .get(&self.jwks_url)
            .send()
            .await
            .map_err(|e| ClerkError::JwksFetch(e.to_string()))?;

        if !response.status().is_success() {
            return Err(ClerkError::JwksFetch(format!("HTTP {}", response.status())));
        }

        let jwks: JwksResponse = response
            .json()
            .await
            .map_err(|e| ClerkError::JwksFetch(format!("Invalid JWKS response: {}", e)))?;

        let mut cache = self.cache.write().await;
        *cache = Some(CachedJwks::new(jwks.keys));

        Ok(())
    }
}

/// Convert a JWK to a jsonwebtoken DecodingKey.
fn jwk_to_decoding_key(jwk: &Jwk) -> Result<DecodingKey, ClerkError> {
    if jwk.key_type != "RSA" {
        return Err(ClerkError::InvalidToken(format!(
            "Unsupported key type: {}",
            jwk.key_type
        )));
    }

    // from_rsa_components expects base64url-encoded strings (not decoded bytes)
    DecodingKey::from_rsa_components(&jwk.n, &jwk.e)
        .map_err(|e| ClerkError::InvalidToken(format!("Failed to create decoding key: {}", e)))
}

impl Clone for ClerkVerifier {
    fn clone(&self) -> Self {
        Self {
            jwks_url: self.jwks_url.clone(),
            cache: Arc::clone(&self.cache),
            cache_ttl: self.cache_ttl,
            authorized_parties: self.authorized_parties.clone(),
            client: self.client.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clerk_claims_display_name() {
        // Full name takes precedence
        let claims = ClerkClaims {
            sub: "user_123".to_string(),
            iat: 0,
            exp: 0,
            azp: None,
            sid: None,
            email: Some("test@example.com".to_string()),
            first_name: Some("John".to_string()),
            last_name: Some("Doe".to_string()),
            name: Some("John Doe".to_string()),
            image_url: None,
        };
        assert_eq!(claims.display_name(), "John Doe");

        // Falls back to first + last
        let claims = ClerkClaims {
            name: None,
            ..claims.clone()
        };
        assert_eq!(claims.display_name(), "John Doe");

        // Falls back to email
        let claims = ClerkClaims {
            first_name: None,
            last_name: None,
            ..claims.clone()
        };
        assert_eq!(claims.display_name(), "test@example.com");

        // Falls back to sub
        let claims = ClerkClaims {
            email: None,
            ..claims.clone()
        };
        assert_eq!(claims.display_name(), "user_123");
    }
}
