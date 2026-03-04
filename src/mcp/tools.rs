//! MCP tool implementations grouped by resource.
//!
//! Each submodule provides the tool functions that [`McpServer`](super::server::McpServer)
//! dispatches to. The [`client_err`] helper converts [`ClientError`] variants
//! into MCP-protocol error codes.

use crate::mcp::client::ClientError;
use rmcp::ErrorData as McpError;

pub mod context;
pub mod features;
pub mod format;
pub mod generate;
pub mod projects;
pub mod spec;
pub mod sync;
pub mod versions;

/// Convert a ManifestClient error to an MCP error.
pub fn client_err(e: ClientError) -> McpError {
    match e {
        ClientError::NotFound(msg) => McpError::resource_not_found(msg, None),
        ClientError::BadRequest(msg) => McpError::invalid_params(msg, None),
        ClientError::Unauthorized => {
            McpError::internal_error("Unauthorized: check MANIFEST_API_KEY", None)
        }
        ClientError::Conflict(msg) => McpError::invalid_params(msg, None),
        ClientError::Http(e) => McpError::internal_error(e.to_string(), None),
        ClientError::Server(msg) => McpError::internal_error(msg, None),
    }
}
