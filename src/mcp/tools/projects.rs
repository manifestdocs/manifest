use std::str::FromStr;

use rmcp::{
    model::{CallToolResult, Content},
    ErrorData as McpError,
};
use uuid::Uuid;

use crate::mcp::{
    types::{
        AddProjectDirectoryRequest, DirectoryInfo, InitProjectRequest, ListProjectsRequest,
        ProjectInfo, ProjectListResponse,
    },
    ManifestClient,
};
use crate::models::{AddDirectoryInput, CreateProjectInput};

use super::client_err;

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

    // Build project info with instructions from root feature (source of truth)
    let mut project_infos = Vec::with_capacity(projects.len());
    for p in projects {
        let instructions = client.get_project_instructions(&p).await;
        project_infos.push(ProjectInfo {
            id: p.id,
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

    Ok(CallToolResult::success(vec![Content::text(json)]))
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
            projects.iter().find(|p| p.id == uuid).cloned()
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
        // Create new project from analysis
        let name = analysis.name.clone().unwrap_or_else(|| {
            // Derive from directory name
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
            })
            .await
            .map_err(client_err)?
    };

    // Add directory to project
    let directory = client
        .add_project_directory(
            project.id,
            &AddDirectoryInput {
                path: req.directory_path.clone(),
                git_remote: analysis.git_remote.clone(),
                is_primary: true,
                instructions: None,
            },
        )
        .await
        .map_err(client_err)?;

    // Get instructions from root feature (source of truth) or fallback
    let instructions = client.get_project_instructions(&project).await;

    // Build response with project info and analysis
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

    Ok(CallToolResult::success(vec![Content::text(json)]))
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

    let result = DirectoryInfo {
        id: directory.id,
        path: directory.path,
        git_remote: directory.git_remote,
        is_primary: directory.is_primary,
        instructions: directory.instructions,
    };

    let json = serde_json::to_string_pretty(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;

    Ok(CallToolResult::success(vec![Content::text(json)]))
}
