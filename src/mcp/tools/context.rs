//! Active feature detection via web UI focus tracking.
//!
//! Resolves the feature currently selected in the Manifest web app so agents
//! can act on "this feature" without the user specifying an ID. Includes
//! staleness warnings for features claimed longer than 24 hours.

use rmcp::{
    model::{CallToolResult, Content, Role},
    ErrorData as McpError,
};

use crate::mcp::{types::GetActiveFeatureRequest, ActiveFeatureResponse, ManifestClient};
use crate::models::FeatureState;

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
                feature_state: Some(state.clone()),
            };
            let json = serde_json::to_string_pretty(&response)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?;

            let mut content = vec![Content::text(summary).with_audience(vec![Role::User])];

            // Check for stale in_progress features
            if state == FeatureState::InProgress.as_str() {
                if let Ok(ctx) = client.get_feature_with_context(feature_id).await {
                    let is_leaf = ctx.children.is_empty();
                    if let Some(warning) = super::features::stale_warning(&ctx.feature, is_leaf) {
                        content.push(Content::text(warning));
                    }
                }
            }

            content.push(Content::text(json).with_audience(vec![Role::Assistant]));
            Ok(CallToolResult::success(content))
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
