mod handlers;
mod middleware;

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::db::Database;
use crate::mcp;

pub use middleware::SecurityConfig;

/// Build CORS layer based on configuration
fn build_cors_layer(config: &SecurityConfig) -> CorsLayer {
    use axum::http::{header, Method};
    use tower_http::cors::AllowOrigin;

    if let Some(ref origins) = config.cors_origins {
        let origins: Vec<_> = origins.iter().filter_map(|s| s.parse().ok()).collect();
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
    } else {
        CorsLayer::permissive()
    }
}

pub fn create_router(db: Database) -> Router {
    create_router_with_config(db, SecurityConfig::from_env())
}

pub fn create_router_with_config(db: Database, config: SecurityConfig) -> Router {
    // Health endpoint (unauthenticated)
    let health_router = Router::new().route("/health", get(handlers::health));

    // Protected API routes
    let protected_api = Router::new()
        // Codebase analysis (separate path to avoid {id} conflicts)
        .route("/codebase/analyze", get(handlers::analyze_project))
        // Projects
        .route("/projects", get(handlers::list_projects))
        .route("/projects", post(handlers::create_project))
        .route(
            "/projects/by-directory",
            get(handlers::get_project_by_directory),
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
        // SSE subscription for real-time feature updates
        .route(
            "/projects/{id}/subscribe",
            get(handlers::subscribe_project_features),
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
            "/features/{id}/history",
            get(handlers::get_feature_history).post(handlers::create_feature_history),
        );

    // Apply auth middleware to protected routes if API key is configured
    let protected_api = if config.api_key.is_some() {
        protected_api.layer(axum::middleware::from_fn_with_state(
            config.clone(),
            middleware::auth_middleware,
        ))
    } else {
        protected_api
    };

    // Apply rate limiting if configured
    let protected_api = if let Some(rate_limiter) = config.rate_limiter.clone() {
        protected_api.layer(axum::middleware::from_fn_with_state(
            rate_limiter,
            middleware::rate_limit_middleware,
        ))
    } else {
        protected_api
    };

    let cors_layer = build_cors_layer(&config);

    // Combine health (unauthenticated) with protected API
    let api = health_router.merge(protected_api);

    // MCP router is stateless (uses its own HTTP client internally)
    let mcp_router = mcp::streamable_http_router();

    Router::new()
        .nest("/api/v1", api)
        .with_state(db)
        .nest("/mcp", mcp_router)
        .layer(TraceLayer::new_for_http())
        .layer(cors_layer)
}
