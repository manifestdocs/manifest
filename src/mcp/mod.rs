//! MCP server for AI-assisted feature development.
//!
//! Supports two modes:
//! - CLI mode (default): 17 tools optimized for single-agent CLI workflows
//! - IDE mode: 12 tools for simplified IDE integration
//!
//! Set `MANIFEST_MODE=ide` to use IDE mode.

mod cli;
pub mod client;
mod tree_render;
mod types;

pub use cli::CliMcpServer;

use std::str::FromStr;

pub use client::ManifestClient;
pub use types::*;

use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt,
};
use uuid::Uuid;

use crate::models::*;
use client::ClientError;

#[derive(Clone)]
pub struct McpServer {
    client: ManifestClient,
    tool_router: ToolRouter<Self>,
}

impl McpServer {
    pub fn new(client: ManifestClient) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }

    /// Create from environment variables.
    pub fn from_env() -> Self {
        Self::new(ManifestClient::from_env())
    }

    fn parse_uuid(s: &str) -> Result<Uuid, McpError> {
        Uuid::parse_str(s)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))
    }

    /// Convert ClientError to McpError.
    fn client_err(e: ClientError) -> McpError {
        match e {
            ClientError::NotFound(msg) => McpError::invalid_params(msg, None),
            ClientError::BadRequest(msg) => McpError::invalid_params(msg, None),
            ClientError::Unauthorized => {
                McpError::internal_error("Unauthorized: check MANIFEST_API_KEY", None)
            }
            ClientError::Http(e) => McpError::internal_error(e.to_string(), None),
            ClientError::Server(msg) => McpError::internal_error(msg, None),
        }
    }
}

#[tool_router]
impl McpServer {
    // ============================================================
    // Discovery Tools - Browse features and projects
    // ============================================================

    #[tool(
        description = "List features, optionally filtered by project or state. Returns summaries only (id, title, state, priority, parent_id). Use get_feature for full details of a specific feature."
    )]
    async fn list_features(
        &self,
        params: Parameters<ListFeaturesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;

        // Parse project_id if provided
        let project_id = match req.project_id {
            Some(ref pid) => Some(Self::parse_uuid(pid)?),
            None => None,
        };

        // Get features via HTTP client (always returns summaries)
        let features = self
            .client
            .list_features(project_id, req.state.as_deref(), req.limit, req.offset)
            .await
            .map_err(Self::client_err)?;

        // Always return summaries only
        let result = FeatureListSummaryResponse {
            features: features
                .into_iter()
                .map(|f| FeatureSummaryInfo {
                    id: f.id.to_string(),
                    title: f.title,
                    state: f.state.as_str().to_string(),
                    priority: f.priority,
                    parent_id: f.parent_id.map(|id| id.to_string()),
                })
                .collect(),
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Search features by title or content. Use this to find specific features without listing all of them. Returns summaries ranked by relevance. Use get_feature for full details."
    )]
    async fn search_features(
        &self,
        params: Parameters<SearchFeaturesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;

        // Parse project_id if provided
        let project_id = match req.project_id {
            Some(ref pid) => Some(Self::parse_uuid(pid)?),
            None => None,
        };

        // Get features via HTTP client
        let features = self
            .client
            .search_features(&req.query, project_id, req.limit)
            .await
            .map_err(Self::client_err)?;

        let result = FeatureListSummaryResponse {
            features: features
                .into_iter()
                .map(|f| FeatureSummaryInfo {
                    id: f.id.to_string(),
                    title: f.title,
                    state: f.state.as_str().to_string(),
                    priority: f.priority,
                    parent_id: f.parent_id.map(|id| id.to_string()),
                })
                .collect(),
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get detailed information about a specific feature by ID. Returns the feature's title, details, and current state. Use this before creating a session to understand what needs to be built."
    )]
    async fn get_feature(
        &self,
        params: Parameters<GetFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let feature_id = Self::parse_uuid(&req.feature_id)?;

        let feature = self
            .client
            .get_feature(feature_id)
            .await
            .map_err(Self::client_err)?;

        let result = ManifestClient::feature_to_info(&feature);

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get implementation history for a feature. Returns past sessions with summaries, files changed, and commit references. Use this to understand previous work before starting a new session or to review what was done."
    )]
    async fn get_feature_history(
        &self,
        params: Parameters<GetFeatureHistoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let feature_id = Self::parse_uuid(&req.feature_id)?;

        let history = self
            .client
            .get_feature_history(feature_id)
            .await
            .map_err(Self::client_err)?;

        let result = FeatureHistoryResponse {
            feature_id: feature_id.to_string(),
            entries: history
                .into_iter()
                .map(|h| HistoryEntryInfo {
                    id: h.id.to_string(),
                    version_id: h.version_id.map(|id| id.to_string()),
                    version_name: None, // Feature history endpoint doesn't join version names
                    summary: h.details.summary,
                    commits: h
                        .details
                        .commits
                        .into_iter()
                        .map(|c| CommitInfo {
                            sha: c.sha,
                            message: c.message,
                            author: c.author,
                        })
                        .collect(),
                    created_at: h.created_at.to_rfc3339(),
                })
                .collect(),
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "List all projects. Returns project summaries including name, description, and instructions."
    )]
    async fn list_projects(
        &self,
        _params: Parameters<ListProjectsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let projects = self
            .client
            .list_projects()
            .await
            .map_err(Self::client_err)?;

        let result = ProjectListResponse {
            projects: projects
                .into_iter()
                .map(|p| ProjectInfo {
                    id: p.id.to_string(),
                    name: p.name,
                    description: p.description,
                    instructions: p.instructions,
                })
                .collect(),
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get project context for a directory path. Given a directory (e.g., your current working directory), returns the associated project with its instructions and coding guidelines. Use this to understand project conventions before starting work."
    )]
    async fn get_project_context(
        &self,
        params: Parameters<GetProjectContextRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;

        let result = self
            .client
            .get_project_context(&req.directory_path)
            .await
            .map_err(Self::client_err)?;

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Render a project's feature tree as ASCII art with status symbols. Returns a visual tree showing feature hierarchy and states (◇ proposed, ○ in_progress, ● implemented, ✗ deprecated)."
    )]
    async fn render_feature_tree(
        &self,
        params: Parameters<RenderFeatureTreeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let project_id = Self::parse_uuid(&req.project_id)?;

        let tree = self
            .client
            .get_feature_tree(project_id)
            .await
            .map_err(Self::client_err)?;

        let rendered = tree_render::render_tree(&tree);

        Ok(CallToolResult::success(vec![Content::text(rendered)]))
    }

    #[tool(
        description = "Get the currently active feature for the current project. Returns the feature ID, title, and details if a feature is selected, or null if no feature is selected. The context is per-project, stored in .manifest/active_context.json in the current working directory."
    )]
    async fn get_active_feature(
        &self,
        _params: Parameters<GetActiveFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        // Read active context from project directory
        let cwd = std::env::current_dir().map_err(|e| {
            McpError::internal_error(
                format!("Could not determine current directory: {}", e),
                None,
            )
        })?;

        let context_path = cwd.join(".manifest").join("active_context.json");

        if !context_path.exists() {
            return Ok(CallToolResult::success(vec![Content::text(
                r#"{"active_feature": null, "message": "No feature is currently selected for this project"}"#.to_string()
            )]));
        }

        let content = std::fs::read_to_string(&context_path).map_err(|e| {
            McpError::internal_error(format!("Failed to read context file: {}", e), None)
        })?;

        // Parse and re-serialize to ensure valid JSON
        let context: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| McpError::internal_error(format!("Invalid context file: {}", e), None))?;

        let response = serde_json::json!({
            "active_feature": context
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        )]))
    }

    #[tool(
        description = "Update a feature's state, title, or details. Use this to transition features through their lifecycle (proposed → in_progress → implemented → deprecated) or to update living documentation when implementation reveals new information. At least one field (state, title, or details) must be provided."
    )]
    async fn update_feature_state(
        &self,
        params: Parameters<UpdateFeatureStateRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let feature_id = Self::parse_uuid(&req.feature_id)?;

        // Validate at least one field is provided
        if req.state.is_none() && req.title.is_none() && req.details.is_none() {
            return Err(McpError::invalid_params(
                "At least one of state, title, or details must be provided",
                None,
            ));
        }

        // Parse state if provided
        let new_state = req
            .state
            .map(|s| {
                FeatureState::from_str(&s).map_err(|_| {
                    McpError::invalid_params(
                        format!(
                            "Invalid state '{}'. Must be: proposed, in_progress, implemented, or deprecated",
                            s
                        ),
                        None,
                    )
                })
            })
            .transpose()?;

        let feature = self
            .client
            .update_feature(
                feature_id,
                &UpdateFeatureInput {
                    parent_id: None,
                    title: req.title,
                    details: req.details,
                    desired_details: None,
                    state: new_state,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .map_err(Self::client_err)?;

        let result = ManifestClient::feature_to_info(&feature);

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ============================================================
    // Setup Tools - Create projects, directories, and features
    // ============================================================

    #[tool(
        description = "Initialize a project from a directory. Either creates a new project (analyzing the codebase to derive name and structure) or links to an existing project by name/ID. Returns project info and analysis results for use with plan_features."
    )]
    async fn init_project(
        &self,
        params: Parameters<InitProjectRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;

        // Always analyze the directory first
        let analysis = self
            .client
            .analyze_project(&req.directory_path, req.include_docs, 3)
            .await
            .map_err(Self::client_err)?;

        let project = if let Some(ref project_ref) = req.project {
            // Try to find existing project by ID or name
            let projects = self
                .client
                .list_projects()
                .await
                .map_err(Self::client_err)?;

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

            self.client
                .create_project(&CreateProjectInput {
                    name,
                    description: analysis.description.clone(),
                    instructions: None,
                })
                .await
                .map_err(Self::client_err)?
        };

        // Add directory to project
        let directory = self
            .client
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
            .map_err(Self::client_err)?;

        // Build response with project info and analysis
        let result = serde_json::json!({
            "project": {
                "id": project.id.to_string(),
                "name": project.name,
                "description": project.description,
                "instructions": project.instructions,
            },
            "directory": {
                "id": directory.id.to_string(),
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

    #[tool(
        description = "Associate an additional directory with an existing project. Use this for monorepos where multiple directories belong to the same project. The first directory should be added via init_project."
    )]
    async fn add_project_directory(
        &self,
        params: Parameters<AddProjectDirectoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let project_id = Self::parse_uuid(&req.project_id)?;

        let directory = self
            .client
            .add_project_directory(
                project_id,
                &AddDirectoryInput {
                    path: req.path,
                    git_remote: req.git_remote,
                    is_primary: req.is_primary,
                    instructions: req.instructions,
                },
            )
            .await
            .map_err(Self::client_err)?;

        let result = DirectoryInfo {
            id: directory.id.to_string(),
            path: directory.path,
            git_remote: directory.git_remote,
            is_primary: directory.is_primary,
            instructions: directory.instructions,
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Create a feature (system capability) within a project. Name by capability, not by phase or task - e.g., 'Router' not 'Phase 1: Implement Routing'. Use parent_id for domain grouping (e.g., 'Authentication' parent with 'OAuth' and 'Password Login' children). Only leaf features can have implementation sessions. Use priority field for sequencing."
    )]
    async fn create_feature(
        &self,
        params: Parameters<CreateFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let project_id = Self::parse_uuid(&req.project_id)?;
        let parent_id = match req.parent_id {
            Some(pid) => Some(Self::parse_uuid(&pid)?),
            None => None,
        };
        let state = FeatureState::from_str(&req.state).map_err(|_| {
            McpError::invalid_params(
                format!(
                    "Invalid state '{}'. Must be: proposed, in_progress, implemented, or deprecated",
                    req.state
                ),
                None,
            )
        })?;

        let feature = self
            .client
            .create_feature(
                project_id,
                &CreateFeatureInput {
                    id: None,
                    parent_id,
                    title: req.title,
                    details: req.details,
                    state: Some(state),
                    priority: req.priority,
                    target_version_id: None,
                },
            )
            .await
            .map_err(Self::client_err)?;

        let result = ManifestClient::feature_to_info(&feature);

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Plan and optionally create a feature tree for a project. Pass your proposed features after applying the user story test: 'As a [user], I can [feature]...'. With confirm=false (default), returns the proposal for user review. With confirm=true, creates all features in the database. Use this for initial project setup or adding multiple related features."
    )]
    async fn plan_features(
        &self,
        params: Parameters<PlanFeaturesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let project_id = Self::parse_uuid(&req.project_id)?;

        // Use HTTP client to bulk create features
        let response = self
            .client
            .bulk_create_features(project_id, &req.features, req.confirm)
            .await
            .map_err(Self::client_err)?;

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: rmcp::model::Implementation {
                name: "manifest".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                title: None,
                icons: None,
                website_url: None,
            },
            capabilities: rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .build(),
            instructions: Some(
                r#"Manifest manages feature implementation sessions and tasks.

FEATURE PHILOSOPHY:
Features are LIVING DOCUMENTATION of system capabilities - not work items to close.
- Unlike JIRA issues, features persist and evolve with the codebase
- A feature describes what the system DOES, not what you're DOING
- Features should make sense to someone reading them years later

USER-CENTERED FEATURES:
Features describe what USERS can do with the system. "User" means whoever consumes
the capability - end users, developers using a library, CLI users, API consumers, etc.

The User Story Test - before creating a feature, complete this sentence:
  "As a [user of this system], I can [feature name]..."

If it reads naturally, it's likely a good feature:
  - "As a developer, I can match dynamic URL paths" → Router
  - "As a CLI user, I can output results as JSON" → JSON Output
  - "As an API consumer, I can authenticate with OAuth" → OAuth Integration

If it doesn't make sense, reconsider:
  - "As a user, I can Project Scaffolding" → setup work, not a capability
  - "As a user, I can Persistence" → quality attribute, not an action

Think: "What can users DO with this system?" Each distinct action = potential feature.

FEATURE NAMING:
- Name by user capability: "Add Todo", "Filter by Status", "Export Report"
- Use nouns or short verb phrases: "Router", "Request Validation", "JSON Output"
- Parent features group related capabilities: "Authentication" contains "Password Login", "OAuth"
- Use priority field for sequencing, not the title

FEATURE HIERARCHY:
- Group features by user goal or domain area
- Parent = capability area (e.g., "Authentication")
- Children = specific capabilities (e.g., "Password Login", "OAuth", "Session Management")
- Only LEAF features can have sessions - parents are organizational
- Flat is fine for small projects; use hierarchy when it aids navigation
- Standalone capabilities can be root-level - not everything needs a parent

QUALITIES AS FEATURES:
Qualities that manifest as user-visible behaviors can be features:
  - "Audit Logging" - users can see who did what
  - "API Documentation" - users can read generated docs
  - "Error Messages" - users can understand what went wrong

Qualities that are implementation attributes belong in feature details, not as features:
  - Performance targets → "Router must match in <100ns" (in Router details)
  - Security requirements → "Must prevent SQL injection" (in Validation details)

Test: "Can a user observe or interact with this?" If yes, it can be a feature.

FEATURE FIELDS:
- title: Short capability name (2-5 words). What users can DO.
- details: Feature specification including user stories, technical notes, constraints, acceptance criteria.
          User stories can follow "As a [user], I can [capability] so that [benefit]" format.
- state: Auto-managed lifecycle (proposed → in_progress → implemented → deprecated)
- priority: Lower number = implement first. Use for sequencing.

FEATURE STATES (auto-managed):
- 'proposed': Initial idea, in backlog
- 'in_progress': Actively being worked on (auto-set when session is created)
- 'implemented': Session completed (auto-set by complete_session with mark_implemented=true)
- 'deprecated': Manually set only via update_feature_state

State transitions happen automatically:
- create_session on a 'proposed' feature → transitions to 'in_progress'
- complete_session with mark_implemented=true → transitions to 'implemented'

FEATURE vs TASK:
- Feature = WHAT users can do (persists as documentation)
- Task = HOW you're implementing it (deleted after session)
- Test: "Will this make sense as a capability description in 2 years?"

EXAMPLE:
  Authentication/
  ├── Password Login
  ├── OAuth Integration
  └── Session Management

SETUP (one-time when starting a new project):
1. Call create_project with name, description, and coding instructions
2. Call add_project_directory to associate your codebase directory with the project
3. Call create_feature to define features (remember: capabilities, not tasks!)

DISCOVERY (find what to work on):
- get_project_context: Given your CWD, find the project and its instructions
- list_features: Browse features, filter by project_id or state
- get_feature: Get full details of a feature before starting work

AGENT WORKFLOW (when assigned a task_id):
1. Call get_task_context with your task_id to understand your assignment
2. Call start_task to signal you're beginning work
3. Implement the task scope - write code, run tests, verify
4. Call complete_task when done and verified

INSTRUCTION PRIORITY:
Task scope > Project instructions > These defaults
When project or task instructions conflict with guidelines below, follow them instead.

CODING GUIDELINES (sensible defaults for all task work):

Simplicity & Clarity:
- Implement only what's asked - no extra features or future-proofing
- Start with the happy path; handle edge cases later (unless security)
- Write explicit, straightforward code; avoid clever one-liners
- Skip retry logic and other complexity unless explicitly needed

Code Structure:
- Keep conditionals/loops under 3 layers of nesting
- Functions should be 25-30 lines max; break up longer ones
- Favor pure functions; minimize side effects
- Prefer concrete over abstract; avoid premature abstraction
- Each function does one thing well; prefer composition

Best Practices:
- Validate inputs, especially user data
- Consider security implications in every change
- NEVER commit secrets, API keys, or credentials
- Use guard clauses (early return) to reduce complexity
- Choose built-in features when sufficient; add packages only when they add real value

Testing:
- Write tests first when requirements are clear (TDD)
- Structure tests to describe WHAT the code should do, not HOW
- Unit tests for domain logic, integration tests for API contracts

Process:
- Read and understand existing patterns before writing new code
- Plan complex tasks before implementing
- Ask questions when requirements are ambiguous
- Make incremental commits; small, verified changes over large batches

ORCHESTRATOR WORKFLOW (when managing a feature):
1. Call list_features with state='in_progress' to find work
2. Call get_feature to read the full specification
3. Call create_session on a leaf feature to start work
4. Call create_task to break down work into agent-sized units
5. Spawn agents with their task_ids
6. Call list_session_tasks to monitor progress
7. Call complete_session when all tasks are done

IMPORTANT:
- Read feature details carefully before coding
- Only call complete_task when work is verified (tests pass, code compiles)
- Tasks should be small enough for one agent (1-3 story points)"#
                    .into(),
            ),
            ..Default::default()
        }
    }
}

/// Check if IDE mode is enabled via MANIFEST_MODE environment variable.
/// Defaults to CLI mode if not set or set to anything other than "ide".
pub fn is_ide_mode() -> bool {
    std::env::var("MANIFEST_MODE")
        .map(|v| v.to_lowercase() == "ide")
        .unwrap_or(false)
}

pub async fn run_stdio_server() -> anyhow::Result<()> {
    use tokio::io::{stdin, stdout};

    if is_ide_mode() {
        tracing::info!("Starting MCP server via stdio (IDE mode)");
        let service = McpServer::from_env();
        let server = service.serve((stdin(), stdout())).await?;
        let quit_reason = server.waiting().await?;
        tracing::info!("MCP server stopped: {:?}", quit_reason);
    } else {
        tracing::info!("Starting MCP server via stdio (CLI mode)");
        let service = CliMcpServer::from_env();
        let server = service.serve((stdin(), stdout())).await?;
        let quit_reason = server.waiting().await?;
        tracing::info!("MCP server stopped: {:?}", quit_reason);
    }

    Ok(())
}

/// Create Axum router for Streamable HTTP MCP transport.
///
/// This provides SSE-based MCP transport at `/mcp` endpoint, allowing
/// AI agents to access Manifest tools via HTTP instead of stdio.
///
/// Uses CLI mode by default. Set MANIFEST_MODE=ide for IDE mode.
pub fn streamable_http_router() -> axum::Router {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, tower::StreamableHttpService,
    };
    use std::sync::Arc;

    if is_ide_mode() {
        let service = StreamableHttpService::new(
            || Ok(McpServer::from_env()),
            Arc::new(LocalSessionManager::default()),
            Default::default(),
        );
        axum::Router::new().fallback_service(service)
    } else {
        let service = StreamableHttpService::new(
            || Ok(CliMcpServer::from_env()),
            Arc::new(LocalSessionManager::default()),
            Default::default(),
        );
        axum::Router::new().fallback_service(service)
    }
}
