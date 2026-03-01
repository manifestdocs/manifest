//! MCP server for AI-assisted feature development.
//!
//! Exposes tools optimized for CLI agents like Claude Code:
//! - Discovery: list_projects, find_features, get_feature, render_feature_tree, sync
//! - Setup: init_project, add_project_directory, plan, create_feature
//! - Work: start_feature, complete_feature, get_next_feature
//! - Versions: list_versions, create_version, set_feature_version, release_version

use super::tools;
use super::types::{
    AddProjectDirectoryRequest, CompleteFeatureRequest, CreateFeatureRequest, CreateVersionRequest,
    DeleteFeatureRequest, FindFeaturesRequest, ForgetRequest, GenerateFeatureTreeRequest,
    GetActiveFeatureRequest, GetFeatureRequest, GetNextFeatureRequest, GetProjectHistoryRequest,
    GetProjectInstructionsRequest, InitProjectRequest, ListProjectsRequest, ListVersionsRequest,
    PlanFeaturesRequest, ProveFeatureRequest, RecallRequest, RecordVerificationRequest,
    ReleaseVersionRequest, RememberRequest, RenderFeatureTreeRequest, SetFeatureVersionRequest,
    StartFeatureRequest, SyncRequest, UpdateFeatureRequest, VerifyFeatureRequest,
};
use super::ManifestClient;
use rmcp::{
    handler::server::{
        tool::{ToolCallContext, ToolRouter},
        wrapper::Parameters,
    },
    model::{CallToolResult, Content, ListToolsResult, ServerInfo},
    service::RequestContext,
    tool, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};
use std::sync::{Arc, OnceLock};

// ============================================================
// Server Implementation
// ============================================================

pub struct McpServer {
    client: ManifestClient,
    tool_router: ToolRouter<Self>,
    /// Populated by a background task with an update notice if a newer
    /// version is available, or `None` if the server is up-to-date.
    update_notice: Arc<OnceLock<Option<String>>>,
}

impl McpServer {
    pub fn new(client: ManifestClient) -> Self {
        let update_notice = Arc::new(OnceLock::new());

        // Spawn a background task to check for updates. The notice is
        // appended to tool responses once the check completes.
        let cell = Arc::clone(&update_notice);
        tokio::spawn(async move {
            let notice = super::version_check::check_for_update().await;
            let _ = cell.set(notice);
        });

        Self {
            client,
            tool_router: Self::tool_router(),
            update_notice,
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
        description = "ORIENT: Find features by project, state, or search query. Returns summaries only. Use get_feature for full details. Call this BEFORE create_feature to check if a similar feature already exists."
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
        description = "ORIENT: Render the feature tree as ASCII art. Essential for understanding project structure, hierarchy, and current status (▣ project root, ▪ feature set, ◇ proposed, ○ in_progress, ● implemented, ✗ archived). Do NOT suggest changing parent feature states based on children's states."
    )]
    async fn render_feature_tree(
        &self,
        params: Parameters<RenderFeatureTreeRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::render_feature_tree(&self.client, params.0).await
    }

    #[tool(
        description = "ORIENT: Get recent activity across a project or for a specific feature. Returns a pre-rendered ASCII timeline of completed work grouped by time — display it directly to the user WITHOUT summarizing, reformatting, or adding commentary. The output mirrors the web app's Activity tab. Use when the user asks for 'recent activity', 'what happened', or 'show history'."
    )]
    async fn get_project_history(
        &self,
        params: Parameters<GetProjectHistoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::projects::get_project_history(&self.client, params.0).await
    }

    // ============================================================
    // Setup Tools
    // ============================================================

    #[tool(
        description = "SETUP: Initialize a project from a directory. Analyzes codebase, creates project (or links to existing), and returns analysis. Typical setup sequence: init_project → generate_feature_tree (existing codebases) → plan (decompose PRD) → update_feature (distill root with project context) → create_version (define milestones)."
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
        description = "SETUP: Decompose a PRD or vision into a feature tree. BEFORE calling, use render_feature_tree to see existing structure — extend it rather than replacing. Parent features should have shared context in details (architecture, patterns, constraints); leaf features should have concise specifications. Always provide target_version_id so features land in a release — call list_versions first or create_version if none exist. Omitting it sends features to the Backlog. With confirm=false, returns a proposal. With confirm=true, creates the features. IMPORTANT: After confirming, use update_feature to distill the root feature — replace the full PRD with high-level project context (tech stack, conventions, architecture) since detailed content now lives in child features."
    )]
    async fn plan(
        &self,
        params: Parameters<PlanFeaturesRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::plan(&self.client, params.0).await
    }

    #[tool(
        description = "DISCOVER: Reconcile the feature tree with codebase changes made outside Manifest. Analyzes git commit history, matches commits against existing features, and returns structured proposals: features to mark as implemented, features needing detail updates, and new capabilities to add. Does NOT modify anything — review the proposals and apply them using update_feature, complete_feature, or create_feature."
    )]
    async fn sync(&self, params: Parameters<SyncRequest>) -> Result<CallToolResult, McpError> {
        tools::sync::sync(&self.client, params.0).await
    }

    #[tool(
        description = "SETUP: Create a single feature. BEFORE creating, call find_features to check for duplicates and render_feature_tree to find the right parent group. Features describe what the system CAN DO, not what you DID. They outlive the work that created them. Good: 'OAuth Login', 'CSV Export'. Bad: 'Fix login bug', 'Update icons', 'Add retry logic'. If the title describes a one-time action, update an existing feature instead of creating a new one. Use parent_id to place under an existing group. For leaf features, add a concise specification in details. For parent features, add shared context that applies to all children. Use `plan` for bulk creation."
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
        description = "CLAIM: Signal you are starting work. Transitions state to 'in_progress' and records your claim (agent_type + metadata). Returns full feature details—this is your spec to implement. IMPORTANT: You MUST call this tool when a user asks you to implement, work on, or build a feature—even if you just created the feature or already have context. Also handles implemented features with pending changes (desired_details set by a human edit)—transitions implemented → in_progress so you can implement the requested changes. Do not change the feature's target version during implementation.\n\nCONCURRENCY: If another agent has claimed this feature, returns a conflict warning with details (who claimed it, when, branch info). Use force=true to override the claim.\n\nLEAF FEATURES ONLY: This tool rejects feature sets (parents with children). If you need to work on an area that has children, start one of its child features instead."
    )]
    async fn start_feature(
        &self,
        params: Parameters<StartFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::start_feature(&self.client, params.0).await
    }

    #[tool(
        description = "DOCUMENT: Mark work as done. Records a history entry with your summary and commits, sets state to 'implemented', and clears the agent claim. Automatically clears desired_details if present (pending change request fulfilled). Call only after verification.\n\nLEAF FEATURES ONLY: This tool rejects feature sets (parents with children). Only leaf features can be completed. If a parent's children are all implemented, the parent's status is derived automatically — do NOT attempt to complete the parent.\n\nYour summary becomes living documentation. Describe what was built, key decisions, and context for future agents. NEVER reference commits in the summary (e.g. 'Committed as abc1234') — commits are tracked separately via the commits parameter and displayed alongside the summary in the UI.\n\nWARNINGS: Returns advisory warnings when test evidence is missing (no prove_feature call) or when the feature spec was not updated after implementation (no update_feature call). Address these warnings before considering the feature complete."
    )]
    async fn complete_feature(
        &self,
        params: Parameters<CompleteFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::complete_feature(&self.client, params.0).await
    }

    #[tool(
        description = "BUILD: Record test evidence for a feature. Creates a proof record with the test command, exit code, structured test results, and evidence file paths. Can be called multiple times during TDD (red/green cycle) or once after tests pass. Proof is separate from completion — call this to record evidence, then call complete_feature when done.\n\nThe agent is the universal adapter: run any test framework, parse its output, and produce structured results. Include { name, suite, state, file, line, duration_ms, message } for each test for consistent rendering in both CLI and web UI.\n\nIn TDD mode (testing_policy: tdd), complete_feature REQUIRES a passing proof (exit_code 0). If tests are failing, you must fix the implementation and call prove_feature again — repeat until green. In advisory mode, proof is encouraged but not blocking."
    )]
    async fn prove_feature(
        &self,
        params: Parameters<ProveFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::prove_feature(&self.client, params.0).await
    }

    #[tool(
        description = "UPDATE: Modify any feature field. Use it to change title, details, state, priority, parent, version assignment, or propose changes for human review via desired_details. Prefer updating existing features over creating duplicates. Use parent_id to reorganize scattered features under the right group."
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
        description = "ORIENT: Get the highest-priority workable feature. Returns the top 'proposed' feature from the next unreleased version (prefers proposed over in_progress). Use ONLY when the user explicitly asks for \"the next feature\", \"what's next\", or \"what should I work on\". Do NOT use this when the user references a specific feature—use get_active_feature instead.\n\nIf the returned feature is 'in_progress', another agent has already claimed it—skip it and call find_features with state='proposed' to find unclaimed work instead."
    )]
    async fn get_next_feature(
        &self,
        params: Parameters<GetNextFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::get_next_feature(&self.client, params.0).await
    }

    // ============================================================
    // Verification Tools
    // ============================================================

    #[tool(
        description = "VERIFY: Assemble the feature spec + implementation diff so you can check the implementation against the spec. Returns spec context + filtered diff with instructions. After analyzing, call record_verification with your findings. No LLM is called server-side — you are the LLM."
    )]
    async fn verify_feature(
        &self,
        params: Parameters<VerifyFeatureRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::verify_feature(&self.client, params.0).await
    }

    #[tool(
        description = "VERIFY: Store verification comments produced by your analysis of verify_feature output. Pass an empty comments array if the implementation fully satisfies the spec. Severity levels: 'critical' (core requirement missing), 'major' (significant gap), 'minor' (style or polish drift)."
    )]
    async fn record_verification(
        &self,
        params: Parameters<RecordVerificationRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::features::record_verification(&self.client, params.0).await
    }

    // ============================================================
    // Memory Tools
    // ============================================================

    #[tool(
        description = "LEARN: Store a project memory — a fact, decision, or architectural observation that should persist across sessions. Use this to record things the agent would otherwise forget when the context window resets: key constraints, architectural decisions, environment quirks, and cross-cutting patterns. Memories are searchable via recall."
    )]
    async fn remember(
        &self,
        params: Parameters<RememberRequest>,
    ) -> Result<CallToolResult, McpError> {
        tools::memories::remember(&self.client, params.0).await
    }

    #[tool(
        description = "LEARN: Search or list project memories. Use this at the start of a session to recover context from previous work. Returns memories ranked by relevance (FTS5 on SQLite, LIKE fallback on PostgreSQL). Omit query to list all memories."
    )]
    async fn recall(&self, params: Parameters<RecallRequest>) -> Result<CallToolResult, McpError> {
        tools::memories::recall(&self.client, params.0).await
    }

    #[tool(
        description = "LEARN: Delete a project memory that is no longer accurate or relevant. Use recall first to find the memory_id."
    )]
    async fn forget(&self, params: Parameters<ForgetRequest>) -> Result<CallToolResult, McpError> {
        tools::memories::forget(&self.client, params.0).await
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

    async fn call_tool(
        &self,
        request: rmcp::model::CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let tcc = ToolCallContext::new(self, request, context);
        let mut result = self.tool_router.call(tcc).await?;

        // Append update notice once the background check has completed
        if let Some(Some(notice)) = self.update_notice.get() {
            result.content.push(Content::text(notice));
        }

        Ok(result)
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: self.tool_router.list_all(),
            meta: None,
            next_cursor: None,
        })
    }
}

const INSTRUCTIONS: &str = r#"<manifest_instructions>
<overview>
Manifest is living documentation for the software we are building in this project. It provides context: what the system does, what has been built, what needs work, and why decisions were made. Read feature specs before implementing, check history to see prior work, and update features when you complete work.
</overview>

<feature_tree>
Every project has a feature tree—a hierarchy of capabilities the system provides. The tree structure groups related features (e.g., Auth > Login > OAuth). Parent features (feature sets) can have details too—use them for shared context like architectural decisions, conventions, or constraints that apply to all children. Each feature has a state:

▪ feature set — parent with children (state not shown; managed by user)
◇ proposed — in the backlog, not yet started
○ in_progress — actively being worked on
● implemented — complete and documented
✗ archived — soft-deleted, kept for historical reference

Feature sets (parents with children) have immutable state — the server rejects state changes on them. Only leaf features can be started, completed, or archived. If get_next_feature or get_active_feature returns a feature set, work on its children instead.
</feature_tree>

<versions>
Versions use semantic versioning (e.g., 0.1.0, 0.2.0, 1.0.0) and organize features into releases. Each version has a lifecycle status:
- **next** — first unreleased version, next to ship, highest priority
- **planned** — remaining unreleased versions, queued for future releases
- **released** — shipped; features CANNOT be assigned to released versions

Features without a version assignment are in the **Backlog**—unscheduled work. By default, new features go to the Backlog. When you start working on a backlog feature (start_feature), it automatically moves to the "next" version.

Version tools:
- list_versions — see Next, Planned, Released, and Backlog counts
- create_version — define milestones (e.g., "0.2.0", "v1.0.0")
- set_feature_version — assign features to unreleased versions only (pass null for Backlog)
- release_version — mark a version as shipped

When all features in the "next" version are implemented, ask the user before calling release_version.
</versions>

<tool_selection>
When the user asks you to work on something, use these tools to find the right feature:

- get_active_feature — the feature selected in the Manifest app. Call this FIRST when the user says "this feature", "work on this", "implement it", or gives instructions without specifying which feature. After calling, confirm by naming the feature.
- get_next_feature — highest-priority proposed feature from the next unreleased version. Use ONLY when the user explicitly says "next feature", "what's next", or "what should I work on next". If it returns an in_progress feature, skip it (another agent claimed it) and use find_features with state='proposed' instead.
- find_features — search by project, state, or keyword
- get_feature — full details and history for a known feature ID
- get_project_instructions — full project instructions when the breadcrumb summary isn't enough
- render_feature_tree — display the full tree as ASCII art
- get_project_history — recent activity timeline. Use when the user says "activity", "recent activity", "what happened", "show history", or "changelog". Returns completed work with summaries and commits grouped by time.

RULE: The word "next" triggers get_next_feature. The word "activity" or "history" triggers get_project_history. Everything else triggers get_active_feature.
</tool_selection>

<features_as_docs>
Features describe system capabilities, not work items to close. A feature titled "Router" should make sense years from now.

**Every leaf feature spec MUST have these two parts:**
1. **User story** opening line: "As a [user], I can [capability] so that [benefit]."
2. **Acceptance criteria** as checkbox items: concrete assertions that can be verified in specs and tests.

Example of a GOOD spec (this is the minimum — write more detail when the feature warrants it):

  As a user, I can mark a todo as complete so that I can track my progress.

  Tapping the checkbox next to a todo toggles its completed state. Completed todos display with strikethrough styling.

  - [ ] Checkbox appears to the left of each todo item
  - [ ] Clicking the checkbox toggles the `completed` boolean
  - [ ] Completed todos render with line-through text decoration
  - [ ] Toggling is immediate — no confirmation dialog

Example of a BAD spec (too terse, no user story, no acceptance criteria):

  Clicking a checkbox next to a todo toggles its completed state. Completed todos show with line-through styling.

start_feature returns tier-specific guidance for writing specs at each level (project, feature set, leaf). To write a spec, use update_feature with `details` to set it directly, or `desired_details` to propose changes for human review.

When a human edits an implemented feature in the web UI, changes are saved to `desired_details` — start_feature returns guidance for handling these change requests.

Write what the agent cannot discover from code — intent, business rules, edge cases, acceptance criteria. More complex features should have proportionally more detail: additional context, more acceptance criteria, edge cases, and constraints. Do NOT put file paths, directory layouts, codebase overviews, or step-by-step implementation plans in specs — agents discover code structure on their own and extra context degrades performance.
</features_as_docs>

<updating_features>
update_feature is the Swiss Army knife for modifying features:
- Change state: Set to 'in_progress', 'implemented', 'archived' as appropriate
- Update spec: Modify details when implementation reveals new information
- Propose changes: Set desired_details to suggest changes for human review
- Reorganize: Change parent_id to move features in the tree
- Reprioritize: Adjust priority to reorder within parent

delete_feature permanently removes a feature and all its descendants. Use only for archived features. Prefer archiving to preserve history.
</updating_features>

<planning>
When asked to break down, plan, or decompose a project into features:
1. Call get_project_instructions to read the root feature content
2. Call render_feature_tree to see existing features — plan ADDITIONS to the tree, not a replacement
3. If the root has content (PRD, spec, or description), use that as input — do NOT explore the filesystem or ask the user what the project is about
4. Call list_versions to find the target version (use the "next" unreleased version)
5. Design the feature tree, merging into existing groups where possible
6. Call plan with confirm=false to propose
7. After user confirms, call plan with confirm=true
8. Distill the root — replace the verbatim PRD with high-level project context using update_feature
</planning>

<organization>
Before creating features, survey what exists:
1. render_feature_tree — see the full hierarchy and identify where new capabilities belong
2. find_features with a search query — check if a similar feature already exists

When a matching feature exists:
- Update it (update_feature) instead of creating a duplicate
- If it needs restructuring, use parent_id to move it under the right group
- If it's archived but relevant again, set state back to 'proposed'

When creating new features:
- Place them under existing parent groups where they fit
- Only create new parent groups when no existing group covers the domain
- Prefer fewer, well-organized features over many scattered ones
</organization>

<workflow>
1. ORIENT — understand what exists and what's needed:
   - list_projects (filter by directory_path to find project for your CWD)
   - render_feature_tree — see the full picture
   - get_active_feature — check what the user is looking at
   - get_feature (include_history=true) — read the spec AND what's been done before
   - get_next_feature — find highest-priority work

2. CLAIM — MANDATORY before implementing:
   - ALWAYS call start_feature when asked to implement, work on, or build a feature
   - start_feature checks specification completeness and transitions proposed → in_progress
   - If the feature is ALREADY in_progress, another agent is likely working on it — ask the user before proceeding, and offer the next proposed feature as an alternative
   - If the feature has no details, start_feature will refuse — write a spec first using update_feature
   - If details are very sparse, you will see a warning — flesh out the spec before implementing

3. BUILD — implement against the spec:
   - The feature details ARE your specification
   - Check breadcrumb for parent context (architectural decisions, conventions, constraints)
   - If desired_details is present, this is a CHANGE REQUEST: compare desired_details with details
   - Follow the testing guidance in the start_feature response — it tells you your first step
   - If testing_policy is "tdd": write failing tests first, call prove_feature (red), implement, call prove_feature again (green)
   - If testing_policy is "advisory": write tests alongside implementation, call prove_feature when tests pass
   - prove_feature records test evidence separately from completion — call it whenever you have test results
   - Include structured test results: { name, suite, state, file, line, duration_ms, message }
   - The agent is the universal adapter: run any test framework, parse its output into the structured format
   - ITERATE until green: if tests fail after implementation, fix the code and call prove_feature again. Repeat until all tests pass. In TDD mode, complete_feature will reject if the latest proof has a non-zero exit code.
   - As you implement, update_feature details to reflect what you actually built

4. DOCUMENT — MANDATORY after implementing:
   a) call update_feature to set details to what was actually built
   b) call prove_feature to record test evidence — do this in ALL modes, not just TDD
   c) call complete_feature with a summary of what you did + commit SHAs
   NOTE: In TDD mode, complete_feature REQUIRES a passing proof (exit_code 0). In advisory mode, complete_feature returns warnings if prove_feature was not called or if the spec was not updated — address these before considering the feature done.

Common sequence: list_projects → get_active_feature → start_feature → [implement] → prove_feature → update_feature (details) → complete_feature
</workflow>

<critical_rules>
ALWAYS call start_feature before implementing — it checks spec completeness and returns your specification.
ALWAYS call update_feature after implementing — details must reflect what was actually built.
ALWAYS call prove_feature before complete_feature — in all modes. TDD mode blocks completion without proof; advisory mode returns warnings.
ALWAYS call complete_feature with summary and commit SHAs — this records history and marks the feature done.
In TDD mode: iterate until tests pass. If prove_feature records failing tests, fix the implementation and call prove_feature again. Do NOT call complete_feature until the latest proof has exit_code 0 — the server will reject it.
NEVER change a feature's target version during implementation.
NEVER call start_feature or complete_feature on a feature set (▪ symbol, has children). These tools reject non-leaf features. Work on leaf children instead.
If a feature is already in_progress, skip it — another agent claimed it. Use find_features with state='proposed' to find unclaimed work instead.
The word "next" triggers get_next_feature. All other references trigger get_active_feature.
</critical_rules>
</manifest_instructions>"#;
