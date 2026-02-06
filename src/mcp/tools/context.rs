use rmcp::{
    model::{CallToolResult, Content},
    ErrorData as McpError,
};

use crate::mcp::{types::GetActiveFeatureRequest, ActiveFeatureResponse, ManifestClient};

use super::client_err;

/// Get the feature the user is currently looking at in the Manifest app.
pub async fn get_active_feature(
    client: &ManifestClient,
    req: GetActiveFeatureRequest,
) -> Result<CallToolResult, McpError> {
    match client
        .get_project_focus(req.project_id)
        .await
        .map_err(client_err)?
    {
        Some((feature_id, title, state)) => {
            let summary = format!("'{}' selected ({})", title, state);
            let response = ActiveFeatureResponse {
                feature_id: Some(feature_id),
                feature_title: Some(title),
                feature_state: Some(state),
            };
            let json = serde_json::to_string_pretty(&response)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            Ok(CallToolResult::success(vec![
                Content::text(summary),
                Content::text(json),
            ]))
        }
        None => {
            let response = ActiveFeatureResponse {
                feature_id: None,
                feature_title: None,
                feature_state: None,
            };
            let json = serde_json::to_string_pretty(&response)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;
            Ok(CallToolResult::success(vec![
                Content::text("No feature selected."),
                Content::text(json),
            ]))
        }
    }
}
