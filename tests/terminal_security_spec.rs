//! Integration tests for terminal WebSocket security.
//!
//! Verifies that the startup connection token is required for WebSocket
//! connections and that the token endpoint behaves correctly, both with
//! and without API key authentication.

use axum::http::StatusCode;
use axum_test::{TestServer, TestServerConfig, Transport};
use manifest::api::{create_router, create_router_with_config, SecurityConfig};
use manifest::db::Database;

/// Standard setup — no API key, mock transport (fine for REST endpoints).
async fn setup() -> TestServer {
    let db = Database::open_memory()
        .await
        .expect("Failed to create database");
    db.migrate().await.expect("Failed to migrate");
    let app = create_router(db);
    TestServer::new(app).expect("Failed to create test server")
}

/// Setup with real HTTP transport — required for WebSocket upgrade tests.
async fn setup_http() -> TestServer {
    let db = Database::open_memory()
        .await
        .expect("Failed to create database");
    db.migrate().await.expect("Failed to migrate");
    let app = create_router(db);
    let config = TestServerConfig {
        transport: Some(Transport::HttpRandomPort),
        ..Default::default()
    };
    TestServer::new_with_config(app, config).expect("Failed to create test server")
}

/// Setup with API key auth, mock transport.
async fn setup_with_auth(api_key: &str) -> TestServer {
    let db = Database::open_memory()
        .await
        .expect("Failed to create database");
    db.migrate().await.expect("Failed to migrate");
    let config = SecurityConfig::with_api_key(api_key);
    let app = create_router_with_config(db, config);
    TestServer::new(app).expect("Failed to create test server")
}

/// Setup with API key auth and real HTTP transport.
async fn setup_http_with_auth(api_key: &str) -> TestServer {
    let db = Database::open_memory()
        .await
        .expect("Failed to create database");
    db.migrate().await.expect("Failed to migrate");
    let config = SecurityConfig::with_api_key(api_key);
    let app = create_router_with_config(db, config);
    let test_config = TestServerConfig {
        transport: Some(Transport::HttpRandomPort),
        ..Default::default()
    };
    TestServer::new_with_config(app, test_config).expect("Failed to create test server")
}

/// Fetch the connection token from the token endpoint.
async fn fetch_token(server: &TestServer) -> String {
    let response = server.get("/api/v1/terminal/token").await;
    response.assert_status_ok();
    let body: serde_json::Value = response.json();
    body["token"]
        .as_str()
        .expect("token field missing")
        .to_string()
}

// ============================================================
// Token endpoint
// ============================================================

mod token_endpoint {
    use super::*;

    #[tokio::test]
    async fn returns_token_as_json() {
        let server = setup().await;

        let response = server.get("/api/v1/terminal/token").await;

        response.assert_status_ok();
        let body: serde_json::Value = response.json();
        let token = body["token"].as_str().expect("should have token field");
        assert!(!token.is_empty(), "token should not be empty");
    }

    #[tokio::test]
    async fn returns_stable_token_across_calls() {
        let server = setup().await;

        let token1 = fetch_token(&server).await;
        let token2 = fetch_token(&server).await;

        assert_eq!(token1, token2, "token should be stable within a process");
    }

    #[tokio::test]
    async fn requires_auth_when_api_key_configured() {
        let server = setup_with_auth("test-api-key").await;

        // Without auth header
        let response = server.get("/api/v1/terminal/token").await;
        response.assert_status(StatusCode::UNAUTHORIZED);

        // With valid auth header
        let response = server
            .get("/api/v1/terminal/token")
            .add_header("Authorization", "Bearer test-api-key")
            .await;
        response.assert_status_ok();
    }
}

// ============================================================
// WebSocket connection token validation
// ============================================================

mod ws_token_validation {
    use super::*;

    #[tokio::test]
    async fn rejects_connection_without_token() {
        let server = setup_http().await;

        let response = server
            .get("/api/v1/terminal/ws")
            .expect_failure()
            .add_header("Connection", "Upgrade")
            .add_header("Upgrade", "websocket")
            .add_header("Sec-WebSocket-Version", "13")
            .add_header("Sec-WebSocket-Key", "dGVzdA==")
            .await;

        response.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rejects_connection_with_wrong_token() {
        let server = setup_http().await;

        let response = server
            .get("/api/v1/terminal/ws?token=not-a-valid-token")
            .expect_failure()
            .add_header("Connection", "Upgrade")
            .add_header("Upgrade", "websocket")
            .add_header("Sec-WebSocket-Version", "13")
            .add_header("Sec-WebSocket-Key", "dGVzdA==")
            .await;

        response.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rejects_connection_with_empty_token() {
        let server = setup_http().await;

        let response = server
            .get("/api/v1/terminal/ws?token=")
            .expect_failure()
            .add_header("Connection", "Upgrade")
            .add_header("Upgrade", "websocket")
            .add_header("Sec-WebSocket-Version", "13")
            .add_header("Sec-WebSocket-Key", "dGVzdA==")
            .await;

        response.assert_status(StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn rejects_ws_without_auth_when_api_key_configured() {
        let server = setup_http_with_auth("test-api-key").await;

        let response = server
            .get("/api/v1/terminal/ws?token=anything")
            .expect_failure()
            .add_header("Connection", "Upgrade")
            .add_header("Upgrade", "websocket")
            .add_header("Sec-WebSocket-Version", "13")
            .add_header("Sec-WebSocket-Key", "dGVzdA==")
            .await;

        response.assert_status(StatusCode::UNAUTHORIZED);
    }
}

// ============================================================
// CORS enforcement
// ============================================================

mod cors {
    use super::*;

    #[tokio::test]
    async fn allows_same_origin_request() {
        let server = setup().await;

        let response = server
            .get("/api/v1/health")
            .add_header("Origin", "http://localhost:17010")
            .await;

        response.assert_status_ok();
        let acao = response
            .headers()
            .get("access-control-allow-origin")
            .expect("should have ACAO header");
        assert_eq!(acao, "http://localhost:17010");
    }

    #[tokio::test]
    async fn blocks_disallowed_origin() {
        let server = setup().await;

        let response = server
            .get("/api/v1/health")
            .add_header("Origin", "http://evil.com")
            .await;

        // CORS layer doesn't block the request, it omits the ACAO header
        // (the browser enforces the block based on the missing header)
        let acao = response.headers().get("access-control-allow-origin");
        assert!(
            acao.is_none(),
            "should not include ACAO for disallowed origin"
        );
    }

    #[tokio::test]
    async fn preflight_options_returns_allowed_methods() {
        let server = setup().await;

        let response = server
            .method(axum::http::Method::OPTIONS, "/api/v1/projects")
            .add_header("Origin", "http://localhost:17010")
            .add_header("Access-Control-Request-Method", "POST")
            .add_header("Access-Control-Request-Headers", "content-type")
            .await;

        // Preflight returns 200 with CORS headers
        let methods = response
            .headers()
            .get("access-control-allow-methods")
            .expect("should have allow-methods header");
        let methods_str = methods.to_str().unwrap();
        assert!(methods_str.contains("GET"), "should allow GET");
        assert!(methods_str.contains("POST"), "should allow POST");
    }
}

// ============================================================
// Security headers (OWASP)
// ============================================================

mod security_headers {
    use super::*;

    #[tokio::test]
    async fn includes_x_frame_options_deny() {
        let server = setup().await;
        let response = server.get("/api/v1/health").await;

        let header = response
            .headers()
            .get("x-frame-options")
            .expect("should have X-Frame-Options header");
        assert_eq!(header, "DENY");
    }

    #[tokio::test]
    async fn includes_x_content_type_options() {
        let server = setup().await;
        let response = server.get("/api/v1/health").await;

        let header = response
            .headers()
            .get("x-content-type-options")
            .expect("should have X-Content-Type-Options header");
        assert_eq!(header, "nosniff");
    }

    #[tokio::test]
    async fn includes_referrer_policy() {
        let server = setup().await;
        let response = server.get("/api/v1/health").await;

        let header = response
            .headers()
            .get("referrer-policy")
            .expect("should have Referrer-Policy header");
        assert_eq!(header, "strict-origin-when-cross-origin");
    }

    #[tokio::test]
    async fn includes_content_security_policy() {
        let server = setup().await;
        let response = server.get("/api/v1/health").await;

        let header = response
            .headers()
            .get("content-security-policy")
            .expect("should have CSP header");
        let value = header.to_str().unwrap();
        assert!(
            value.contains("default-src 'self'"),
            "CSP should include default-src"
        );
    }

    #[tokio::test]
    async fn includes_x_xss_protection() {
        let server = setup().await;
        let response = server.get("/api/v1/health").await;

        let header = response
            .headers()
            .get("x-xss-protection")
            .expect("should have X-XSS-Protection header");
        assert_eq!(header, "1; mode=block");
    }
}

// ============================================================
// Rate limiting (HTTP level)
// ============================================================

mod rate_limiting {
    use super::*;

    /// Setup with rate limiting enabled (low limit for testing).
    async fn setup_with_rate_limit() -> TestServer {
        let db = Database::open_memory()
            .await
            .expect("Failed to create database");
        db.migrate().await.expect("Failed to migrate");
        let config = SecurityConfig::with_rate_limit(3);
        let app = create_router_with_config(db, config);
        TestServer::new(app).expect("Failed to create test server")
    }

    #[tokio::test]
    async fn returns_429_when_rate_limit_exceeded() {
        let server = setup_with_rate_limit().await;

        // First 3 requests should succeed (using protected endpoint)
        for _ in 0..3 {
            let response = server.get("/api/v1/projects").await;
            response.assert_status_ok();
        }

        // 4th request should be rate limited
        let response = server.get("/api/v1/projects").expect_failure().await;
        response.assert_status(StatusCode::TOO_MANY_REQUESTS);
    }
}

// ============================================================
// MCP auth passthrough
// ============================================================

mod mcp_auth {
    use super::*;

    #[tokio::test]
    async fn mcp_endpoint_requires_auth_when_api_key_set() {
        let server = setup_with_auth("mcp-test-key").await;

        // MCP endpoint without auth should be rejected
        let response = server.post("/mcp").expect_failure().await;
        response.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn mcp_endpoint_accessible_without_auth_when_no_api_key() {
        let server = setup().await;

        // MCP endpoint should be accessible (may return an error since
        // we're not sending a valid MCP request, but NOT 401)
        let response = server.post("/mcp").await;
        let status = response.status_code();
        assert_ne!(
            status,
            StatusCode::UNAUTHORIZED,
            "should not require auth when no API key set"
        );
    }
}
