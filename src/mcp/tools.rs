use crate::mcp::client::ClientError;
use rmcp::ErrorData as McpError;

pub mod context;
pub mod features;
pub mod format;
pub mod generate;
pub mod memories;
pub mod projects;
pub mod spec;
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
