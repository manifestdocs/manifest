//! MCP server for AI-assisted feature development.

pub mod client;
pub mod git;
mod server;
mod tools;
mod tree_render;
mod types;

pub use client::ManifestClient;
pub use server::McpServer;
pub use types::*;

/// Run the MCP server using stdio transport.
pub async fn run_stdio_server() -> anyhow::Result<()> {
    use rmcp::ServiceExt;
    use tokio::io::{stdin, stdout};

    tracing::info!("Starting MCP server via stdio");
    let service = McpServer::from_env();
    let server = service.serve((stdin(), stdout())).await?;
    let quit_reason = server.waiting().await?;
    tracing::info!("MCP server stopped: {:?}", quit_reason);

    Ok(())
}

/// Create Axum router for Streamable HTTP MCP transport.
///
/// This provides SSE-based MCP transport at `/mcp` endpoint, allowing
/// AI agents to access Manifest tools via HTTP instead of stdio.
pub fn streamable_http_router() -> axum::Router {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, tower::StreamableHttpService,
    };
    use std::sync::Arc;

    let service = StreamableHttpService::new(
        || Ok(McpServer::from_env()),
        Arc::new(LocalSessionManager::default()),
        Default::default(),
    );
    axum::Router::new().fallback_service(service)
}
