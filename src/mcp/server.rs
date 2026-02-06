//! MCP server for AI-assisted feature development.
//!
//! Exposes tools optimized for CLI agents like Claude Code:
//! - Discovery: list_projects, find_features, get_feature, render_feature_tree
//! - Setup: init_project, add_project_directory, plan, create_feature
//! - Work: start_feature, complete_feature, get_next_feature
//! - Versions: list_versions, create_version, set_feature_version, release_version

use super::tools;
use super::types::{
    AddProjectDirectoryRequest, CompleteFeatureRequest, CreateFeatureRequest, CreateVersionRequest,
    DeleteFeatureRequest, FindFeaturesRequest, GenerateFeatureTreeRequest, GetActiveFeatureRequest,
    GetFeatureRequest, GetNextFeatureRequest, GetProjectInstructionsRequest, InitProjectRequest,
    ListProjectsRequest, ListVersionsRequest, PlanFeaturesRequest, ReleaseVersionRequest,
    RenderFeatureTreeRequest, SetFeatureVersionRequest, StartFeatureRequest, UpdateFeatureRequest,
};
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
        description = "ORIENT: Get full project instructions (coding guidelines, conventions, architectural decisions). Use this when the breadcrumb summary indicates more context is available. Returns the complete root feature details."
    )]
    async fn get_project_instructions(
        &self,
        params: Parameters<GetProjectInstructionsRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::get_project_instructions(&self.client, params.0).await
    }

    #[tool(
        description = "ORIENT: Get the feature the user is currently looking at in the Manifest app. This is your DEFAULT tool for resolving what the user means—call it first when they say \"this feature\", \"work on this\", \"implement it\", or give instructions without naming a specific feature. Requires project_id — call list_projects first if you don't have it. Returns null if no feature is selected. After calling, confirm by naming the feature in your response (e.g., \"I see you have 'OAuth Login' selected\")."
    )]
    async fn get_active_feature(
        &self,
        params: Parameters<GetActiveFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::context::get_active_feature(&self.client, params.0).await
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
        description = "ORIENT/BUILD: Get detailed feature spec with hierarchical context. Returns the feature details plus breadcrumb with ancestor context (architectural decisions, conventions). Set include_history=true to see past work. READ THIS before starting work."
    )]
    async fn get_feature(
        &self,
        params: Parameters<GetFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::get_feature(&self.client, params.0).await
    }

    #[tool(
        description = "ORIENT: Render the feature tree as ASCII art. Essential for understanding project structure, hierarchy, and current status (▣ project root, ▪ feature set, ◇ proposed, ○ in_progress, ● implemented, ✗ archived)."
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
        description = "DISCOVER: Generate a feature tree from an existing codebase by analyzing code structure and git history. Use `since` to limit to recent commits (e.g., 'v1.0.0'). Returns a markdown document describing system capabilities."
    )]
    async fn generate_feature_tree(
        &self,
        params: Parameters<GenerateFeatureTreeRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::generate::generate_feature_tree(params.0).await
    }

    #[tool(
        description = "SETUP: Decompose a PRD or vision into a feature tree. Parent features should have shared context in details (architecture, patterns, constraints); leaf features should have concise specifications. Always provide target_version_id so features land in a release — call list_versions first or create_version if none exist. Omitting it sends features to the Backlog. With confirm=false, returns a proposal. With confirm=true, creates the features. IMPORTANT: After confirming, use update_feature to distill the root feature — replace the full PRD with high-level project context (tech stack, conventions, architecture) since detailed content now lives in child features."
    )]
    async fn plan(
        &self,
        params: Parameters<PlanFeaturesRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::plan(&self.client, params.0).await
    }

    #[tool(
        description = "SETUP: Create a single feature. Name by capability (e.g., 'Router') not task. Use parent_id for grouping. For leaf features, add a concise specification in details. For parent features, add shared context that applies to all children. Use `plan` for bulk creation."
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
        description = "CLAIM: Signal you are starting work. Transitions state to 'in_progress'. Returns full feature details—this is your spec to implement. IMPORTANT: You MUST call this tool when a user asks you to implement, work on, or build a feature—even if you just created the feature or already have context. Also handles implemented features with pending changes (desired_details set by a human edit)—transitions implemented → in_progress so you can implement the requested changes. Do not change the feature's target version during implementation."
    )]
    async fn start_feature(
        &self,
        params: Parameters<StartFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::start_feature(&self.client, params.0).await
    }

    #[tool(
        description = "DOCUMENT: Mark work as done. Records a history entry with your summary and commits, then sets state to 'implemented'. Automatically clears desired_details if present (pending change request fulfilled). Set mark_implemented=false to record progress without changing state. Call only after verification.\n\nYour summary becomes living documentation. Describe what was built, key decisions, and context for future agents. NEVER reference commits in the summary (e.g. 'Committed as abc1234') — commits are tracked separately via the commits parameter and displayed alongside the summary in the UI."
    )]
    async fn complete_feature(
        &self,
        params: Parameters<CompleteFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::complete_feature(&self.client, params.0).await
    }

    #[tool(
        description = "UPDATE: Modify any feature field. This is the Swiss Army knife for feature updates—use it to change title, details, state, priority, parent, version assignment, or propose changes for human review via desired_details. Replaces narrow tools with one flexible tool + guidance."
    )]
    async fn update_feature(
        &self,
        params: Parameters<UpdateFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::update_feature(&self.client, params.0).await
    }

    #[tool(
        description = "CLEANUP: Permanently delete a feature and all its descendants. Use this only for archived features that are no longer needed. This action cannot be undone."
    )]
    async fn delete_feature(
        &self,
        params: Parameters<DeleteFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::delete_feature(&self.client, params.0).await
    }

    #[tool(
        description = "ORIENT: Get the highest-priority workable feature. Returns the top 'proposed' or 'in_progress' feature from the next unreleased version. Use ONLY when the user explicitly asks for \"the next feature\", \"what's next\", or \"what should I work on\". Do NOT use this when the user references a specific feature—use get_active_feature instead."
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
        description = "ORIENT: List versions. Returns release milestones with status indicators: 'next' (next to ship), 'planned' (upcoming), and 'released'."
    )]
    async fn list_versions(
        &self,
        params: Parameters<ListVersionsRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::versions::list_versions(&self.client, params.0).await
    }

    #[tool(
        description = "PLAN: Create a release milestone. Names must be semantic versions in MAJOR.MINOR.PATCH format (e.g., '0.2.0', 'v1.0.0'). Freeform text like 'MVP' or status labels like 'next' are rejected."
    )]
    async fn create_version(
        &self,
        params: Parameters<CreateVersionRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::versions::create_version(&self.client, params.0).await
    }

    #[tool(
        description = "PLAN: Assign a feature to a release version. Only unreleased versions are valid targets. Pass null to unassign."
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
Every project has a feature tree—a hierarchy of capabilities the system provides. The tree structure groups related features (e.g., Auth > Login > OAuth). Parent features (feature sets) can have details too—use them for shared context like architectural decisions, conventions, or constraints that apply to all children. Each feature has a state:

▪ feature set — parent with children (state not shown; managed by user)
◇ proposed — in the backlog, not yet started
○ in_progress — actively being worked on
● implemented — complete and documented
✗ archived — soft-deleted, kept for historical reference

IMPORTANT: Parent feature states are managed independently by the user. Do NOT suggest changing a parent's state based on its children's states. A feature set marked ◇ proposed with all ● implemented children is normal — the parent may be proposed because the user plans to add more children, or simply hasn't updated it yet. Only change parent states when the user explicitly asks.

DISCOVERING FEATURES:
When the user asks you to work on something, use these tools to find the right feature:

- get_active_feature — returns the feature selected in the Manifest app. Call this FIRST when the user says "this feature", "work on this", "implement it", or gives instructions without specifying which feature. After calling, confirm by naming the feature (e.g., "I'll work on 'OAuth Login'").
- get_next_feature — returns the highest-priority proposed or in_progress feature from the next unreleased version. Use ONLY when the user explicitly says "next feature", "what's next", or "what should I work on next".
- find_features — search features by project, state, or keyword when you need to locate a specific feature by name
- get_feature — get full details and history for a feature you already have an ID for
- get_project_instructions — get full project instructions when the breadcrumb summary isn't enough
- render_feature_tree — display the full tree as ASCII art for the user

RULE: The word "next" triggers get_next_feature. Everything else triggers get_active_feature.

VERSIONS & BACKLOG:
Versions use semantic versioning (e.g., 0.1.0, 0.2.0, 1.0.0) and organize features into releases. Each version has a lifecycle status:
- **next** — first unreleased version, next to ship, highest priority
- **planned** — remaining unreleased versions, queued for future releases
- **released** — shipped; features CANNOT be assigned to released versions

Features without a version assignment are in the **Backlog**—unscheduled work. By default, new features go to the Backlog. When you start working on a backlog feature (start_feature), it automatically moves to the "next" version.

Assigning features to a released version will be rejected with an error. Use list_versions to find valid (unreleased) targets.

FEATURES AS LIVING DOCUMENTATION:
Features describe system capabilities, not work items to close. A feature titled "Router" should make sense years from now. Before creating one, apply the user story test: "As a [user], I can [capability] so that [benefit]."
- Good: "As a developer, I can match dynamic URL paths so that I can build REST APIs" → Router
- Bad: "As a user, I can have data persistence" → quality attribute, not capability

CONTENT GUIDANCE BY TIER:
Features form a three-tier hierarchy. Write different content at each level:

PROJECT LEVEL (root feature — the top-level feature with no parent):
This is the project's source of truth for all agents. Write content that applies across every feature:
- Tech stack and key dependencies (language, framework, database)
- Architectural decisions and rationale ("We use X because Y")
- Coding conventions and patterns ("Error handling uses Result<T,E>, never exceptions")
- Security boundaries and constraints ("Never commit secrets", "All endpoints require auth")
- Testing expectations ("TDD with property-based tests for core logic")
- Domain terminology ("User means authenticated account, not session")

When updating project instructions (root feature details), also provide a details_summary (~200 words) via update_feature. The summary appears in breadcrumbs and project listings; agents call get_project_instructions for full text when they need it.

FEATURE SET LEVEL (parent feature — has children):
Shared context for a group of related capabilities. Write content that applies to all children:
- Architectural context for this area ("Auth uses JWT with refresh tokens")
- Shared patterns and interfaces ("All handlers implement the RequestHandler trait")
- Cross-cutting constraints ("All endpoints in this group require admin role")
- Design decisions specific to this scope ("We chose OAuth over SAML because...")

LEAF FEATURE LEVEL (no children — the implementable unit):
Concise specification that an agent implements against:
- Goal statement: what the feature does and why (~1-2 sentences)
- Key constraints: performance, security, compatibility requirements
- For interface-heavy features: function signatures with types
- For complex logic: structural hints (main sequence, branching, loops)
- 1-3 concrete examples of expected behavior when helpful

Specification length and guidance adapt to the project's configured `ac_level` and `detail_level` settings (concise, standard, or thorough). The project also has an `ac_format` setting (checkbox or gherkin) that controls how acceptance criteria are formatted. The `start_feature` and `get_next_feature` tools return the active levels and tailored guidance text.

start_feature will block if a leaf feature has no details at all — write a spec first using update_feature.
If details are very sparse, you will receive a warning (threshold adapts to ac_level).

To write a spec:
- Use update_feature with `details` to set the spec directly
- Use update_feature with `desired_details` to propose a spec for human review (they see a diff in the web UI)

CHANGE REQUESTS (desired_details set by humans):
When a human edits an implemented feature in the web UI, changes are saved to `desired_details` instead of overwriting `details`. This creates a pending change request visible as a "changes" badge. When you call start_feature on such a feature, it transitions implemented → in_progress and you receive guidance to compare desired_details with details. After implementing the changes, update details and call complete_feature (which clears desired_details automatically).

Specification length is guided by the project's ac_level setting. After implementation, update details to reflect what was built.

UPDATING FEATURES:
update_feature is the Swiss Army knife for modifying features. Use it to:
- Change state: Set to 'in_progress', 'implemented', 'archived' as appropriate
- Update spec: Modify details when implementation reveals new information
- Propose changes: Set desired_details to suggest changes for human review (they see a diff in web UI)
- Reorganize: Change parent_id to move features in the tree
- Reprioritize: Adjust priority to reorder within parent

DELETING FEATURES:
delete_feature permanently removes a feature and all its descendants. Use it only for archived features that are no longer needed. This cannot be undone. Prefer archiving (update_feature with state='archived') to preserve history.

VERSIONS & PLANNING:
- list_versions — see Next (next to ship), Planned, Released, and Backlog counts. Each version includes a `status` field.
- create_version — define milestones with semantic versions (e.g., "0.2.0", "v1.0.0")
- set_feature_version — assign features to unreleased versions only (pass null to move to Backlog). Released versions are rejected.
- release_version — mark a version as shipped (auto-creates new versions to maintain minimum of 4 unreleased)

When all features in the "next" version are implemented, ask the user before calling release_version. Releasing promotes the next planned version to become the new "next". New versions are auto-created to maintain at least 4 unreleased versions.

SETUP (when starting fresh):
1. init_project — analyze codebase, create project, link directory
2. generate_feature_tree — for existing codebases, extract features from code structure and git history
3. plan — break down a PRD, tech spec, or vision into a feature tree
4. **After plan: distill the root** — plan distributes content to children but does NOT update the root. Use update_feature to replace the root's PRD/spec with high-level project context (tech stack, conventions, architecture). Set details_summary too. Skip if the root already has appropriate project-level content.
5. add_project_directory — for monorepos with multiple directories
6. create_version — define release milestones

DISPLAY GUIDELINES:
Tool results are collapsed JSON. Always summarize for humans:
- list_projects: "Found project 'Name'" or "No project found for this directory"
- find_features: "Found N features" + brief list
- get_feature: "Feature: Title (state)" + key spec details + breadcrumb context if relevant
- get_active_feature: "You have '[Title]' selected ([state])" or "No feature is currently selected in the app"
- get_next_feature: "Next up: Title" or "No workable features"
- render_feature_tree: Show the ASCII tree directly. Do NOT suggest changing parent feature states based on children
- init_project: "Initialized 'Name' with N detected modules"
- generate_feature_tree: "Extracted N features from codebase" + summary
- plan: "Proposed N features" (confirm=false) or "Created N features" (confirm=true)
- start_feature: "Started 'Title' — now in_progress" + note any spec warnings or breadcrumb context. If blocked: "Cannot start 'Title' — specification required"
- complete_feature: "Completed 'Title' — recorded N commits"
- update_feature: "Updated 'Title'" + what changed
- list_versions: "0.1.0 (released), 0.2.0 (next, 3 features), 0.3.0 (planned)"
- create_version: "Created version 'Name'"
- set_feature_version: "Assigned 'Feature' to version 'Name'" or "Unassigned from version"
- get_project_instructions: Show key sections or confirm instructions were retrieved
- release_version: "Released 'Name'"

WORKFLOW:

1. ORIENT — understand what exists and what's needed:
   - list_projects (filter by directory_path to find project for your CWD)
   - render_feature_tree — see the full picture
   - get_active_feature — check what the user is looking at
   - get_feature (include_history=true) — read the spec AND what's been done before
   - get_next_feature — find highest-priority work

2. CLAIM — MANDATORY before implementing:
   - ALWAYS call start_feature when asked to implement, work on, or build a feature
   - start_feature checks specification completeness and transitions proposed → in_progress
   - start_feature also handles CHANGE REQUESTS: if an implemented feature has desired_details (set by a human edit in the web UI), it transitions implemented → in_progress and returns guidance explaining what changed
   - If the feature has no details, start_feature will refuse — write a spec first using update_feature
   - If details are very sparse, you will see a warning — flesh out the spec before implementing
   - IMPORTANT: The feature's target version is locked during implementation. Do not call set_feature_version while working on a feature.

3. BUILD — implement against the spec:
   - The feature details ARE your specification
   - Check breadcrumb for parent context (architectural decisions, conventions, constraints)
   - If desired_details is present, this is a CHANGE REQUEST: compare desired_details (what's wanted) with details (what's currently built) to understand what needs to change. Update details to reflect what you build.
   - Write tests first, then implement, then verify
   - Use update_feature to evolve the spec as you learn more

4. DOCUMENT — MANDATORY after implementing:
   - You MUST call complete_feature when work is done. This is not optional.
   - Provide a summary of what you did + commit SHAs
   - complete_feature automatically clears desired_details when marking as implemented
   - This creates a history entry so future agents (or future you) know what happened
   - If you skip this step, there is no record of the work and the feature stays in_progress forever
   - If you learned something that applies to sibling features, update the parent's details with shared context

Common sequence: list_projects → get_active_feature → start_feature → [implement] → complete_feature"#;
