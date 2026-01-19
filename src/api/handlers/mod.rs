use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path as StdPath;
use uuid::Uuid;

use crate::db::{Database, ManifestError};
use crate::models::{
    AddDirectoryInput, CommitRef, CreateFeatureInput, CreateHistoryInput, CreateProjectInput,
    CreateVersionInput, Feature, FeatureDiff, FeatureHistory, FeatureState, FeatureSummary,
    FeatureTreeNode, HistoryDetails, ListFeaturesQuery, Project, ProjectDirectory,
    ProjectHistoryEntry, ProjectWithDirectories, UpdateFeatureInput, UpdateProjectInput,
    UpdateVersionInput, Version,
};

// Import MCP types for bulk feature creation (re-exported from mcp module)
use crate::mcp::{
    DirectorySignal, DocumentationContent, FeatureHint, ModuleSignal, PlanFeaturesResponse,
    ProjectAnalysis, ProjectType, ProposedFeature,
};

// ============================================================
// Error Handling
// ============================================================

/// Convert a ManifestError to an HTTP response.
/// These are domain errors that should be exposed to the client.
fn manifest_error(e: ManifestError) -> (StatusCode, String) {
    let status = match &e {
        ManifestError::NotFound(_) => StatusCode::NOT_FOUND,
        ManifestError::Validation(_) => StatusCode::BAD_REQUEST,
        ManifestError::InvalidState(_) => StatusCode::CONFLICT,
    };
    tracing::warn!("Client error: {}", e);
    (status, e.to_string())
}

/// Convert an anyhow::Error to an HTTP response.
/// Checks if the error is a ManifestError (domain error) and handles it appropriately.
/// Other errors are treated as internal server errors.
fn internal_error(e: anyhow::Error) -> (StatusCode, String) {
    // Check if this is a wrapped ManifestError (domain error)
    if let Some(manifest_err) = e.downcast_ref::<ManifestError>() {
        return manifest_error(manifest_err.clone());
    }

    // True internal error - log full details but return generic message
    tracing::error!("Internal error: {:?}", e);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Internal server error".to_string(),
    )
}

// ============================================================
// Health
// ============================================================

pub async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

// ============================================================
// Projects
// ============================================================

pub async fn list_projects(
    State(db): State<Database>,
) -> Result<Json<Vec<Project>>, (StatusCode, String)> {
    db.get_all_projects().map(Json).map_err(internal_error)
}

pub async fn get_project(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProjectWithDirectories>, (StatusCode, String)> {
    db.get_project_with_directories(id)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))
}

pub async fn create_project(
    State(db): State<Database>,
    Json(input): Json<CreateProjectInput>,
) -> Result<(StatusCode, Json<Project>), (StatusCode, String)> {
    db.create_project(input)
        .map(|p| (StatusCode::CREATED, Json(p)))
        .map_err(internal_error)
}

pub async fn update_project(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateProjectInput>,
) -> Result<Json<Project>, (StatusCode, String)> {
    db.update_project(id, input)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))
}

pub async fn delete_project(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    if db.delete_project(id).map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "Project not found".to_string()))
    }
}

/// Query parameters for project history.
#[derive(Debug, Deserialize)]
pub struct ProjectHistoryQuery {
    /// Filter to entries for a specific version (useful for release notes).
    pub version_id: Option<Uuid>,
    /// Maximum number of entries to return. Defaults to 50.
    pub limit: Option<u32>,
    /// Number of entries to skip for pagination. Defaults to 0.
    pub offset: Option<u32>,
    /// Optional ISO datetime to filter entries created after this time.
    pub since: Option<String>,
}

/// Get project-wide history across all features.
///
/// Returns history entries for all features in the project, ordered by
/// creation date (newest first). Can be filtered by version_id to generate
/// release notes. Useful for PMs to see recent changes.
pub async fn get_project_history(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<ProjectHistoryQuery>,
) -> Result<Json<Vec<ProjectHistoryEntry>>, (StatusCode, String)> {
    // Parse optional since datetime
    let since = query
        .since
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));

    db.get_project_history(
        project_id,
        query.version_id,
        query.limit,
        query.offset,
        since,
    )
    .map(Json)
    .map_err(internal_error)
}

// ============================================================
// Project Directories
// ============================================================

pub async fn list_project_directories(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<ProjectDirectory>>, (StatusCode, String)> {
    db.get_project_directories(project_id)
        .map(Json)
        .map_err(internal_error)
}

pub async fn add_project_directory(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<AddDirectoryInput>,
) -> Result<(StatusCode, Json<ProjectDirectory>), (StatusCode, String)> {
    db.add_project_directory(project_id, input)
        .map(|d| (StatusCode::CREATED, Json(d)))
        .map_err(internal_error)
}

pub async fn remove_project_directory(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    if db.remove_project_directory(id).map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "Directory not found".to_string()))
    }
}

// ============================================================
// Versions
// ============================================================

pub async fn list_project_versions(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<Version>>, (StatusCode, String)> {
    db.get_versions_by_project(project_id)
        .map(Json)
        .map_err(internal_error)
}

pub async fn create_version(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateVersionInput>,
) -> Result<(StatusCode, Json<Version>), (StatusCode, String)> {
    db.create_version(project_id, input)
        .map(|v| (StatusCode::CREATED, Json(v)))
        .map_err(internal_error)
}

pub async fn get_version(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<Version>, (StatusCode, String)> {
    db.get_version(id)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Version not found".to_string()))
}

pub async fn update_version(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateVersionInput>,
) -> Result<Json<Version>, (StatusCode, String)> {
    db.update_version(id, input)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Version not found".to_string()))
}

pub async fn delete_version(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    if db.delete_version(id).map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "Version not found".to_string()))
    }
}

// ============================================================
// Features
// ============================================================

pub async fn list_features(
    State(db): State<Database>,
    Query(query): Query<ListFeaturesQuery>,
) -> Result<Json<Vec<FeatureSummary>>, (StatusCode, String)> {
    // Use SQL-based pagination for efficiency
    let features = db
        .get_all_features_paginated(query.limit, query.offset)
        .map_err(internal_error)?;

    // Always return summaries only - use get_feature for full details
    let summaries: Vec<FeatureSummary> = features.into_iter().map(Into::into).collect();
    Ok(Json(summaries))
}

pub async fn list_project_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<ListFeaturesQuery>,
) -> Result<Json<Vec<FeatureSummary>>, (StatusCode, String)> {
    // Use SQL-based pagination for efficiency
    let features = db
        .get_features_by_project_paginated(project_id, query.limit, query.offset)
        .map_err(internal_error)?;
    // Always return summaries only - use get_feature for full details
    let summaries: Vec<FeatureSummary> = features.into_iter().map(Into::into).collect();
    Ok(Json(summaries))
}

pub async fn list_root_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<Feature>>, (StatusCode, String)> {
    db.get_root_features(project_id)
        .map(Json)
        .map_err(internal_error)
}

pub async fn get_feature_tree(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<FeatureTreeNode>>, (StatusCode, String)> {
    db.get_feature_tree(project_id)
        .map(Json)
        .map_err(internal_error)
}

pub async fn list_children(
    State(db): State<Database>,
    Path(parent_id): Path<Uuid>,
) -> Result<Json<Vec<Feature>>, (StatusCode, String)> {
    db.get_children(parent_id).map(Json).map_err(internal_error)
}

pub async fn get_feature_history(
    State(db): State<Database>,
    Path(feature_id): Path<Uuid>,
) -> Result<Json<Vec<FeatureHistory>>, (StatusCode, String)> {
    db.get_feature_history(feature_id)
        .map(Json)
        .map_err(internal_error)
}

/// Input for creating a history entry directly on a feature (CLI mode).
#[derive(Debug, Deserialize)]
pub struct CreateFeatureHistoryInput {
    pub summary: String,
    #[serde(default)]
    pub commits: Vec<CommitRef>,
    /// Version this work was done for.
    /// If not specified, defaults to the feature's target_version_id.
    pub version_id: Option<Uuid>,
    /// If true, also update feature state to 'implemented'. Defaults to true.
    #[serde(default = "default_true")]
    pub mark_implemented: bool,
}

fn default_true() -> bool {
    true
}

pub async fn create_feature_history(
    State(db): State<Database>,
    Path(feature_id): Path<Uuid>,
    Json(input): Json<CreateFeatureHistoryInput>,
) -> Result<(StatusCode, Json<FeatureHistory>), (StatusCode, String)> {
    // Verify feature exists
    let feature = db
        .get_feature(feature_id)
        .map_err(internal_error)?
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))?;

    // Verify it's a leaf feature
    if !db.is_leaf(feature_id).map_err(internal_error)? {
        return Err((
            StatusCode::BAD_REQUEST,
            "Cannot create history on a non-leaf feature".to_string(),
        ));
    }

    // Create history entry directly
    // If version_id not provided, database layer defaults to feature's target_version_id
    let history = db
        .create_history_entry(CreateHistoryInput {
            feature_id,
            version_id: input.version_id,
            details: HistoryDetails {
                summary: input.summary,
                commits: input.commits,
            },
        })
        .map_err(internal_error)?;

    // Optionally update feature state to implemented
    if input.mark_implemented && feature.state != FeatureState::Implemented {
        db.update_feature(
            feature_id,
            UpdateFeatureInput {
                parent_id: None,
                title: None,
                details: None,
                desired_details: None,
                state: Some(FeatureState::Implemented),
                priority: None,
                target_version_id: None,
            },
        )
        .map_err(internal_error)?;
    }

    Ok((StatusCode::CREATED, Json(history)))
}

pub async fn get_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<Feature>, (StatusCode, String)> {
    db.get_feature(id)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))
}

pub async fn get_feature_diff(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<Json<FeatureDiff>, (StatusCode, String)> {
    db.get_feature_diff(id)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))
}

pub async fn create_feature(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<CreateFeatureInput>,
) -> Result<(StatusCode, Json<Feature>), (StatusCode, String)> {
    db.create_feature(project_id, input)
        .map(|f| (StatusCode::CREATED, Json(f)))
        .map_err(internal_error)
}

pub async fn update_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateFeatureInput>,
) -> Result<Json<Feature>, (StatusCode, String)> {
    db.update_feature(id, input)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "Feature not found".to_string()))
}

pub async fn delete_feature(
    State(db): State<Database>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, String)> {
    if db.delete_feature(id).map_err(internal_error)? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "Feature not found".to_string()))
    }
}

/// Query parameters for searching features.
#[derive(Debug, Deserialize)]
pub struct SearchFeaturesQuery {
    /// Search term to match against title and details.
    pub q: String,
    /// Optional project UUID to limit search to.
    pub project_id: Option<Uuid>,
    /// Maximum number of results to return. Defaults to 10.
    pub limit: Option<u32>,
}

/// Search features by title and details.
/// Returns summaries ranked by relevance.
pub async fn search_features(
    State(db): State<Database>,
    Query(query): Query<SearchFeaturesQuery>,
) -> Result<Json<Vec<FeatureSummary>>, (StatusCode, String)> {
    db.search_features(&query.q, query.project_id, query.limit)
        .map(Json)
        .map_err(internal_error)
}

// ============================================================
// Project by Directory (for MCP get_project_context)
// ============================================================

/// Query parameters for getting a project by directory path.
#[derive(Debug, Deserialize)]
pub struct GetProjectByDirectoryQuery {
    pub path: String,
}

/// Find a project by directory path.
///
/// Returns the project and matching directory if the path matches exactly,
/// or if the path is a subdirectory of a registered project directory.
pub async fn get_project_by_directory(
    State(db): State<Database>,
    Query(query): Query<GetProjectByDirectoryQuery>,
) -> Result<Json<ProjectWithDirectories>, (StatusCode, String)> {
    db.get_project_by_directory(&query.path)
        .map_err(internal_error)?
        .map(Json)
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("No project found for directory: {}", query.path),
        ))
}

// ============================================================
// Bulk Feature Creation (for MCP plan_features)
// ============================================================

/// Input for bulk feature creation.
#[derive(Debug, Deserialize)]
pub struct BulkCreateFeaturesInput {
    /// The proposed feature tree.
    pub features: Vec<ProposedFeature>,
    /// If true, creates the features in the database. If false, returns preview only.
    #[serde(default)]
    pub confirm: bool,
}

/// Create multiple features at once with hierarchical structure.
///
/// When confirm=false (default), returns the proposed features without creating them.
/// When confirm=true, creates all features and returns their IDs.
pub async fn bulk_create_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
    Json(input): Json<BulkCreateFeaturesInput>,
) -> Result<Json<PlanFeaturesResponse>, (StatusCode, String)> {
    // Verify project exists
    db.get_project(project_id)
        .map_err(internal_error)?
        .ok_or((StatusCode::NOT_FOUND, "Project not found".to_string()))?;

    let mut created_ids = Vec::new();

    if input.confirm {
        // Flatten the tree into a list of inputs with pre-generated UUIDs
        // This allows us to use the transactional bulk insert
        let mut feature_inputs = Vec::new();
        for feature in &input.features {
            flatten_feature_tree(None, feature, &mut feature_inputs, &mut created_ids);
        }

        // Create all features in a single transaction
        db.create_features_bulk(project_id, feature_inputs)
            .map_err(internal_error)?;
    }

    Ok(Json(PlanFeaturesResponse {
        proposed_features: input.features,
        created: input.confirm,
        created_feature_ids: created_ids,
    }))
}

/// Flatten a ProposedFeature tree into a list of CreateFeatureInput.
/// Pre-generates UUIDs so parent-child relationships can be established.
fn flatten_feature_tree(
    parent_id: Option<Uuid>,
    proposed: &ProposedFeature,
    inputs: &mut Vec<CreateFeatureInput>,
    created_ids: &mut Vec<String>,
) -> Uuid {
    let id = Uuid::new_v4();
    created_ids.push(id.to_string());

    inputs.push(CreateFeatureInput {
        id: Some(id),
        parent_id,
        title: proposed.title.clone(),
        details: proposed.details.clone(),
        state: Some(FeatureState::InProgress),
        priority: Some(proposed.priority),
        target_version_id: None,
    });

    // Recursively flatten children with this feature's ID as parent
    for child in &proposed.children {
        flatten_feature_tree(Some(id), child, inputs, created_ids);
    }

    id
}

// ============================================================
// Project Analysis (for AI feature planning)
// ============================================================

/// Query parameters for analyzing a project directory.
#[derive(Debug, Deserialize)]
pub struct AnalyzeProjectQuery {
    /// Absolute path to the directory to analyze.
    pub path: String,
    /// Include documentation content (README, CLAUDE.md). Defaults to true.
    #[serde(default = "default_true_query")]
    pub include_docs: bool,
    /// Maximum directory depth to scan. Defaults to 3.
    #[serde(default = "default_depth")]
    pub max_depth: u32,
}

fn default_true_query() -> bool {
    true
}

fn default_depth() -> u32 {
    3
}

/// Analyze a codebase directory to discover project structure.
///
/// Returns detected language, frameworks, modules, and documentation.
/// Used by AI agents before plan_features to understand what capabilities exist.
pub async fn analyze_project(
    State(_db): State<Database>,
    Query(query): Query<AnalyzeProjectQuery>,
) -> Result<Json<ProjectAnalysis>, (StatusCode, String)> {
    let root = StdPath::new(&query.path);

    // Validate directory exists
    if !root.exists() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("Directory not found: {}", query.path),
        ));
    }
    if !root.is_dir() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Path is not a directory: {}", query.path),
        ));
    }

    // Detect project type from manifest files
    let project_type = detect_project_type(root);
    let (name, description) = extract_project_metadata(root, &project_type);

    // Get git remote
    let git_remote = get_git_remote(root);

    // Walk directory tree
    let directories = scan_directories(root, query.max_depth);

    // Detect modules
    let modules = detect_modules(root, &project_type);

    // Read documentation
    let documentation = if query.include_docs {
        Some(read_documentation(root))
    } else {
        None
    };

    // Generate feature hints
    let hints = generate_feature_hints(root, &directories, &modules, &project_type);

    Ok(Json(ProjectAnalysis {
        name,
        description,
        project_type,
        git_remote,
        directories,
        modules,
        documentation,
        hints,
    }))
}

/// Detect project type from manifest files.
fn detect_project_type(root: &StdPath) -> ProjectType {
    // Check for Cargo.toml (Rust)
    if root.join("Cargo.toml").exists() {
        let mut frameworks = Vec::new();

        // Read Cargo.toml to detect frameworks
        if let Ok(content) = std::fs::read_to_string(root.join("Cargo.toml")) {
            if content.contains("axum") {
                frameworks.push("axum".to_string());
            }
            if content.contains("actix") {
                frameworks.push("actix".to_string());
            }
            if content.contains("rocket") {
                frameworks.push("rocket".to_string());
            }
            if content.contains("warp") {
                frameworks.push("warp".to_string());
            }
            if content.contains("tokio") {
                frameworks.push("tokio".to_string());
            }
        }

        return ProjectType {
            language: "rust".to_string(),
            frameworks,
            build_tool: Some("cargo".to_string()),
        };
    }

    // Check for package.json (TypeScript/JavaScript)
    if root.join("package.json").exists() {
        let mut frameworks = Vec::new();
        let mut language = "javascript".to_string();
        let mut build_tool = None;

        if let Ok(content) = std::fs::read_to_string(root.join("package.json")) {
            // Detect TypeScript
            if root.join("tsconfig.json").exists() || content.contains("typescript") {
                language = "typescript".to_string();
            }

            // Detect build tool
            if content.contains("\"pnpm\"") || root.join("pnpm-lock.yaml").exists() {
                build_tool = Some("pnpm".to_string());
            } else if root.join("yarn.lock").exists() {
                build_tool = Some("yarn".to_string());
            } else {
                build_tool = Some("npm".to_string());
            }

            // Detect frameworks
            if content.contains("svelte") {
                frameworks.push("svelte".to_string());
            }
            if content.contains("@sveltejs/kit") {
                frameworks.push("sveltekit".to_string());
            }
            if content.contains("\"react\"") {
                frameworks.push("react".to_string());
            }
            if content.contains("\"next\"") {
                frameworks.push("next".to_string());
            }
            if content.contains("\"vue\"") {
                frameworks.push("vue".to_string());
            }
            if content.contains("\"express\"") {
                frameworks.push("express".to_string());
            }
            if content.contains("\"fastify\"") {
                frameworks.push("fastify".to_string());
            }
        }

        return ProjectType {
            language,
            frameworks,
            build_tool,
        };
    }

    // Check for F#/C# projects
    let fsproj = std::fs::read_dir(root).ok().and_then(|entries| {
        entries
            .filter_map(|e| e.ok())
            .find(|e| e.path().extension().is_some_and(|ext| ext == "fsproj"))
    });
    if fsproj.is_some() || root.join("*.sln").exists() {
        return ProjectType {
            language: "fsharp".to_string(),
            frameworks: Vec::new(),
            build_tool: Some("dotnet".to_string()),
        };
    }

    // Check for Python
    if root.join("pyproject.toml").exists() || root.join("setup.py").exists() {
        let mut frameworks = Vec::new();
        let mut build_tool = None;

        if root.join("pyproject.toml").exists() {
            if let Ok(content) = std::fs::read_to_string(root.join("pyproject.toml")) {
                if content.contains("poetry") {
                    build_tool = Some("poetry".to_string());
                }
                if content.contains("fastapi") {
                    frameworks.push("fastapi".to_string());
                }
                if content.contains("django") {
                    frameworks.push("django".to_string());
                }
                if content.contains("flask") {
                    frameworks.push("flask".to_string());
                }
            }
        }
        if build_tool.is_none() {
            build_tool = Some("pip".to_string());
        }

        return ProjectType {
            language: "python".to_string(),
            frameworks,
            build_tool,
        };
    }

    // Check for Go
    if root.join("go.mod").exists() {
        return ProjectType {
            language: "go".to_string(),
            frameworks: Vec::new(),
            build_tool: Some("go".to_string()),
        };
    }

    // Unknown
    ProjectType {
        language: "unknown".to_string(),
        frameworks: Vec::new(),
        build_tool: None,
    }
}

/// Extract project name and description from manifest files.
fn extract_project_metadata(
    root: &StdPath,
    project_type: &ProjectType,
) -> (Option<String>, Option<String>) {
    match project_type.language.as_str() {
        "rust" => {
            if let Ok(content) = std::fs::read_to_string(root.join("Cargo.toml")) {
                let name = extract_toml_value(&content, "name");
                let description = extract_toml_value(&content, "description");
                return (name, description);
            }
        }
        "typescript" | "javascript" => {
            if let Ok(content) = std::fs::read_to_string(root.join("package.json")) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    let name = json.get("name").and_then(|v| v.as_str()).map(String::from);
                    let description = json
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    return (name, description);
                }
            }
        }
        "python" => {
            if let Ok(content) = std::fs::read_to_string(root.join("pyproject.toml")) {
                let name = extract_toml_value(&content, "name");
                let description = extract_toml_value(&content, "description");
                return (name, description);
            }
        }
        "go" => {
            if let Ok(content) = std::fs::read_to_string(root.join("go.mod")) {
                // First line is usually "module github.com/org/name"
                if let Some(line) = content.lines().next() {
                    if line.starts_with("module ") {
                        let module = line.trim_start_matches("module ").trim();
                        let name = module.rsplit('/').next().map(String::from);
                        return (name, None);
                    }
                }
            }
        }
        _ => {}
    }
    (None, None)
}

/// Simple TOML value extraction (handles quoted strings).
fn extract_toml_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&format!("{} =", key)) || trimmed.starts_with(&format!("{}=", key)) {
            let value = trimmed.split('=').nth(1)?.trim();
            // Remove quotes
            let unquoted = value.trim_matches('"').trim_matches('\'');
            if !unquoted.is_empty() {
                return Some(unquoted.to_string());
            }
        }
    }
    None
}

/// Get git remote URL from .git/config.
fn get_git_remote(root: &StdPath) -> Option<String> {
    let config_path = root.join(".git/config");
    if let Ok(content) = std::fs::read_to_string(config_path) {
        // Look for [remote "origin"] section and url =
        let mut in_origin = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "[remote \"origin\"]" {
                in_origin = true;
            } else if trimmed.starts_with('[') {
                in_origin = false;
            } else if in_origin && trimmed.starts_with("url = ") {
                return Some(trimmed.trim_start_matches("url = ").to_string());
            }
        }
    }
    None
}

/// Scan directories up to max_depth.
fn scan_directories(root: &StdPath, max_depth: u32) -> Vec<DirectorySignal> {
    let mut directories = Vec::new();
    scan_directories_recursive(root, root, 0, max_depth, &mut directories);
    directories
}

fn scan_directories_recursive(
    base: &StdPath,
    current: &StdPath,
    depth: u32,
    max_depth: u32,
    result: &mut Vec<DirectorySignal>,
) {
    if depth > max_depth {
        return;
    }

    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden and common excluded directories
        if name.starts_with('.')
            || name == "node_modules"
            || name == "target"
            || name == "__pycache__"
            || name == "venv"
            || name == ".venv"
            || name == "dist"
            || name == "build"
            || name == "coverage"
        {
            continue;
        }

        // Count files in this directory (non-recursive)
        let file_count = std::fs::read_dir(&path)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_file())
                    .count()
            })
            .unwrap_or(0) as u32;

        // Only include directories with files or that are significant
        let kind = classify_directory(&name);
        if file_count > 0 || kind != "unknown" {
            let relative_path = path.strip_prefix(base).unwrap_or(&path);
            result.push(DirectorySignal {
                path: relative_path.to_string_lossy().to_string(),
                kind: kind.to_string(),
                file_count,
            });
        }

        // Recurse into subdirectories
        scan_directories_recursive(base, &path, depth + 1, max_depth, result);
    }
}

/// Classify a directory by its name.
fn classify_directory(name: &str) -> &'static str {
    match name.to_lowercase().as_str() {
        "src" | "lib" | "app" | "source" | "sources" => "source",
        "tests" | "test" | "__tests__" | "spec" | "specs" => "tests",
        "docs" | "doc" | "documentation" => "docs",
        "config" | "configs" | ".config" | "settings" => "config",
        "api" | "handlers" | "routes" | "endpoints" => "source",
        "models" | "entities" | "schemas" => "source",
        "utils" | "helpers" | "common" | "shared" => "source",
        "components" | "views" | "pages" => "source",
        "services" | "core" | "domain" => "source",
        _ => "unknown",
    }
}

/// Detect modules based on language conventions.
fn detect_modules(root: &StdPath, project_type: &ProjectType) -> Vec<ModuleSignal> {
    let mut modules = Vec::new();
    let src_dirs = ["src", "lib", "app"];

    for src_dir in &src_dirs {
        let src_path = root.join(src_dir);
        if !src_path.exists() {
            continue;
        }

        detect_modules_in_dir(&src_path, root, project_type, &mut modules);
    }

    // Also check root for modules
    detect_modules_in_dir(root, root, project_type, &mut modules);

    modules
}

fn detect_modules_in_dir(
    dir: &StdPath,
    base: &StdPath,
    project_type: &ProjectType,
    modules: &mut Vec<ModuleSignal>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden
        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            // Check for module index files
            let is_module = match project_type.language.as_str() {
                "rust" => path.join("mod.rs").exists() || path.join("lib.rs").exists(),
                "typescript" | "javascript" => {
                    path.join("index.ts").exists()
                        || path.join("index.tsx").exists()
                        || path.join("index.js").exists()
                }
                "python" => path.join("__init__.py").exists(),
                "go" => {
                    // Go packages are directories with .go files
                    std::fs::read_dir(&path)
                        .map(|entries| {
                            entries
                                .filter_map(|e| e.ok())
                                .any(|e| e.path().extension().is_some_and(|ext| ext == "go"))
                        })
                        .unwrap_or(false)
                }
                _ => false,
            };

            if is_module {
                // Count files to determine if major
                let file_count = count_source_files(&path, project_type);
                let is_major = file_count > 5
                    || matches!(
                        name.to_lowercase().as_str(),
                        "api" | "core" | "db" | "handlers" | "models" | "services" | "domain"
                    );

                let relative_path = path.strip_prefix(base).unwrap_or(&path);
                modules.push(ModuleSignal {
                    name: name.clone(),
                    path: relative_path.to_string_lossy().to_string(),
                    is_major,
                });
            }
        }
    }
}

/// Count source files in a directory (recursive).
fn count_source_files(dir: &StdPath, project_type: &ProjectType) -> u32 {
    let extensions: &[&str] = match project_type.language.as_str() {
        "rust" => &["rs"],
        "typescript" => &["ts", "tsx"],
        "javascript" => &["js", "jsx"],
        "python" => &["py"],
        "go" => &["go"],
        "fsharp" => &["fs", "fsi"],
        _ => &[],
    };

    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let path = e.path();
                    if path.is_file() {
                        path.extension()
                            .is_some_and(|ext| extensions.iter().any(|e| ext == *e))
                    } else {
                        false
                    }
                })
                .count()
        })
        .unwrap_or(0) as u32
}

/// Read documentation files.
fn read_documentation(root: &StdPath) -> DocumentationContent {
    let readme = read_doc_file(root, &["README.md", "README", "readme.md", "Readme.md"]);
    let claude_md = read_doc_file(root, &["CLAUDE.md", "claude.md"]);

    DocumentationContent { readme, claude_md }
}

fn read_doc_file(root: &StdPath, names: &[&str]) -> Option<String> {
    for name in names {
        let path = root.join(name);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                // Truncate to ~500 lines
                let lines: Vec<&str> = content.lines().take(500).collect();
                let truncated = lines.join("\n");
                if lines.len() == 500 && content.lines().count() > 500 {
                    return Some(format!("{}\n\n... (truncated)", truncated));
                }
                return Some(truncated);
            }
        }
    }
    None
}

/// Generate feature hints from project structure.
fn generate_feature_hints(
    root: &StdPath,
    directories: &[DirectorySignal],
    modules: &[ModuleSignal],
    project_type: &ProjectType,
) -> Vec<FeatureHint> {
    let mut hints = Vec::new();
    let mut seen_hints: HashMap<String, bool> = HashMap::new();

    // Hint from major modules
    for module in modules.iter().filter(|m| m.is_major) {
        let title = match module.name.to_lowercase().as_str() {
            "api" | "handlers" | "routes" | "endpoints" => "HTTP API",
            "db" | "database" | "persistence" | "storage" => "Data Persistence",
            "auth" | "authentication" => "Authentication",
            "models" | "entities" | "domain" => "Domain Model",
            "mcp" => "MCP Server",
            "cli" => "CLI Interface",
            _ => &module.name,
        };

        if !seen_hints.contains_key(title) {
            seen_hints.insert(title.to_string(), true);
            hints.push(FeatureHint {
                title: title.to_string(),
                reason: format!("Major module detected: {}", module.name),
                paths: vec![module.path.clone()],
            });
        }
    }

    // Hint from source directories with significant content
    for dir in directories
        .iter()
        .filter(|d| d.kind == "source" && d.file_count > 3)
    {
        let dir_name = StdPath::new(&dir.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let title = match dir_name.to_lowercase().as_str() {
            "api" | "handlers" | "routes" => "HTTP API",
            "db" | "database" | "models" => "Data Persistence",
            "auth" => "Authentication",
            "components" | "views" => "UI Components",
            "services" => "Business Logic",
            _ => continue,
        };

        if !seen_hints.contains_key(title) {
            seen_hints.insert(title.to_string(), true);
            hints.push(FeatureHint {
                title: title.to_string(),
                reason: format!("Source directory with {} files", dir.file_count),
                paths: vec![dir.path.clone()],
            });
        }
    }

    // Hint from frameworks
    for framework in &project_type.frameworks {
        let title = match framework.as_str() {
            "axum" | "actix" | "rocket" | "warp" | "express" | "fastify" | "fastapi" | "flask"
            | "django" => "HTTP API",
            "sveltekit" | "next" => "Server-Side Rendering",
            "react" | "vue" | "svelte" => "Frontend UI",
            _ => continue,
        };

        if !seen_hints.contains_key(title) {
            seen_hints.insert(title.to_string(), true);
            hints.push(FeatureHint {
                title: title.to_string(),
                reason: format!("Framework detected: {}", framework),
                paths: Vec::new(),
            });
        }
    }

    // Check for specific files that indicate features
    if root.join("Dockerfile").exists() || root.join("docker-compose.yml").exists() {
        if !seen_hints.contains_key("Container Deployment") {
            hints.push(FeatureHint {
                title: "Container Deployment".to_string(),
                reason: "Docker configuration found".to_string(),
                paths: vec!["Dockerfile".to_string()],
            });
        }
    }

    if root.join("openapi.yaml").exists() || root.join("openapi.json").exists() {
        if !seen_hints.contains_key("API Documentation") {
            hints.push(FeatureHint {
                title: "API Documentation".to_string(),
                reason: "OpenAPI spec found".to_string(),
                paths: vec!["openapi.yaml".to_string()],
            });
        }
    }

    hints
}

// ============================================================
// SSE - Feature Change Notifications
// ============================================================

use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::Stream;
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

/// Subscribe to feature change events for a project via Server-Sent Events.
///
/// The stream emits a simple "change" event whenever any feature in the project
/// is created, updated, or deleted. Clients should refetch the feature tree
/// when they receive an event.
pub async fn subscribe_project_features(
    State(db): State<Database>,
    Path(project_id): Path<Uuid>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = db.subscribe();

    let stream = BroadcastStream::new(rx).filter_map(move |result| {
        match result {
            Ok(event) if event.project_id() == project_id => {
                // Emit a simple "change" event - client will refetch
                Some(Ok(Event::default().event("change").data("feature_changed")))
            }
            Ok(_) => None,  // Different project, ignore
            Err(_) => None, // Lagged, ignore (client will catch up on next event)
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
