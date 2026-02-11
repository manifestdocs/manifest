//! Security middleware for API authentication and rate limiting.

use crate::api::auth::constant_time_compare;
use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

/// Security configuration loaded from environment variables.
#[derive(Clone, Debug)]
pub struct SecurityConfig {
    /// API key for authentication (from MANIFEST_API_KEY)
    pub api_key: Option<String>,
    /// Allowed CORS origins (from MANIFEST_CORS_ORIGINS, comma-separated)
    pub cors_origins: Option<Vec<String>>,
    /// Rate limiter instance
    pub rate_limiter: Option<RateLimiter>,
}

impl SecurityConfig {
    /// Load security configuration from environment variables.
    pub fn from_env() -> Self {
        let api_key = std::env::var("MANIFEST_API_KEY").ok();

        let cors_origins = std::env::var("MANIFEST_CORS_ORIGINS")
            .ok()
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect());

        let rate_limit = std::env::var("MANIFEST_RATE_LIMIT")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(100); // Default: 100 requests per minute

        // Enable rate limiting in cloud mode OR if API key is set
        let is_cloud_mode = std::env::var("MANIFEST_MODE")
            .map(|m| m.to_lowercase() == "cloud")
            .unwrap_or(false);

        let rate_limiter = if api_key.is_some() || is_cloud_mode {
            Some(RateLimiter::new(rate_limit, Duration::from_secs(60)))
        } else {
            None
        };

        Self {
            api_key,
            cors_origins,
            rate_limiter,
        }
    }

    /// Create security config for cloud mode with rate limiting enabled.
    pub fn for_cloud_mode() -> Self {
        let cors_origins = std::env::var("MANIFEST_CORS_ORIGINS")
            .ok()
            .map(|s| s.split(',').map(|s| s.trim().to_string()).collect());

        let rate_limit = std::env::var("MANIFEST_RATE_LIMIT")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(100);

        Self {
            api_key: None,
            cors_origins,
            rate_limiter: Some(RateLimiter::new(rate_limit, Duration::from_secs(60))),
        }
    }

    /// Create a config with no authentication (for local development/testing).
    pub fn disabled() -> Self {
        Self {
            api_key: None,
            cors_origins: None,
            rate_limiter: None,
        }
    }

    /// Create a config with authentication enabled (for testing).
    pub fn with_api_key(key: impl Into<String>) -> Self {
        Self {
            api_key: Some(key.into()),
            cors_origins: None,
            rate_limiter: None,
        }
    }

    /// Create a config with specific CORS origins.
    pub fn with_cors_origins(origins: Vec<String>) -> Self {
        Self {
            api_key: None,
            cors_origins: Some(origins),
            rate_limiter: None,
        }
    }

    /// Create a config with rate limiting enabled.
    pub fn with_rate_limit(max_requests: u32) -> Self {
        Self {
            api_key: None,
            cors_origins: None,
            rate_limiter: Some(RateLimiter::new(
                max_requests,
                std::time::Duration::from_secs(60),
            )),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

/// Simple in-memory rate limiter using sliding window.
#[derive(Clone, Debug)]
pub struct RateLimiter {
    /// Maximum requests allowed per window
    max_requests: u32,
    /// Time window duration
    window: Duration,
    /// Request counts per IP
    requests: Arc<Mutex<HashMap<IpAddr, Vec<Instant>>>>,
}

impl RateLimiter {
    /// Create a new rate limiter.
    pub fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if a request from the given IP is allowed.
    /// Returns true if allowed, false if rate limited.
    pub fn check(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let cutoff = now - self.window;

        let mut requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());
        let entry = requests.entry(ip).or_default();

        // Remove expired entries
        entry.retain(|&t| t > cutoff);

        if entry.len() < self.max_requests as usize {
            entry.push(now);
            true
        } else {
            false
        }
    }

    /// Clean up old entries to prevent memory growth.
    pub fn cleanup(&self) {
        let cutoff = Instant::now() - self.window;
        let mut requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());

        requests.retain(|_, timestamps| {
            timestamps.retain(|&t| t > cutoff);
            !timestamps.is_empty()
        });
    }

    /// Spawn a background task that periodically cleans up expired entries.
    pub fn start_cleanup_task(&self) {
        let limiter = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                limiter.cleanup();
            }
        });
    }
}

/// Authentication middleware that checks for valid API key.
pub async fn auth_middleware(
    State(config): State<SecurityConfig>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected_key = match &config.api_key {
        Some(key) => key,
        None => return Ok(next.run(request).await),
    };

    let auth_header = request
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok());

    match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let token = &header[7..];
            // Use constant-time comparison to prevent timing attacks
            if constant_time_compare(token, expected_key) {
                Ok(next.run(request).await)
            } else {
                tracing::warn!("Invalid API key provided");
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        Some(_) => {
            tracing::warn!("Invalid Authorization header format");
            Err(StatusCode::UNAUTHORIZED)
        }
        None => {
            tracing::warn!("Missing Authorization header");
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

/// Rate limiting middleware.
pub async fn rate_limit_middleware(
    State(rate_limiter): State<RateLimiter>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract client IP from request
    // In production, you'd want to check X-Forwarded-For or similar headers
    let ip = extract_client_ip(&request);

    if rate_limiter.check(ip) {
        Ok(next.run(request).await)
    } else {
        tracing::warn!("Rate limit exceeded for IP: {}", ip);
        Err(StatusCode::TOO_MANY_REQUESTS)
    }
}

/// Extract client IP from request.
///
/// Only trusts proxy headers (X-Forwarded-For, X-Real-IP) when
/// `MANIFEST_TRUST_PROXY=true` is set. Without this, a malicious client
/// could spoof their IP to bypass rate limiting.
fn extract_client_ip(request: &Request<Body>) -> IpAddr {
    static TRUST_PROXY: OnceLock<bool> = OnceLock::new();
    let trust_proxy = *TRUST_PROXY.get_or_init(|| {
        std::env::var("MANIFEST_TRUST_PROXY")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false)
    });

    if trust_proxy {
        // Try X-Forwarded-For header first (for proxied requests)
        if let Some(forwarded) = request.headers().get("X-Forwarded-For") {
            if let Ok(value) = forwarded.to_str() {
                if let Some(ip_str) = value.split(',').next() {
                    if let Ok(ip) = ip_str.trim().parse() {
                        return ip;
                    }
                }
            }
        }

        // Try X-Real-IP header
        if let Some(real_ip) = request.headers().get("X-Real-IP") {
            if let Ok(value) = real_ip.to_str() {
                if let Ok(ip) = value.trim().parse() {
                    return ip;
                }
            }
        }
    }

    // Default to localhost for local development
    "127.0.0.1".parse().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn rate_limiter_allows_requests_under_limit() {
        let limiter = RateLimiter::new(5, Duration::from_secs(60));
        let ip: IpAddr = "192.168.1.1".parse().unwrap();

        for _ in 0..5 {
            assert!(limiter.check(ip));
        }
    }

    #[test]
    fn rate_limiter_blocks_requests_over_limit() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        let ip: IpAddr = "192.168.1.1".parse().unwrap();

        // First 3 requests should be allowed
        assert!(limiter.check(ip));
        assert!(limiter.check(ip));
        assert!(limiter.check(ip));

        // 4th request should be blocked
        assert!(!limiter.check(ip));
    }

    #[test]
    fn rate_limiter_tracks_ips_independently() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        let ip1: IpAddr = "192.168.1.1".parse().unwrap();
        let ip2: IpAddr = "192.168.1.2".parse().unwrap();

        // Both IPs should have their own limit
        assert!(limiter.check(ip1));
        assert!(limiter.check(ip1));
        assert!(!limiter.check(ip1));

        assert!(limiter.check(ip2));
        assert!(limiter.check(ip2));
        assert!(!limiter.check(ip2));
    }

    #[test]
    fn security_config_disabled_has_no_auth() {
        let config = SecurityConfig::disabled();
        assert!(config.api_key.is_none());
        assert!(config.cors_origins.is_none());
        assert!(config.rate_limiter.is_none());
    }

    #[test]
    fn security_config_with_api_key_has_auth() {
        let config = SecurityConfig::with_api_key("test-key");
        assert_eq!(config.api_key, Some("test-key".to_string()));
    }
}
