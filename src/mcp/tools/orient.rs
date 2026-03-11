//! Orient tool — single-call session bootloader for AI agents.
//!
//! Bundles all context an agent needs to begin a session: project info,
//! feature tree, active feature, work queue, active sessions, and recent history.

use rmcp::{
    model::{CallToolResult, Content},
    ErrorData as McpError,
};
use uuid::Uuid;

use crate::mcp::{
    tree_render,
    types::{ActiveSessionInfo, OrientRequest, RecentHistoryItem, WorkQueueItem},
    ManifestClient,
};
use crate::models::{FeatureState, FeatureTreeNode};

use super::{client_err, format};

/// Orient: single-call session bootloader.
pub async fn orient(
    client: &ManifestClient,
    req: OrientRequest,
) -> Result<CallToolResult, McpError> {
    // 1. Resolve project
    let project_id = resolve_project(client, req.project_id, req.directory_path.as_deref()).await?;

    let project = client.get_project(project_id).await.map_err(client_err)?;
    let key_prefix = project.project.key_prefix.clone();

    // 2. Get project instructions summary
    let instructions_summary = client
        .get_project_instructions_summary(&project.project)
        .await;

    // 3. Get feature tree (used for rendering + extracting active sessions + work queue)
    let tree = client
        .get_feature_tree(project_id)
        .await
        .map_err(client_err)?;

    let tree_ascii = tree_render::render_tree_with_depth(&tree, req.max_depth, &key_prefix);

    // 4. Extract active sessions from tree (features with claimed_by + in_progress)
    let mut active_sessions = Vec::new();
    collect_active_sessions(&tree, &mut active_sessions);

    // 5. Extract work queue from tree (top 3 proposed leaf features)
    let mut work_queue = Vec::new();
    collect_proposed_leaves(&tree, &key_prefix, &mut work_queue);
    work_queue.sort_by_key(|w| w.priority);
    work_queue.truncate(3);

    // 6. Active feature (focused in UI)
    let active_feature = client
        .get_project_focus(project_id)
        .await
        .map_err(client_err)?;

    // 7. Recent history
    let history_items = if req.include_history {
        let entries = client
            .get_project_history(project_id, Some(5))
            .await
            .map_err(client_err)?;

        entries
            .iter()
            .map(|e| {
                let headline = e
                    .summary
                    .lines()
                    .next()
                    .unwrap_or(&e.summary)
                    .trim()
                    .to_string();
                RecentHistoryItem {
                    feature_title: e.feature_title.clone(),
                    summary_headline: headline,
                    completed_at: format::time_bucket(&e.created_at),
                }
            })
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    // 8. Build response sections
    let mut sections = Vec::new();

    // Project context
    let mut project_section = format!("# {}\n", project.project.name);
    if let Some(ref desc) = project.project.description {
        project_section.push_str(desc);
        project_section.push('\n');
    }
    if let Some(ref summary) = instructions_summary {
        project_section.push('\n');
        project_section.push_str(summary);
        project_section.push('\n');
    }
    if !project.directories.is_empty() {
        project_section.push_str("\nDirectories:\n");
        for dir in &project.directories {
            let primary = if dir.is_primary { " (primary)" } else { "" };
            project_section.push_str(&std::format!("- {}{}\n", dir.path, primary));
        }
    }
    sections.push(project_section);

    // Feature tree
    sections.push(std::format!("## Feature Tree\n```\n{}```", tree_ascii));

    // Active feature
    if let Some((fid, title, state)) = &active_feature {
        sections.push(std::format!(
            "## Active Feature\n{} — {} ({})",
            format::display_id(None, &key_prefix, fid),
            title,
            state
        ));
    }

    // Active sessions
    if !active_sessions.is_empty() {
        let mut s = "## Active Sessions\n".to_string();
        for session in &active_sessions {
            s.push_str(&std::format!(
                "- {} — {} ({})\n",
                session.feature_title,
                session.agent_type,
                session.claimed_at
            ));
        }
        sections.push(s);
    }

    // Work queue
    if !work_queue.is_empty() {
        let mut s = "## Work Queue (next 3 proposed)\n".to_string();
        for item in &work_queue {
            let short_id = item.id.to_string();
            let id = item.display_id.as_deref().unwrap_or(&short_id[..8]);
            s.push_str(&std::format!("- {} {}\n", id, item.title));
        }
        sections.push(s);
    }

    // Recent history
    if !history_items.is_empty() {
        let mut s = "## Recent Completions\n".to_string();
        for item in &history_items {
            s.push_str(&std::format!(
                "- {} — {} ({})\n",
                item.feature_title,
                item.summary_headline,
                item.completed_at
            ));
        }
        sections.push(s);
    }

    let output = sections.join("\n");
    Ok(CallToolResult::success(vec![Content::text(output)]))
}

/// Resolve project ID from explicit ID or directory path.
async fn resolve_project(
    client: &ManifestClient,
    project_id: Option<Uuid>,
    directory_path: Option<&str>,
) -> Result<Uuid, McpError> {
    if let Some(pid) = project_id {
        return Ok(pid);
    }

    if let Some(dir) = directory_path {
        let ctx = client.get_project_context(dir).await.map_err(client_err)?;
        return Ok(ctx.project.id);
    }

    Err(McpError::invalid_params(
        "Either project_id or directory_path is required",
        None,
    ))
}

/// Recursively collect active sessions (in_progress + claimed_by) from the tree.
fn collect_active_sessions(nodes: &[FeatureTreeNode], out: &mut Vec<ActiveSessionInfo>) {
    for node in nodes {
        if node.feature.state == FeatureState::InProgress {
            if let Some(ref agent) = node.feature.claimed_by {
                out.push(ActiveSessionInfo {
                    feature_title: node.feature.title.clone(),
                    agent_type: agent.clone(),
                    claimed_at: node
                        .feature
                        .claimed_at
                        .map(|t| format::time_bucket(&t))
                        .unwrap_or_default(),
                });
            }
        }
        collect_active_sessions(&node.children, out);
    }
}

/// Recursively collect proposed leaf features from the tree.
fn collect_proposed_leaves(
    nodes: &[FeatureTreeNode],
    key_prefix: &str,
    out: &mut Vec<WorkQueueItem>,
) {
    for node in nodes {
        if node.feature.state == FeatureState::Proposed && node.children.is_empty() {
            let fid: Uuid = node.feature.id.into();
            out.push(WorkQueueItem {
                id: fid,
                display_id: node
                    .feature
                    .feature_number
                    .map(|n| std::format!("{}-{}", key_prefix, n))
                    .filter(|_| !key_prefix.is_empty()),
                title: node.feature.title.clone(),
                priority: node.feature.priority,
            });
        }
        collect_proposed_leaves(&node.children, key_prefix, out);
    }
}
