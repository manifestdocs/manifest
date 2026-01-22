//! MCP server for AI-assisted feature development.
//!
//! Exposes tools optimized for CLI agents like Claude Code:
//! - Discovery: list_projects, find_features, get_feature, render_feature_tree
//! - Setup: init_project, add_project_directory, plan, create_feature
//! - Work: start_feature, complete_feature, get_next_feature
//! - Versions: list_versions, create_version, set_feature_version, release_version

use super::tools;
use super::types::*;
use super::ManifestClient;
use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};

// ============================================================
// Server Implementation
// ============================================================

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

    pub fn from_env() -> Self {
        Self::new(ManifestClient::from_env())
    }
}

#[tool_router]
impl McpServer {
    // ============================================================
    // Discovery Tools
    // ============================================================

    #[tool(
        description = "ORIENT: List projects. If directory_path is provided, finds the project containing that directory (useful for auto-discovery). Otherwise lists all projects."
    )]
    async fn list_projects(
        &self,
        params: Parameters<ListProjectsRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::list_projects(&self.client, params.0).await
    }

    #[tool(
        description = "ORIENT: Find features by project, state, or search query. Returns summaries only. Use get_feature for full details."
    )]
    async fn find_features(
        &self,
        params: Parameters<FindFeaturesRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::find_features(&self.client, params.0).await
    }

    #[tool(
        description = "ORIENT/BUILD: Get detailed feature spec. Returns title, description, acceptance criteria, and state. Set include_history=true to see implementation history. READ THIS before starting work."
    )]
    async fn get_feature(
        &self,
        params: Parameters<GetFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::get_feature(&self.client, params.0).await
    }

    #[tool(
        description = "ORIENT: Render the feature tree as ASCII art. Essential for understanding project structure, hierarchy, and current status (◇ proposed, ○ in_progress, ● implemented)."
    )]
    async fn render_feature_tree(
        &self,
        params: Parameters<RenderFeatureTreeRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::render_feature_tree(&self.client, params.0).await
    }

    // ============================================================
    // Setup Tools
    // ============================================================

    #[tool(
        description = "SETUP: Initialize a project from a directory. Analyzes codebase, creates project (or links to existing), and returns analysis. Use this before `plan`."
    )]
    async fn init_project(
        &self,
        params: Parameters<InitProjectRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::init_project(&self.client, params.0).await
    }

    #[tool(
        description = "SETUP: Associate an additional directory with an existing project. Use this for monorepos. First directory should be added via `init_project`."
    )]
    async fn add_project_directory(
        &self,
        params: Parameters<AddProjectDirectoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::add_project_directory(&self.client, params.0).await
    }

    #[tool(
        description = "DISCOVER: Generate a feature tree from an existing codebase by analyzing code structure and git history. Returns a markdown document describing system capabilities. Use this to understand what features exist in an undocumented codebase."
    )]
    async fn generate_feature_tree(
        &self,
        params: Parameters<GenerateFeatureTreeRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::generate::generate_feature_tree(params.0).await
    }

    #[tool(
        description = "SETUP: Decompose a PRD or vision into a feature tree. With confirm=false, returns a proposal. With confirm=true, creates the features."
    )]
    async fn plan(
        &self,
        params: Parameters<PlanFeaturesRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::plan(&self.client, params.0).await
    }

    #[tool(
        description = "SETUP: Create a single feature. Name by capability (e.g., 'Router') not task. Use parent_id for grouping. Use `plan` for bulk creation."
    )]
    async fn create_feature(
        &self,
        params: Parameters<CreateFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::create_feature(&self.client, params.0).await
    }

    // ============================================================
    // Work Tools
    // ============================================================

    #[tool(
        description = "CLAIM: Signal you are starting work. Transitions state to 'in_progress'. Returns full feature details—this is your spec to implement. IMPORTANT: Do not change the feature's target version during implementation."
    )]
    async fn start_feature(
        &self,
        params: Parameters<StartFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::start_feature(&self.client, params.0).await
    }

    #[tool(
        description = "DOCUMENT: Mark work as done. Records a history entry with your summary and commits, then sets state to 'implemented'. Call this only after verification."
    )]
    async fn complete_feature(
        &self,
        params: Parameters<CompleteFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::complete_feature(&self.client, params.0).await
    }

    #[tool(
        description = "ORIENT: Get the next workable feature. Returns the highest-priority 'proposed' or 'in_progress' feature. Prioritizes the 'now' version. Use this to find what to work on."
    )]
    async fn get_next_feature(
        &self,
        params: Parameters<GetNextFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::get_next_feature(&self.client, params.0).await
    }

    // ============================================================
    // Version Tools
    // ============================================================

    #[tool(
        description = "ORIENT: List versions. Returns release milestones with status indicators: 'now' (current focus), 'next' (upcoming), and 'later'."
    )]
    async fn list_versions(
        &self,
        params: Parameters<ListVersionsRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::versions::list_versions(&self.client, params.0).await
    }

    #[tool(
        description = "PLAN: Create a release milestone (e.g., 'v0.2', 'MVP'). Defines the target for a group of features."
    )]
    async fn create_version(
        &self,
        params: Parameters<CreateVersionRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::versions::create_version(&self.client, params.0).await
    }

    #[tool(
        description = "PLAN: Assign a feature to a release version. Use this to schedule features for specific milestones. Pass null to unassign."
    )]
    async fn set_feature_version(
        &self,
        params: Parameters<SetFeatureVersionRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::versions::set_feature_version(&self.client, params.0).await
    }

    #[tool(
        description = "DOCUMENT: Mark a version as shipped. Sets released_at timestamp. Use this when a milestone is complete and deployed."
    )]
    async fn release_version(
        &self,
        params: Parameters<ReleaseVersionRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::versions::release_version(&self.client, params.0).await
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
            instructions: Some(INSTRUCTIONS.into()),
            ..Default::default()
        }
    }
}

const INSTRUCTIONS: &str = r#"Manifest is living documentation for the software we are building in this project.

HOW TO USE MANIFEST:
Manifest provides context: what the system does, what has been built, what needs work, and why decisions were made. Read feature specs before implementing. Check history to see prior work. Update features when you complete work.

THE FEATURE TREE:
Every project has a feature tree—a hierarchy of capabilities the system provides. The tree structure groups related features (e.g., Auth > Login > OAuth). Each feature has a state:

◇ proposed — in the backlog, not yet started
○ in_progress — actively being worked on
● implemented — complete and documented
✗ archived — soft-deleted, kept for historical reference

DISCOVERING FEATURES:
- find_features — find features by project, state, or search term
- get_feature — get full details and history for a specific feature
- get_next_feature — get the highest priority proposed or in_progress feature
- render_feature_tree — display the full tree as ASCII art for the user

VERSIONS:
Versions use semantic versioning e.g., 0.1.0, 0.2.0, 1.0.0), and organize features into releases. The first unreleased version is "now"—the current focus. The second unreleased version is "next". Everything after that is "later". Features in "now" are highest priority.

FEATURES AS LIVING DOCUMENTATION:
Features describe system capabilities, not work items to close. A feature titled "Router" should make sense years from now. Before creating one, apply the user story test: "As a [user], I can [capability] so that [benefit]."
- Good: "As a developer, I can match dynamic URL paths so that I can build REST APIs" → Router
- Bad: "As a user, I can have data persistence" → quality attribute, not capability

WORKFLOW:

1. ORIENT — understand what exists and what's needed:
   - list_projects (filter by directory_path to find project for your CWD)
   - render_feature_tree — see the full picture
   - get_feature (include_history=true) — read the spec AND what's been done before
   - get_next_feature — find highest-priority work

2. CLAIM — signal you're starting:
   - start_feature — transitions proposed → in_progress, returns full spec
   - IMPORTANT: The feature's target version is locked during implementation. Do not call set_feature_version while working on a feature.

3. BUILD — implement against the spec:
   - The feature details ARE your specification
   - Write tests first, then implement, then verify

4. DOCUMENT — record what you did:
   - complete_feature — provide summary + commit SHAs
   - This creates a history entry so future agents (or future you) know what happened

VERSIONS & PLANNING:
- list_versions — see Now (current focus), Next (queued), Later (backlog)
- create_version — define milestones like "v0.2.0"
- set_feature_version — assign features to releases
- release_version — mark a version as shipped

When all features in the "now" version are implemented, ask the user before calling release_version. Releasing shifts "next" to become the new "now".

SETUP (when starting fresh):
1. init_project — analyze codebase, create project, link directory
2. add_project_directory — for projects that may have directories in different locations
3. plan — break down a PRD, tech spec, or vision into a feature tree
4. create_version — define release milestones

DISPLAY GUIDELINES:
Tool results are collapsed JSON. Always summarize for humans:
- render_feature_tree: Show the ASCII tree directly
- get_feature: "Feature: Title (state)" + key spec details + relevant history
- get_next_feature: "Next up: Title" or "No workable features"
- start_feature: "Started 'Title' — now in_progress"
- complete_feature: "Completed 'Title' — recorded N commits"
- list_versions: "0.1.0 (released), 0.2.0 (now, 3 features), 0.3.0 (next)""#;
