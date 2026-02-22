use rmcp::{
    model::{CallToolResult, Content},
    ErrorData as McpError,
};

use crate::mcp::{
    types::{ForgetRequest, RecallRequest, RememberRequest},
    ManifestClient,
};

use super::client_err;

/// Store a memory in the project's memory store.
pub async fn remember(
    client: &ManifestClient,
    req: RememberRequest,
) -> Result<CallToolResult, McpError> {
    let memory = client
        .create_memory(
            req.project_id,
            &req.content,
            &req.tags,
            req.source_feature_id,
        )
        .await
        .map_err(client_err)?;

    let json = serde_json::to_string_pretty(&memory)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Search or list project memories.
pub async fn recall(
    client: &ManifestClient,
    req: RecallRequest,
) -> Result<CallToolResult, McpError> {
    let limit = req.limit.unwrap_or(10).min(50);
    let memories = client
        .search_memories(req.project_id, req.query.as_deref(), Some(limit))
        .await
        .map_err(client_err)?;

    let result = serde_json::json!({
        "memories": memories,
        "count": memories.len(),
    });
    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}

/// Delete a memory from the project's memory store.
pub async fn forget(
    client: &ManifestClient,
    req: ForgetRequest,
) -> Result<CallToolResult, McpError> {
    client
        .delete_memory(req.project_id, req.memory_id)
        .await
        .map_err(client_err)?;

    let result = serde_json::json!({ "deleted": true, "memory_id": req.memory_id });
    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![Content::text(json)]))
}
