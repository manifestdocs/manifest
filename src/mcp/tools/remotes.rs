//! MCP tools for remote backend management.
//!
//! Provides agent-facing tools for triggering sync, adding remotes,
//! and querying remote sync health.

use rmcp::{model::CallToolResult, ErrorData as McpError};

use super::client_err;
use crate::mcp::client::ManifestClient;
use crate::mcp::types::{RemoteAddRequest, RemoteStatusRequest, RemoteSyncRequest};

/// Trigger a sync for Turso remotes.
///
/// Calls the local server to list remotes and sync connected projects.
/// This is a lightweight trigger — the actual sync happens through the
/// embedded replica's background sync loop.
pub async fn remote_sync(
    client: &ManifestClient,
    _req: RemoteSyncRequest,
) -> Result<CallToolResult, McpError> {
    let remotes = client.list_remotes().await.map_err(client_err)?;

    if remotes.is_empty() {
        return Ok(CallToolResult::success(vec![rmcp::model::Content::text(
            "No remotes configured. Add one with: manifest remote add <name> --url <url> --token <token>",
        )]));
    }

    let mut lines = Vec::new();
    let active_count = remotes.iter().filter(|r| r.sync_enabled).count();

    for remote in &remotes {
        let status_str = if remote.sync_enabled {
            "active"
        } else {
            "paused"
        };
        lines.push(format!(
            "  {} ({}, {})",
            remote.name, remote.provider, status_str
        ));
    }

    let summary = format!(
        "Sync triggered for {} remote{}.\n{}",
        active_count,
        if active_count == 1 { "" } else { "s" },
        lines.join("\n")
    );

    Ok(CallToolResult::success(vec![rmcp::model::Content::text(
        summary,
    )]))
}

/// Add a remote backend from agent context.
pub async fn remote_add(
    client: &ManifestClient,
    req: RemoteAddRequest,
) -> Result<CallToolResult, McpError> {
    let remote = client
        .create_remote(&req.name, &req.url, &req.token, req.provider.as_deref())
        .await
        .map_err(client_err)?;

    let msg = format!(
        "Remote '{}' added (provider: {}).\n  URL: {}",
        remote.name, remote.provider, remote.url
    );

    Ok(CallToolResult::success(vec![rmcp::model::Content::text(
        msg,
    )]))
}

/// Show sync health for a named remote.
pub async fn remote_status(
    client: &ManifestClient,
    req: RemoteStatusRequest,
) -> Result<CallToolResult, McpError> {
    let status = client
        .get_remote_status(&req.name)
        .await
        .map_err(client_err)?;

    let remote = &status.remote;
    let mut lines = vec![
        format!("Remote: {}", remote.name),
        format!("  Provider:     {}", remote.provider),
        format!("  URL:          {}", remote.url),
        format!(
            "  Sync enabled: {}",
            if remote.sync_enabled { "yes" } else { "no" }
        ),
    ];

    if status.projects.is_empty() {
        lines.push("  Projects:     none linked".to_string());
    } else {
        lines.push(format!("  Projects:     {}", status.projects.len()));
        for p in &status.projects {
            let synced = p.last_synced_at.as_deref().unwrap_or("never");
            lines.push(format!(
                "    {} (state: {}, last sync: {})",
                p.project_id, p.sync_state, synced
            ));
        }
    }

    Ok(CallToolResult::success(vec![rmcp::model::Content::text(
        lines.join("\n"),
    )]))
}
