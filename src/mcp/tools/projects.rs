//! Project management tools for AI agents.
//!
//! Provides project discovery, initialization, directory association, history
//! retrieval, and project instructions. When filtered by `directory_path`,
//! returns context for the agent's current working directory.

use std::collections::HashSet;
use std::str::FromStr;

use rmcp::{
    model::{CallToolResult, Content},
    ErrorData as McpError,
};
use uuid::Uuid;

use crate::mcp::{
    types::{
        AddProjectDirectoryRequest, DirectoryInfo, GetProjectHistoryRequest,
        GetProjectInstructionsRequest, InitProjectRequest, ListProjectsRequest, ProjectInfo,
        ProjectListResponse,
    },
    ManifestClient,
};
use crate::models::{AddDirectoryInput, CreateProjectInput, FeatureId, FeatureTreeNode, ProjectId};

use super::client_err;
use super::format;

/// List projects, optionally filtered by directory path.
pub async fn list_projects(
    client: &ManifestClient,
    req: ListProjectsRequest,
) -> Result<CallToolResult, McpError> {
    // If directory_path is provided, filter to that project
    if let Some(ref dir_path) = req.directory_path {
        match client.get_project_context(dir_path).await {
            Ok(ctx) => {
                let result = ProjectListResponse {
                    projects: vec![ProjectInfo {
                        id: ctx.project.id,
                        name: ctx.project.name,
                        description: ctx.project.description,
                        instructions: ctx.project.instructions,
                    }],
                    hint: None,
                };
                let json = serde_json::to_string_pretty(&result)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                return Ok(CallToolResult::success(vec![Content::text(json)]));
            }
            Err(_) => {
                // Directory not linked to any project
                let result = ProjectListResponse {
                    projects: vec![],
                    hint: Some(format!(
                        "No project found for '{}'. Use init_project to link this directory to a project.",
                        dir_path
                    )),
                };
                let json = serde_json::to_string_pretty(&result)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                return Ok(CallToolResult::success(vec![Content::text(json)]));
            }
        }
    }

    // No filter - return all projects
    let projects = client.list_projects().await.map_err(client_err)?;

    if projects.len() <= 1 {
        // Single project or empty: return as JSON (same as directory-filtered path)
        let mut project_infos = Vec::with_capacity(projects.len());
        for p in projects {
            let instructions = client.get_project_instructions_summary(&p).await;
            project_infos.push(ProjectInfo {
                id: p.id.into(),
                name: p.name,
                description: p.description,
                instructions,
            });
        }
        let result = ProjectListResponse {
            projects: project_infos,
            hint: None,
        };
        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        return Ok(CallToolResult::success(vec![Content::text(json)]));
    }

    // Multiple projects: render as markdown table
    let headers = &["ID", "Name", "Description"];
    let rows: Vec<Vec<String>> = projects
        .iter()
        .map(|p| {
            let id: uuid::Uuid = p.id.into();
            vec![
                id.to_string(),
                p.name.clone(),
                p.description.clone().unwrap_or_default(),
            ]
        })
        .collect();

    let output = format::markdown_table(headers, &rows);
    Ok(CallToolResult::success(vec![Content::text(output)]))
}

/// Initialize a new project or link a directory to an existing one.
pub async fn init_project(
    client: &ManifestClient,
    req: InitProjectRequest,
) -> Result<CallToolResult, McpError> {
    // Always analyze the directory first
    let analysis = client
        .analyze_project(&req.directory_path, req.include_docs, 3)
        .await
        .map_err(client_err)?;

    let project = if let Some(ref project_ref) = req.project {
        // Try to find existing project by ID or name
        let projects = client.list_projects().await.map_err(client_err)?;

        // Try as UUID first
        let found = if let Ok(uuid) = Uuid::from_str(project_ref) {
            projects
                .iter()
                .find(|p| p.id == ProjectId::from(uuid))
                .cloned()
        } else {
            // Try exact name match
            projects.iter().find(|p| p.name == *project_ref).cloned()
        };

        match found {
            Some(p) => p,
            None => {
                // Return helpful error with existing project names
                let existing: Vec<String> = projects.iter().map(|p| p.name.clone()).collect();
                let msg = if existing.is_empty() {
                    format!("No project '{}' found. No projects exist yet.", project_ref)
                } else {
                    format!(
                        "No project '{}' found. Existing projects: {}",
                        project_ref,
                        existing.join(", ")
                    )
                };
                return Err(McpError::invalid_params(msg, None));
            }
        }
    } else {
        // Check if directory is already linked to a project
        if let Ok(pwd) = client.get_project_by_directory(&req.directory_path).await {
            pwd.project
        } else {
            // Create new project from analysis
            let name = analysis.name.clone().unwrap_or_else(|| {
                std::path::Path::new(&req.directory_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Untitled")
                    .to_string()
            });

            client
                .create_project(&CreateProjectInput {
                    name,
                    slug: None,
                    description: analysis.description.clone(),
                    instructions: None,
                    key_prefix: None,
                })
                .await
                .map_err(client_err)?
        }
    };

    // Add directory to project
    let directory = client
        .add_project_directory(
            project.id.into(),
            &AddDirectoryInput {
                path: req.directory_path.clone(),
                git_remote: analysis.git_remote.clone(),
                is_primary: true,
                instructions: None,
            },
        )
        .await
        .map_err(client_err)?;

    // Get instructions summary from root feature (source of truth) or fallback
    let instructions = client.get_project_instructions_summary(&project).await;

    // Build response with project info and analysis
    let summary = format!("Initialized project '{}'", project.name);
    let result = serde_json::json!({
        "project": {
            "id": project.id,
            "name": project.name,
            "description": project.description,
            "instructions": instructions,
        },
        "directory": {
            "id": directory.id,
            "path": directory.path,
            "git_remote": directory.git_remote,
            "is_primary": directory.is_primary,
        },
        "analysis": analysis,
    });

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![
        Content::text(summary),
        Content::text(json),
    ]))
}

/// Associate an additional directory with an existing project.
pub async fn add_project_directory(
    client: &ManifestClient,
    req: AddProjectDirectoryRequest,
) -> Result<CallToolResult, McpError> {
    let directory = client
        .add_project_directory(
            req.project_id,
            &AddDirectoryInput {
                path: req.path,
                git_remote: req.git_remote,
                is_primary: req.is_primary,
                instructions: req.instructions,
            },
        )
        .await
        .map_err(client_err)?;

    let summary = format!("Added directory '{}'", directory.path);
    let result = DirectoryInfo {
        id: directory.id.into(),
        path: directory.path,
        git_remote: directory.git_remote,
        is_primary: directory.is_primary,
        instructions: directory.instructions,
    };

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![
        Content::text(summary),
        Content::text(json),
    ]))
}

/// Get full project instructions (root feature details).
pub async fn get_project_instructions(
    client: &ManifestClient,
    req: GetProjectInstructionsRequest,
) -> Result<CallToolResult, McpError> {
    let project = client
        .get_project(req.project_id)
        .await
        .map_err(client_err)?;

    let instructions = client.get_project_instructions_full(&project.project).await;

    match instructions {
        Some(text) => Ok(CallToolResult::success(vec![Content::text(text)])),
        None => Ok(CallToolResult::success(vec![Content::text(
            "No project instructions found. Use update_feature on the root feature to add instructions.",
        )])),
    }
}

/// Get recent activity across a project or for a specific feature.
pub async fn get_project_history(
    client: &ManifestClient,
    req: GetProjectHistoryRequest,
) -> Result<CallToolResult, McpError> {
    let limit = req.limit.unwrap_or(20);

    let mut entries = client
        .get_project_history(req.project_id, Some(limit))
        .await
        .map_err(client_err)?;

    // If feature_id provided, filter to that feature + descendants
    if let Some(ref feature_id_str) = req.feature_id {
        let feature_id = client
            .resolve_feature_id(feature_id_str, Some(req.project_id))
            .await
            .map_err(client_err)?;

        let tree = client
            .get_feature_tree(req.project_id)
            .await
            .map_err(client_err)?;

        let mut ids = HashSet::new();
        ids.insert(FeatureId::from(feature_id));
        collect_descendant_ids(&tree, &FeatureId::from(feature_id), &mut ids);

        entries.retain(|e| ids.contains(&e.feature_id));
    }

    let timeline = format::render_activity_timeline(&entries);
    Ok(CallToolResult::success(vec![Content::text(timeline)]))
}

/// Recursively collect all descendant feature IDs from a tree.
fn collect_descendant_ids(
    nodes: &[FeatureTreeNode],
    parent_id: &FeatureId,
    ids: &mut HashSet<FeatureId>,
) {
    for node in nodes {
        if node.feature.id == *parent_id {
            // Found the target node — collect all children recursively
            collect_all_ids(&node.children, ids);
            return;
        }
        // Search deeper
        collect_descendant_ids(&node.children, parent_id, ids);
    }
}

/// Collect all feature IDs from a subtree.
fn collect_all_ids(nodes: &[FeatureTreeNode], ids: &mut HashSet<FeatureId>) {
    for node in nodes {
        ids.insert(node.feature.id);
        collect_all_ids(&node.children, ids);
    }
}
