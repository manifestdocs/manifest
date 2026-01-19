//! CLI mode MCP server - simplified tools for single-agent CLI workflows.
//!
//! This mode exposes 16 tools optimized for CLI agents like Claude Code:
//! - Discovery: get_project_context, list_features, search_features, get_feature, get_feature_history, render_feature_tree
//! - Setup: create_project, add_project_directory, create_feature, plan_features
//! - Work: start_feature, complete_feature
//! - Versions: list_versions, create_version, set_feature_version, release_version

use std::str::FromStr;

use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerInfo},
    schemars::JsonSchema,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use serde::Deserialize;
use uuid::Uuid;

use super::tree_render;
use super::types::*;
use super::ManifestClient;
use crate::models::*;

// ============================================================
// CLI-specific Request Types
// ============================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartFeatureRequest {
    #[schemars(description = "The UUID of the feature to start working on")]
    pub feature_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteFeatureRequest {
    #[schemars(description = "The UUID of the feature to complete")]
    pub feature_id: String,
    #[schemars(
        description = "Summary of work done (git-style format). First line is a concise headline shown in list views. Add details after a blank line if needed (bullet points, technical notes). Example:\n\nImplemented OAuth login flow\n\n- Added Google OAuth provider\n- Created session management\n- Updated user model with provider field"
    )]
    pub summary: String,
    #[schemars(description = "Git commits created during this work")]
    #[serde(default)]
    pub commits: Vec<CommitRefInput>,
    #[schemars(description = "Whether to mark the feature as 'implemented'. Defaults to true.")]
    #[serde(default = "default_true")]
    pub mark_implemented: bool,
}

fn default_true() -> bool {
    true
}

// ============================================================
// CLI MCP Server
// ============================================================

#[derive(Clone)]
pub struct CliMcpServer {
    client: ManifestClient,
    tool_router: ToolRouter<Self>,
}

impl CliMcpServer {
    pub fn new(client: ManifestClient) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(ManifestClient::from_env())
    }

    fn parse_uuid(s: &str) -> Result<Uuid, McpError> {
        Uuid::parse_str(s)
            .map_err(|e| McpError::invalid_params(format!("Invalid UUID: {}", e), None))
    }

    fn client_err(e: super::client::ClientError) -> McpError {
        match e {
            super::client::ClientError::NotFound(msg) => McpError::invalid_params(msg, None),
            super::client::ClientError::BadRequest(msg) => McpError::invalid_params(msg, None),
            super::client::ClientError::Unauthorized => {
                McpError::internal_error("Unauthorized: check MANIFEST_API_KEY", None)
            }
            super::client::ClientError::Http(e) => McpError::internal_error(e.to_string(), None),
            super::client::ClientError::Server(msg) => McpError::internal_error(msg, None),
        }
    }
}

#[tool_router]
impl CliMcpServer {
    // ============================================================
    // Discovery Tools
    // ============================================================

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
        description = "List features, optionally filtered by project or state. Returns summaries only (id, title, state, priority, parent_id). Use get_feature for full details of a specific feature."
    )]
    async fn list_features(
        &self,
        params: Parameters<ListFeaturesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;

        let project_id = match req.project_id {
            Some(ref pid) => Some(Self::parse_uuid(pid)?),
            None => None,
        };

        let features = self
            .client
            .list_features(project_id, req.state.as_deref(), req.limit, req.offset)
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
        description = "Search features by title or content. Use this to find specific features without listing all of them. Returns summaries ranked by relevance. Use get_feature for full details."
    )]
    async fn search_features(
        &self,
        params: Parameters<SearchFeaturesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;

        let project_id = match req.project_id {
            Some(ref pid) => Some(Self::parse_uuid(pid)?),
            None => None,
        };

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
        description = "Get detailed information about a specific feature by ID. Returns the feature's title, details, and current state. Use this to understand what needs to be built before starting work."
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
        description = "Get implementation history for a feature. Returns past work with summaries and commit references. Use this to understand previous work before continuing on a feature."
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
        description = "Render a project's feature tree as ASCII art with status symbols. Returns a visual tree showing feature hierarchy and states (◇ proposed, ○ specified, ● implemented, ✗ deprecated)."
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

    // ============================================================
    // Setup Tools
    // ============================================================

    #[tool(
        description = "Create a new project. Projects are containers for features and can have multiple directories (e.g., monorepo subdirectories). Use this when starting work on a new codebase. After creating, use add_project_directory to associate directories."
    )]
    async fn create_project(
        &self,
        params: Parameters<CreateProjectRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;

        let project = self
            .client
            .create_project(&CreateProjectInput {
                name: req.name,
                description: req.description,
                instructions: req.instructions,
            })
            .await
            .map_err(Self::client_err)?;

        let result = ProjectInfo {
            id: project.id.to_string(),
            name: project.name,
            description: project.description,
            instructions: project.instructions,
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Associate a directory with a project. This enables get_project_context to find the project when given a directory path. Use after create_project. Mark one directory as is_primary=true for the main project location."
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
        description = "Create a feature (system capability) within a project. Name by capability, not by task - e.g., 'Router' not 'Implement Routing'. Use parent_id for domain grouping. Use priority field for sequencing."
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
                    "Invalid state '{}'. Must be: proposed, specified, implemented, or deprecated",
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
        description = "Plan and optionally create a feature tree for a project. Pass your proposed features after applying the user story test: 'As a [user], I can [feature]...'. With confirm=false (default), returns the proposal for review. With confirm=true, creates all features."
    )]
    async fn plan_features(
        &self,
        params: Parameters<PlanFeaturesRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let project_id = Self::parse_uuid(&req.project_id)?;

        let response = self
            .client
            .bulk_create_features(project_id, &req.features, req.confirm)
            .await
            .map_err(Self::client_err)?;

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ============================================================
    // Work Tools
    // ============================================================

    #[tool(
        description = "Signal that you are starting work on a feature. Sets state to 'specified' if currently 'proposed'. Returns the feature details so you know what to implement."
    )]
    async fn start_feature(
        &self,
        params: Parameters<StartFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let feature_id = Self::parse_uuid(&req.feature_id)?;

        // Get current feature
        let feature = self
            .client
            .get_feature(feature_id)
            .await
            .map_err(Self::client_err)?;

        // Transition to specified if proposed
        let feature = if feature.state == FeatureState::Proposed {
            self.client
                .update_feature(
                    feature_id,
                    &UpdateFeatureInput {
                        parent_id: None,
                        title: None,
                        details: None,
                        desired_details: None,
                        state: Some(FeatureState::Specified),
                        priority: None,
                        target_version_id: None,
                    },
                )
                .await
                .map_err(Self::client_err)?
        } else {
            feature
        };

        let result = ManifestClient::feature_to_info(&feature);

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Complete work on a feature. Creates a history entry with your summary and commits, then marks the feature as 'implemented'. Call this when work is done and verified."
    )]
    async fn complete_feature(
        &self,
        params: Parameters<CompleteFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let feature_id = Self::parse_uuid(&req.feature_id)?;

        // Convert commits
        let commits: Vec<CommitRef> = req
            .commits
            .into_iter()
            .map(|c| CommitRef {
                sha: c.sha,
                message: c.message,
                author: c.author,
            })
            .collect();

        // Create history entry directly (no session)
        let history = self
            .client
            .create_feature_history(feature_id, &req.summary, &commits, req.mark_implemented)
            .await
            .map_err(Self::client_err)?;

        // Get updated feature
        let feature = self
            .client
            .get_feature(feature_id)
            .await
            .map_err(Self::client_err)?;

        let result = serde_json::json!({
            "feature": ManifestClient::feature_to_info(&feature),
            "history_entry": {
                "id": history.id.to_string(),
                "summary": history.details.summary,
                "created_at": history.created_at.to_rfc3339()
            }
        });

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ============================================================
    // Version Tools
    // ============================================================

    #[tool(
        description = "List versions for a project with release planning context. Returns versions with feature counts, plus 'now' (first unreleased) and 'next' (second unreleased) IDs for planning."
    )]
    async fn list_versions(
        &self,
        params: Parameters<ListVersionsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let project_id = Self::parse_uuid(&req.project_id)?;

        // Get versions and features in parallel
        let versions = self
            .client
            .list_versions(project_id)
            .await
            .map_err(Self::client_err)?;

        let features = self
            .client
            .list_features(Some(project_id), None, None, None)
            .await
            .map_err(Self::client_err)?;

        // Count features per version
        let mut version_feature_counts: std::collections::HashMap<Uuid, u32> =
            std::collections::HashMap::new();
        for feature in &features {
            if let Some(vid) = feature.target_version_id {
                *version_feature_counts.entry(vid).or_insert(0) += 1;
            }
        }

        // Find unreleased versions for Now/Next
        let mut unreleased: Vec<_> = versions
            .iter()
            .filter(|v| v.released_at.is_none())
            .collect();
        unreleased.sort_by(|a, b| a.created_at.cmp(&b.created_at));

        let now = unreleased.first().map(|v| v.id.to_string());
        let next = unreleased.get(1).map(|v| v.id.to_string());

        // Build response
        let result = VersionListResponse {
            versions: versions
                .into_iter()
                .map(|v| VersionInfo {
                    id: v.id.to_string(),
                    name: v.name,
                    description: v.description,
                    released_at: v.released_at.map(|dt| dt.to_rfc3339()),
                    feature_count: version_feature_counts.get(&v.id).copied().unwrap_or(0),
                })
                .collect(),
            now,
            next,
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Create a new version for release planning. Use this to define milestones like 'v0.2', 'MVP', or '2024.1'."
    )]
    async fn create_version(
        &self,
        params: Parameters<CreateVersionRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let project_id = Self::parse_uuid(&req.project_id)?;

        let version = self
            .client
            .create_version(
                project_id,
                &CreateVersionInput {
                    name: req.name,
                    description: req.description,
                },
            )
            .await
            .map_err(Self::client_err)?;

        let result = VersionInfo {
            id: version.id.to_string(),
            name: version.name,
            description: version.description,
            released_at: version.released_at.map(|dt| dt.to_rfc3339()),
            feature_count: 0, // New version has no features yet
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Assign a feature to a target version for release planning. Use this to express 'put this feature in v0.2'. Pass null for version_id to unassign."
    )]
    async fn set_feature_version(
        &self,
        params: Parameters<SetFeatureVersionRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let feature_id = Self::parse_uuid(&req.feature_id)?;
        let version_id = match req.version_id {
            Some(ref vid) => Some(Some(Self::parse_uuid(vid)?)),
            None => Some(None), // Explicitly unassign
        };

        let feature = self
            .client
            .update_feature(
                feature_id,
                &UpdateFeatureInput {
                    parent_id: None,
                    title: None,
                    details: None,
                    desired_details: None,
                    state: None,
                    priority: None,
                    target_version_id: version_id,
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
        description = "Mark a version as released. Sets released_at to now. Use this when you ship a version."
    )]
    async fn release_version(
        &self,
        params: Parameters<ReleaseVersionRequest>,
    ) -> Result<CallToolResult, McpError> {
        let req = params.0;
        let version_id = Self::parse_uuid(&req.version_id)?;

        let version = self
            .client
            .update_version(
                version_id,
                &UpdateVersionInput {
                    name: None,
                    description: None,
                    released_at: Some(chrono::Utc::now()),
                },
            )
            .await
            .map_err(Self::client_err)?;

        let result = VersionInfo {
            id: version.id.to_string(),
            name: version.name,
            description: version.description,
            released_at: version.released_at.map(|dt| dt.to_rfc3339()),
            feature_count: 0, // Would need to fetch features to count
        };

        let json = serde_json::to_string_pretty(&result)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

#[tool_handler]
impl ServerHandler for CliMcpServer {
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
            instructions: Some(CLI_INSTRUCTIONS.into()),
            ..Default::default()
        }
    }
}

const CLI_INSTRUCTIONS: &str = r#"Manifest tracks features as living documentation of system capabilities.

PHILOSOPHY:
Features are not work items to close - they describe what the system DOES.
A feature should make sense to someone reading it years from now.

USER STORY TEST:
Before creating a feature, complete: "As a [user], I can [feature]..."
- Good: "As a developer, I can match dynamic URL paths" → Router
- Bad: "As a user, I can Persistence" → quality attribute, not capability

FEATURE STATES:
- proposed (◇): Idea in backlog
- specified (○): Work in progress
- implemented (●): Complete and documented
- deprecated (✗): No longer active

WORKFLOW:

1. DISCOVER what to work on:
   - get_project_context: Find project from your directory
   - list_features / search_features: Browse the backlog
   - render_feature_tree: Visualize the hierarchy
   - get_feature: Read full specification

2. START work:
   - start_feature: Transitions proposed → specified
   - Returns feature details for implementation

3. IMPLEMENT:
   - Write code, run tests, verify
   - The feature details are your specification

4. COMPLETE work:
   - complete_feature: Records summary + commits, marks implemented
   - Creates history entry for future reference

VERSIONS & PLANNING:
Versions represent planned releases (v0.1, v0.2, MVP, etc.):
- list_versions: See all versions with feature counts and Now/Next/Later context
- create_version: Define a new milestone for planning
- set_feature_version: Assign a feature to a version ("put this in v0.2")
- release_version: Mark a version as shipped

Planning context:
- Now = first unreleased version (current focus)
- Next = second unreleased version (queued up)
- Later = all other unreleased versions (backlog)

SETUP (one-time):
1. create_project - name and coding guidelines
2. add_project_directory - associate your codebase
3. create_feature or plan_features - define capabilities
4. create_version - define release milestones (optional)

GUIDELINES:
- Read feature details before coding
- Only call complete_feature when work is verified
- Write summaries in git-style format: first line is a concise headline, details after a blank line
- Link commits to document what changed

DISPLAY:
Tool results appear as collapsed JSON in the UI. Always summarize results inline for humans:
- render_feature_tree: Display the ASCII tree in your response
- search_features: "Found N features: Title1 (state), Title2 (state), ..."
- list_features: Same as search - summarize titles and states
- get_feature: "Feature: Title (state)" + key details from specification
- start_feature: "Started work on 'Title' - now in 'specified' state"
- complete_feature: "Completed 'Title' - marked as implemented"
- get_project_context: Summarize project name and any relevant instructions
- list_versions: "Versions: v0.1 (released), v0.2 (now, 3 features), v0.3 (next, 1 feature)"
- create_version: "Created version 'v0.2'"
- set_feature_version: "Assigned 'Feature Name' to v0.2"
- release_version: "Released v0.1""#;
