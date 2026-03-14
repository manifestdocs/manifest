//! HTTP API layer for Manifest.
//!
//! Builds the Axum router with public routes (health, SSE subscriptions) and
//! protected CRUD routes under `/api/v1`. Applies middleware for CORS, security
//! headers, and optional API-key authentication.

pub mod auth;
pub mod config;
mod handlers;
mod middleware;
pub mod state;
pub mod validation;

use std::sync::OnceLock;

use axum::{
    extract::DefaultBodyLimit,
    http::{header::HeaderName, HeaderValue},
    routing::{delete, get, post, put},
    Router,
};
use tokio::sync::watch;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, set_header::SetResponseHeaderLayer, trace::TraceLayer};

#[cfg(feature = "embed-web")]
use crate::assets::static_handler;
use crate::db::Database;
use crate::mcp;

pub use auth::{AuthContext, AuthError, AuthMethod};
pub use middleware::SecurityConfig;
pub use state::{AppState, CurrentUser};

/// Global shutdown sender, set by main() before starting the server.
static SHUTDOWN_TX: OnceLock<watch::Sender<bool>> = OnceLock::new();

/// Store the shutdown sender so the settings handler can trigger a restart.
pub fn set_shutdown_sender(tx: watch::Sender<bool>) {
    let _ = SHUTDOWN_TX.set(tx);
}

/// Trigger a graceful shutdown (called by the settings handler after config change).
pub fn trigger_shutdown() {
    if let Some(tx) = SHUTDOWN_TX.get() {
        let _ = tx.send(true);
    }
}

/// Security headers for all responses.
#[allow(clippy::type_complexity)] // Tower's nested layer types are inherently complex
fn security_headers_layer() -> ServiceBuilder<
    tower::layer::util::Stack<
        SetResponseHeaderLayer<HeaderValue>,
        tower::layer::util::Stack<
            SetResponseHeaderLayer<HeaderValue>,
            tower::layer::util::Stack<
                SetResponseHeaderLayer<HeaderValue>,
                tower::layer::util::Stack<
                    SetResponseHeaderLayer<HeaderValue>,
                    tower::layer::util::Stack<
                        SetResponseHeaderLayer<HeaderValue>,
                        tower::layer::util::Identity,
                    >,
                >,
            >,
        >,
    >,
> {
    // Security headers per OWASP recommendations
    let x_frame_options = HeaderName::from_static("x-frame-options");
    let x_content_type_options = HeaderName::from_static("x-content-type-options");
    let referrer_policy = HeaderName::from_static("referrer-policy");
    let x_xss_protection = HeaderName::from_static("x-xss-protection");
    let csp = HeaderName::from_static("content-security-policy");

    ServiceBuilder::new()
        // Prevent clickjacking
        .layer(SetResponseHeaderLayer::overriding(
            x_frame_options,
            HeaderValue::from_static("DENY"),
        ))
        // Prevent MIME type sniffing
        .layer(SetResponseHeaderLayer::overriding(
            x_content_type_options,
            HeaderValue::from_static("nosniff"),
        ))
        // Control referrer information
        .layer(SetResponseHeaderLayer::overriding(
            referrer_policy,
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        ))
        // Legacy XSS protection (for older browsers)
        .layer(SetResponseHeaderLayer::overriding(
            x_xss_protection,
            HeaderValue::from_static("1; mode=block"),
        ))
        // Content Security Policy
        .layer(SetResponseHeaderLayer::overriding(
            csp,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self' http://localhost:* http://127.0.0.1:* ws://localhost:* ws://127.0.0.1:* https://api.github.com",
            ),
        ))
}

/// Build CORS layer based on configuration
fn build_cors_layer(config: &SecurityConfig) -> CorsLayer {
    use axum::http::{header, Method};
    use tower_http::cors::AllowOrigin;

    let origins: Vec<_> = config
        .resolved_origins()
        .into_iter()
        .filter_map(|s| s.parse().ok())
        .collect();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
}

/// Apply auth middleware to a router if an API key is configured.
fn apply_auth_layer<S: Clone + Send + Sync + 'static>(
    router: Router<S>,
    config: &SecurityConfig,
) -> Router<S> {
    if config.api_key.is_some() {
        router.layer(axum::middleware::from_fn_with_state(
            config.clone(),
            middleware::auth_middleware,
        ))
    } else {
        router
    }
}

/// Create the API router with default security configuration.
pub fn create_router(db: Database) -> Router {
    create_router_with_config(db, SecurityConfig::from_env())
}

/// Create the API router with custom security configuration.
pub fn create_router_with_config(db: Database, config: SecurityConfig) -> Router {
    create_router_inner(db, config)
}

fn create_router_inner(db: Database, config: SecurityConfig) -> Router {
    let app_state = AppState::new(db);
    let public_router = build_public_router();
    let protected_api = apply_protected_layers(build_protected_api_router(), &config);

    let cors_layer = build_cors_layer(&config);

    // Combine public (unauthenticated) with protected API
    let api = public_router.merge(protected_api);

    // MCP router is stateless (uses its own HTTP client internally)
    let mcp_router = mcp::streamable_http_router();

    // Apply auth middleware to MCP router if API key is configured
    let mcp_router = apply_auth_layer(mcp_router, &config);

    let router = Router::new()
        .nest("/api/v1", api)
        .with_state(app_state)
        .nest("/mcp", mcp_router);

    #[cfg(feature = "embed-web")]
    let router = router.fallback(static_handler);

    router
        .layer(DefaultBodyLimit::max(2_000_000)) // 2MB
        .layer(TraceLayer::new_for_http())
        .layer(cors_layer)
        .layer(security_headers_layer())
}

fn build_public_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/version", get(handlers::version))
        .route(
            "/projects/{id}/subscribe",
            get(handlers::subscribe_project_features),
        )
        .route("/portfolio/events", get(handlers::subscribe_portfolio))
}

fn build_protected_api_router() -> Router<AppState> {
    Router::new()
        // Codebase analysis (separate path to avoid {id} conflicts)
        .route("/codebase/analyze", get(handlers::analyze_project))
        // Filesystem browsing and management
        .route("/filesystem/browse", get(handlers::browse_filesystem))
        .route("/filesystem/mkdir", post(handlers::create_directory))
        // Projects
        .route("/projects", get(handlers::list_projects))
        .route("/projects", post(handlers::create_project))
        .route(
            "/projects/by-directory",
            get(handlers::get_project_by_directory),
        )
        .route(
            "/projects/by-slug/{slug}",
            get(handlers::get_project_by_slug),
        )
        .route("/projects/{id}", get(handlers::get_project))
        .route("/projects/{id}", put(handlers::update_project))
        .route("/projects/{id}", delete(handlers::delete_project))
        .route(
            "/projects/{id}/directories",
            get(handlers::list_project_directories),
        )
        .route(
            "/projects/{id}/directories",
            post(handlers::add_project_directory),
        )
        .route("/projects/{id}/history", get(handlers::get_project_history))
        .route(
            "/projects/{id}/focus",
            get(handlers::get_project_focus).put(handlers::set_project_focus),
        )
        // Spec Template (one per project)
        .route(
            "/projects/{id}/template",
            get(handlers::get_project_template).put(handlers::update_project_template),
        )
        // Versions
        .route(
            "/projects/{id}/versions",
            get(handlers::list_project_versions).post(handlers::create_version),
        )
        .route("/versions/{id}", get(handlers::get_version))
        .route("/versions/{id}", put(handlers::update_version))
        .route("/versions/{id}", delete(handlers::delete_version))
        .route(
            "/projects/{id}/features",
            get(handlers::list_project_features),
        )
        .route("/projects/{id}/features", post(handlers::create_feature))
        .route(
            "/projects/{id}/features/bulk",
            post(handlers::bulk_create_features),
        )
        .route(
            "/projects/{id}/features/roots",
            get(handlers::list_root_features),
        )
        .route(
            "/projects/{id}/features/tree",
            get(handlers::get_feature_tree),
        )
        // Directories (for delete by directory id)
        .route(
            "/directories/{id}",
            delete(handlers::remove_project_directory),
        )
        // Next workable feature
        .route(
            "/projects/{id}/features/next",
            get(handlers::get_next_feature),
        )
        // Features (by feature id)
        .route("/features", get(handlers::list_features))
        .route("/features/resolve", get(handlers::resolve_feature))
        .route("/features/search", get(handlers::search_features))
        .route("/features/{id}", get(handlers::get_feature))
        .route("/features/{id}", put(handlers::update_feature))
        .route("/features/{id}", delete(handlers::delete_feature))
        .route("/features/{id}/children", get(handlers::list_children))
        .route(
            "/features/{id}/context",
            get(handlers::get_feature_with_context),
        )
        .route("/features/{id}/diff", get(handlers::get_feature_diff))
        .route(
            "/features/{id}/blockers",
            get(handlers::get_feature_blockers),
        )
        .route(
            "/features/{id}/dependents",
            get(handlers::get_feature_dependents),
        )
        .route(
            "/features/{id}/blocked-ancestor",
            get(handlers::find_blocked_ancestor),
        )
        .route(
            "/features/{id}/history",
            get(handlers::get_feature_history).post(handlers::create_feature_history),
        )
        .route("/features/{id}/verify", post(handlers::get_verify_context))
        .route(
            "/features/{id}/verification",
            put(handlers::record_feature_verification),
        )
        .route("/features/{id}/claim", put(handlers::set_feature_claim))
        .route("/features/{id}/complete", post(handlers::complete_feature))
        // Proofs
        .route(
            "/features/{id}/proofs",
            get(handlers::list_proofs_for_feature).post(handlers::create_proof_for_feature),
        )
        .route("/proofs/{id}", get(handlers::get_proof))
        // Remotes
        .route(
            "/remotes",
            get(handlers::list_remotes).post(handlers::create_remote),
        )
        .route("/remotes/{name}/status", get(handlers::get_remote_status))
        // Portfolio
        .route("/portfolio", get(handlers::get_portfolio))
        // Server settings
        .route(
            "/settings",
            get(handlers::get_settings).put(handlers::update_settings),
        )
        .route("/settings/mcp-status", get(handlers::check_mcp_status))
        .route("/settings/configure-mcp", post(handlers::configure_mcp))
}

fn apply_protected_layers(
    protected_api: Router<AppState>,
    config: &SecurityConfig,
) -> Router<AppState> {
    let protected_api = apply_auth_layer(protected_api, config);

    if let Some(rate_limiter) = config.rate_limiter.clone() {
        rate_limiter.start_cleanup_task();
        protected_api.layer(axum::middleware::from_fn_with_state(
            rate_limiter,
            middleware::rate_limit_middleware,
        ))
    } else {
        protected_api
    }
}
