//! Request and response types for MCP tools.
//!
//! Types derive both [`Deserialize`](serde::Deserialize) and [`JsonSchema`](rmcp::schemars::JsonSchema)
//! so they serve double duty: schemars generates the JSON Schema that agents see
//! in tool definitions, while serde handles runtime deserialization of tool arguments.

use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::{BreadcrumbItem, Feature, FeatureSummaryContext, FeatureWithContext};
use crate::serde_helpers::default_true;

// ============================================================
// Request Types
// ============================================================

/// A reference to a git commit for MCP input.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartFeatureRequest {
    #[schemars(description = "The UUID of the feature to start working on")]
    pub feature_id: String,
    #[schemars(
        description = "Agent type claiming this feature: 'claude', 'gemini', 'codex', or 'copilot'. Defaults to 'claude'."
    )]
    #[serde(default = "default_agent_type")]
    pub agent_type: String,
    #[schemars(
        description = "Force start even if another agent has claimed this feature or the spec lacks testable criteria. Default false."
    )]
    #[serde(default)]
    pub force: bool,
    #[schemars(
        description = "Optional JSON metadata about the agent's execution context (e.g., branch name, worktree path, container ID)."
    )]
    #[serde(default)]
    pub claim_metadata: Option<String>,
}

fn default_agent_type() -> String {
    "claude".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteFeatureRequest {
    #[schemars(description = "Feature ID (UUID, display ID like 'MAN-42', or UUID prefix)")]
    pub feature_id: String,
    #[schemars(
        description = "Summary of work done (git-style format). First line is a concise headline shown in list views. Add details after a blank line if needed (bullet points, technical notes).\n\nYour summary becomes living documentation. Include:\n- Key DECISIONS and their rationale (\"Chose X over Y because...\")\n- Any DEVIATIONS from the original spec and why\n- DISCOVERIES that affect sibling features (\"Discovered that...\")\n- Constraints or gotchas for future work (\"Note: X requires Y\")\n\nExample:\n\nImplemented OAuth login flow\n\n- Added Google OAuth provider using passport.js\n- Chose session-based auth over JWT (simpler for SSR app)\n- Deviated from spec: skipped GitHub OAuth (rate limits too restrictive)\n- Discovered that OAuth callback must use HTTPS even in dev\n- Note: refresh token rotation not yet implemented\n\nIMPORTANT: Describe WHAT was built, not that something was committed. Commits are tracked separately and displayed alongside the summary in the UI. Never write summaries like 'Committed as abc1234' or 'Committed X implementation' — the commit SHAs are already shown."
    )]
    pub summary: String,
    #[schemars(
        description = "Git commits created during this work. Accepts either:\n- Simple strings: [\"abc1234\", \"def5678\"] (commit messages resolved automatically)\n- Objects: [{sha: \"abc1234\", message: \"Add feature\"}]"
    )]
    #[serde(default, deserialize_with = "deserialize_commits")]
    pub commits: Vec<CommitRefInput>,
    #[schemars(
        description = "Set to true when bootstrapping existing projects. Skips proof and spec requirements — the existing code is the proof. History entry is tagged as 'backfilled' for visual distinction from features that went through the full TDD cycle."
    )]
    #[serde(default)]
    pub backfill: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CommitRefInput {
    #[schemars(description = "The commit SHA (short or full)")]
    pub sha: String,
    #[schemars(description = "The commit message (first line)")]
    #[serde(default)]
    pub message: String,
    #[schemars(description = "The commit author")]
    #[serde(default)]
    pub author: Option<String>,
}

/// Deserialize commits from either:
/// - `["sha1", "sha2"]` (simple string array)
/// - `[{sha: "sha1", message: "msg"}]` (structured array)
fn deserialize_commits<'de, D>(deserializer: D) -> Result<Vec<CommitRefInput>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum CommitEntry {
        Simple(String),
        Structured(CommitRefInput),
    }

    let entries: Vec<CommitEntry> = Vec::deserialize(deserializer)?;
    Ok(entries
        .into_iter()
        .map(|e| match e {
            CommitEntry::Simple(sha) => CommitRefInput {
                sha,
                message: String::new(),
                author: None,
            },
            CommitEntry::Structured(c) => c,
        })
        .collect())
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProveFeatureRequest {
    #[schemars(description = "Feature ID (UUID, display ID like 'MAN-42', or UUID prefix)")]
    pub feature_id: String,
    #[schemars(
        description = "The test command that was run (e.g., 'cargo test auth_spec', 'pytest tests/auth')"
    )]
    pub command: String,
    #[schemars(description = "Process exit code (0 = success/all tests passed)")]
    pub exit_code: i32,
    #[schemars(
        description = "Raw stdout/stderr output (truncated to 10K chars). Optional — structured test results are preferred."
    )]
    #[serde(default)]
    pub output: Option<String>,
    #[schemars(
        description = "Structured test results grouped by suite (preferred). Each entry: { name, file?, tests: [{ name, state, file?, line?, duration_ms?, message? }] }."
    )]
    #[serde(default)]
    pub test_suites: Option<Vec<TestSuiteInput>>,
    #[schemars(
        description = "Flat test results (legacy, kept for backwards compatibility). Each entry: { name, suite?, state: 'passed'|'failed'|'errored'|'skipped', file?, line?, duration_ms?, message? }. Use test_suites instead when possible."
    )]
    #[serde(default)]
    pub tests: Option<Vec<TestResultInput>>,
    #[schemars(
        description = "Evidence file paths (relative to project root) with optional notes. Example: [{ path: 'tests/auth_test.rs', note: 'Auth integration tests' }]"
    )]
    #[serde(default)]
    pub evidence: Vec<EvidenceInput>,
    #[schemars(description = "Git commit SHA at the time of proving (short or full)")]
    #[serde(default)]
    pub commit_sha: Option<String>,
}

/// Structured test suite input for the new grouped format.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TestSuiteInput {
    #[schemars(description = "Suite or module name (e.g., 'db_spec', 'auth::tests')")]
    pub name: String,
    #[schemars(description = "Source file path shared by all tests in the suite")]
    #[serde(default)]
    pub file: Option<String>,
    #[schemars(description = "Individual test results within this suite")]
    pub tests: Vec<TestResultInput>,
}

/// Flat test result input (legacy format, kept for backwards compatibility).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TestResultInput {
    #[schemars(description = "Test name (e.g., 'creates a feature')")]
    pub name: String,
    #[schemars(
        description = "Test suite or module (e.g., 'db_spec'). Used for grouping in flat format."
    )]
    #[serde(default)]
    pub suite: Option<String>,
    #[schemars(description = "Result: 'passed', 'failed', 'errored', or 'skipped'")]
    pub state: String,
    #[schemars(description = "Source file path relative to project root")]
    #[serde(default)]
    pub file: Option<String>,
    #[schemars(description = "Line number in source file")]
    #[serde(default)]
    pub line: Option<u32>,
    #[schemars(description = "Duration in milliseconds")]
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[schemars(description = "Failure or error message")]
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EvidenceInput {
    #[schemars(description = "File path relative to project root")]
    pub path: String,
    #[schemars(description = "Note about why this file is evidence")]
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindFeaturesRequest {
    #[schemars(description = "Optional project UUID to filter features by project")]
    pub project_id: Option<Uuid>,
    #[schemars(
        description = "Optional version UUID to filter features by their target version assignment"
    )]
    pub version_id: Option<Uuid>,
    #[schemars(
        description = "Optional state filter: 'proposed', 'in_progress', 'implemented', or 'archived'"
    )]
    pub state: Option<String>,
    #[schemars(
        description = "Optional search query to match against title and details. When provided, returns features ranked by relevance."
    )]
    pub query: Option<String>,
    #[schemars(
        description = "Search mode: 'default' (LIKE search on title + details, current behavior) or 'full' (FTS5 full-text search with relevance ranking — better for natural language queries like 'Redis session state'). Defaults to 'default'."
    )]
    #[serde(default)]
    pub search_mode: Option<String>,
    #[schemars(description = "Maximum number of features to return. Defaults to no limit.")]
    pub limit: Option<u32>,
    #[schemars(description = "Number of features to skip for pagination. Defaults to 0.")]
    pub offset: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFeatureRequest {
    #[schemars(description = "The UUID of the feature to retrieve")]
    pub feature_id: String,
    #[schemars(
        description = "Include implementation history (past work summaries and commits). Defaults to false."
    )]
    #[serde(default)]
    pub include_history: bool,
    #[schemars(
        description = "Context depth: 'shallow' (spec only, no breadcrumb/siblings/children), 'standard' (spec + rich breadcrumb, default), 'deep' (spec + breadcrumb + history + siblings). Overrides include_history when set to 'deep'."
    )]
    #[serde(default)]
    pub depth: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteFeatureRequest {
    #[schemars(description = "The UUID of the feature to permanently delete")]
    pub feature_id: String,
}

/// General-purpose tool for updating any feature field.
/// Replaces narrow tools (archive, restore, specify, reopen, deprecate) with one flexible tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateFeatureRequest {
    #[schemars(description = "Feature ID (UUID, display ID like 'MAN-42', or UUID prefix)")]
    pub feature_id: String,

    #[schemars(description = "New title for the feature")]
    #[serde(default)]
    pub title: Option<String>,

    #[schemars(
        description = "New details for the feature. Content depends on the feature's role in the hierarchy: project-level features hold decisions and conventions; feature sets hold shared architectural context; leaf features hold concise specifications — goal, constraints, key interfaces. Use this to update living documentation as you learn more during implementation. Do not repeat the feature title in the details — the title is displayed separately."
    )]
    #[serde(default)]
    pub details: Option<String>,

    #[schemars(
        description = "Proposed changes for human review. Set this when you want to suggest spec changes without immediately applying them. Humans see a diff in the web UI and can accept/reject."
    )]
    #[serde(default)]
    pub desired_details: Option<String>,

    #[schemars(
        description = "Short summary (~200 words) of root feature instructions. When provided for a root feature, breadcrumbs and project listings show this summary instead of the full details. Agents call get_project_instructions for full text when needed."
    )]
    #[serde(default)]
    pub details_summary: Option<String>,

    #[schemars(
        description = "New state. Valid values: 'proposed', 'blocked', 'in_progress', 'implemented', 'archived'. Use your judgment: working on it? 'in_progress'. Found a bug in 'implemented'? Set back to 'in_progress'. Want to hide but preserve history? 'archived'. To mark work as DONE, use complete_feature instead — it records history. Only set 'implemented' directly here for corrections or bulk updates."
    )]
    #[serde(default)]
    pub state: Option<String>,

    #[schemars(
        description = "New priority for ordering within parent. Lower values appear first."
    )]
    #[serde(default)]
    pub priority: Option<i32>,

    #[schemars(
        description = "Move feature to a different parent (reparenting). Pass the parent feature UUID, or null to make it a root feature."
    )]
    #[serde(default)]
    pub parent_id: Option<Uuid>,

    #[schemars(
        description = "Assign to a version for release planning. Pass version UUID to assign, or use set_version_null=true to unassign. Released versions cannot be targeted."
    )]
    #[serde(default)]
    pub target_version_id: Option<Uuid>,

    #[schemars(
        description = "Set to true to explicitly unassign the feature from any version. Use this instead of passing null for target_version_id."
    )]
    #[serde(default)]
    pub clear_version: bool,

    #[schemars(
        description = "Feature IDs (UUIDs) that block this feature. Required when setting state to 'blocked'. All blockers must be in the same project. The feature auto-transitions to 'proposed' when all blockers reach 'implemented'."
    )]
    #[serde(default)]
    pub blocked_by: Option<Vec<Uuid>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddProjectDirectoryRequest {
    #[schemars(description = "The UUID of the project to add this directory to")]
    pub project_id: Uuid,
    #[schemars(description = "Absolute path to the directory (e.g., '/Users/me/projects/myapp')")]
    pub path: String,
    #[schemars(description = "Optional git remote URL (e.g., 'git@github.com:org/repo.git')")]
    #[serde(default)]
    pub git_remote: Option<String>,
    #[schemars(
        description = "Whether this is the primary directory for the project. Defaults to false."
    )]
    #[serde(default)]
    pub is_primary: bool,
    #[schemars(
        description = "Optional directory-specific instructions (build commands, test commands)"
    )]
    #[serde(default)]
    pub instructions: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateFeatureRequest {
    #[schemars(description = "The UUID of the project this feature belongs to")]
    pub project_id: Uuid,
    #[schemars(
        description = "Parent feature UUID. Check render_feature_tree to find the right group — most features belong under an existing parent rather than at the root."
    )]
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[schemars(
        description = "Short capability name (2-5 words). Describe what users can DO, not what you changed. Good: 'User Authentication', 'Markdown Export'. Bad: 'Fix auth bug', 'Update header styling', 'Add validation'. If the work is a refinement of an existing capability, update that feature instead."
    )]
    pub title: String,
    #[schemars(
        description = "Feature details. For leaf features: concise specification — goal, constraints, key interfaces. For parent features: shared context (architecture, patterns, constraints) that flows to children. IMPORTANT: Do not repeat the feature title in the details — the title is displayed separately."
    )]
    #[serde(default)]
    pub details: Option<String>,
    #[schemars(
        description = "Initial state: 'proposed' (default), 'in_progress', 'implemented', or 'archived'"
    )]
    #[serde(default = "default_proposed")]
    pub state: String,
    #[schemars(
        description = "Priority for ordering within parent. Lower values appear first. Defaults to 0."
    )]
    #[serde(default)]
    pub priority: Option<i32>,
}

fn default_proposed() -> String {
    "proposed".to_string()
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlanFeaturesRequest {
    #[schemars(description = "The UUID of the project to plan features for")]
    pub project_id: Uuid,
    #[schemars(
        description = "Optional target version UUID (must be unreleased). Features will be assigned to this version. If not provided, features go to the backlog (unassigned). Call list_versions first to get available versions."
    )]
    #[serde(default)]
    pub target_version_id: Option<Uuid>,
    #[schemars(
        description = "The proposed feature tree. Parent features should have shared context in details (architecture, patterns, constraints for children). Leaf features should have specifications — goal, constraints, key interfaces. Use the project's spec template if available."
    )]
    pub features: Vec<ProposedFeature>,
    #[schemars(
        description = "If true, creates the features in the database. If false (default), returns proposal for user review."
    )]
    #[serde(default)]
    pub confirm: bool,
}

// ============================================================
// Response Types
// ============================================================

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInfo {
    pub id: Uuid,
    pub title: String,
    /// Feature details including user stories, implementation notes, and technical context.
    pub details: Option<String>,
    /// Desired details for pending changes. When non-null, indicates edits awaiting implementation.
    pub desired_details: Option<String>,
    pub state: String,
    /// Priority for ordering within parent. Lower values appear first.
    pub priority: i32,
    /// Target version for release planning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_version_id: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct HistoryEntryInfo {
    pub id: Uuid,
    /// The version this work was done for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<Uuid>,
    /// Version name for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_name: Option<String>,
    pub summary: String,
    pub commits: Vec<CommitInfo>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CommitInfo {
    pub sha: String,
    pub message: String,
    pub author: Option<String>,
}

/// Lightweight feature summary without details (used for MCP list operations).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureSummaryInfo {
    pub id: Uuid,
    pub title: String,
    pub state: String,
    pub priority: i32,
    pub parent_id: Option<Uuid>,
}

/// Response for find_features in summary mode (default).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureListSummaryResponse {
    pub features: Vec<FeatureSummaryInfo>,
}

/// Lightweight feature summary for context (parent, siblings, children).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureSummaryContextInfo {
    pub id: Uuid,
    /// Human-friendly display ID (e.g., "MAN-42"). Omitted if not yet assigned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_id: Option<String>,
    pub title: String,
    pub state: String,
}

/// Breadcrumb item for navigation path (root → feature).
/// Includes details for ancestor context that flows down to children.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BreadcrumbItemInfo {
    pub id: Uuid,
    /// Human-friendly display ID (e.g., "MAN-42"). Omitted if not yet assigned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_id: Option<String>,
    pub title: String,
    /// Contextual details from this ancestor (architectural decisions, conventions, constraints).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

/// A feature with its hierarchical context (parent, siblings, children, breadcrumb).
/// Used by get_feature MCP tool to provide navigation context.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInfoWithContext {
    /// The feature itself with all details.
    pub id: Uuid,
    /// Human-friendly display ID (e.g., "MAN-42"). Omitted if not yet assigned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_id: Option<String>,
    pub title: String,
    /// Feature details including user stories, implementation notes, and technical context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    /// Desired details for pending changes. When non-null, indicates edits awaiting implementation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired_details: Option<String>,
    pub state: String,
    /// Priority for ordering within parent. Lower values appear first.
    pub priority: i32,
    /// Target version for release planning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_version_id: Option<Uuid>,
    /// Parent feature (if not a root).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<FeatureSummaryContextInfo>,
    /// Sibling features (same parent, excluding self).
    pub siblings: Vec<FeatureSummaryContextInfo>,
    /// Direct children of this feature.
    pub children: Vec<FeatureSummaryContextInfo>,
    /// Breadcrumb trail from root to this feature.
    pub breadcrumb: Vec<BreadcrumbItemInfo>,
}

/// Info about a project's default spec template.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SpecTemplateInfo {
    /// Template name.
    pub name: String,
    /// Template content (markdown).
    pub content: String,
}

/// Advisory context cost estimate for orchestrators and agents.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ContextEstimate {
    /// Estimated tokens for the feature spec + breadcrumb context.
    pub spec_tokens: u64,
    /// Number of files in the feature's project directories.
    pub file_count: u64,
    /// Estimated tokens for relevant history entries.
    pub history_tokens: u64,
    /// Sum estimate for spec + typical implementation session.
    pub total_estimate: u64,
    /// Advisory recommendation: "continue" or "fresh_session".
    pub recommendation: String,
}

/// Response for start_feature MCP tool (serialized as YAML).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct StartFeatureResponse {
    /// The feature with hierarchical context.
    #[serde(flatten)]
    pub feature: FeatureInfoWithContext,
    /// Spec completeness status.
    pub spec_status: String,
    /// Feature tier (root, feature_set, leaf).
    pub feature_tier: String,
    /// Number of testable criteria detected in the spec.
    pub testable_criteria_count: usize,
    /// Default spec template for this project (if one exists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_template: Option<SpecTemplateInfo>,
    /// Advisory context cost estimate for orchestrators.
    pub context_estimate: ContextEstimate,
}

/// Response for get_next_feature MCP tool (serialized as YAML).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct NextFeatureResponse {
    /// The feature with hierarchical context.
    #[serde(flatten)]
    pub feature: FeatureInfoWithContext,
    /// Spec completeness status.
    pub spec_status: String,
    /// Feature tier (root, feature_set, leaf).
    pub feature_tier: String,
    /// Testing workflow guidance based on project policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub testing_guidance: Option<String>,
    /// Number of testable criteria detected in the spec.
    pub testable_criteria_count: usize,
    /// Guidance for improving spec (if needed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_guidance: Option<String>,
    /// Default spec template for this project (if one exists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec_template: Option<SpecTemplateInfo>,
}

/// Get feature response with optional history (serialized as YAML).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GetFeatureResponse {
    /// The feature with hierarchical context.
    #[serde(flatten)]
    pub feature: FeatureInfoWithContext,
    /// Feature tier: "project", "feature_set", or "leaf".
    pub feature_tier: String,
    /// Implementation history (past work summaries and commits).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<HistoryEntryInfo>>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectContextResponse {
    pub project: ProjectInfo,
    pub directory: DirectoryInfo,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectInfo {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    /// Project-wide instructions for AI agents (coding guidelines, conventions).
    pub instructions: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DirectoryInfo {
    pub id: Uuid,
    pub path: String,
    pub git_remote: Option<String>,
    pub is_primary: bool,
    /// Directory-specific instructions (build commands, test commands).
    pub instructions: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PlanFeaturesResponse {
    /// The proposed feature tree. Review before confirming.
    pub proposed_features: Vec<ProposedFeature>,
    /// Whether the features were created (true if confirm=true was passed)
    pub created: bool,
    /// IDs of created features (only populated if created=true)
    #[serde(default)]
    pub created_feature_ids: Vec<Uuid>,
}

// ============================================================
// Version Response Types
// ============================================================

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct VersionListResponse {
    pub versions: Vec<VersionInfo>,
    /// ID of the first unreleased version (next to ship)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<Uuid>,
    /// Number of features in the backlog (no version assigned)
    #[serde(default)]
    pub backlog_count: u32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct VersionInfo {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// When this version was released, or null if unreleased
    #[serde(skip_serializing_if = "Option::is_none")]
    pub released_at: Option<String>,
    /// Number of features targeting this version
    pub feature_count: u32,
    /// Version status: "next" (first unreleased, next to ship), "planned" (other unreleased), "released" (shipped)
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProposedFeature {
    /// Short capability name (2-5 words). What users can DO.
    pub title: String,
    /// Feature details — content depends on position in hierarchy.
    /// For parent features (those with children): shared architectural context, patterns,
    /// constraints that apply to all children.
    /// For leaf features (no children): specification — goal, constraints, key interfaces.
    /// Use the project's spec template if available. Parents provide context; leaves provide specifications.
    /// IMPORTANT: Do not repeat the feature title in the details — the title is displayed separately.
    #[serde(default)]
    pub details: Option<String>,
    /// Priority for ordering. Lower values = implement first.
    #[serde(default)]
    pub priority: i32,
    /// Initial state for this feature. Valid values: 'proposed' (default), 'implemented'.
    /// Use 'implemented' when bootstrapping existing projects — features detected from the
    /// codebase can be created directly in the implemented state, skipping the prove/complete cycle.
    #[serde(default)]
    pub state: Option<String>,
    /// Child features (for hierarchical structure)
    #[serde(default)]
    pub children: Vec<ProposedFeature>,
}

fn default_max_depth() -> u32 {
    0
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RenderFeatureTreeRequest {
    #[schemars(description = "The UUID of the project to render the feature tree for")]
    pub project_id: Uuid,
    #[schemars(
        description = "Maximum depth of the tree to render. Default is 0 (unlimited). Set to a positive number to limit depth."
    )]
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetNextFeatureRequest {
    #[schemars(description = "The UUID of the project to get the next feature for")]
    pub project_id: Uuid,
    #[schemars(
        description = "Optional version ID to filter features. If not provided, prioritizes the 'next' version (first unreleased)."
    )]
    pub version_id: Option<Uuid>,
}

// ============================================================
// Verification Request Types
// ============================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VerifyFeatureRequest {
    #[schemars(description = "Feature ID (UUID, display ID like 'MAN-42', or UUID prefix)")]
    pub feature_id: String,
    #[schemars(
        description = "Optional git commit range to diff, e.g. 'abc123..HEAD'. If not provided, the tool attempts to get the uncommitted diff via git."
    )]
    #[serde(default)]
    pub commit_range: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RecordVerificationRequest {
    #[schemars(description = "Feature ID (UUID, display ID like 'MAN-42', or UUID prefix)")]
    pub feature_id: String,
    #[schemars(
        description = "Verification comments from your analysis. Pass an empty array if the implementation fully satisfies the spec."
    )]
    pub comments: Vec<VerificationCommentInput>,
}

/// A single verification comment from the agent's analysis.
#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
pub struct VerificationCommentInput {
    #[schemars(
        description = "Severity level: 'critical' (core requirement missing), 'major' (significant gap), 'minor' (style or polish drift)"
    )]
    pub severity: String,
    #[schemars(description = "One-line summary of the gap")]
    pub title: String,
    #[schemars(description = "Actionable explanation with suggested fix")]
    pub body: String,
    #[schemars(description = "Affected file path if the gap is localized to a single file")]
    #[serde(default)]
    pub file: Option<String>,
}

// ============================================================
// Orient Request Type
// ============================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OrientRequest {
    #[schemars(
        description = "Project UUID. If omitted, auto-detects from the agent's working directory."
    )]
    #[serde(default)]
    pub project_id: Option<Uuid>,
    #[schemars(
        description = "Working directory path for auto-detection when project_id is omitted."
    )]
    #[serde(default)]
    pub directory_path: Option<String>,
    #[schemars(
        description = "Maximum depth for the feature tree. Default 2. Set 0 for unlimited."
    )]
    #[serde(default = "default_orient_depth")]
    pub max_depth: u32,
    #[schemars(description = "Include recent completion history. Default true.")]
    #[serde(default = "default_true")]
    pub include_history: bool,
}

fn default_orient_depth() -> u32 {
    2
}

// ============================================================
// Orient Response Types
// ============================================================

/// Section of the orient response for active sessions (features being worked on).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ActiveSessionInfo {
    pub feature_title: String,
    pub agent_type: String,
    pub claimed_at: String,
}

/// Section of the orient response for the work queue.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct WorkQueueItem {
    pub id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_id: Option<String>,
    pub title: String,
    pub priority: i32,
}

/// Section of the orient response for recent history.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RecentHistoryItem {
    pub feature_title: String,
    pub summary_headline: String,
    pub completed_at: String,
}

// ============================================================
// Sync Request Type
// ============================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SyncRequest {
    #[schemars(description = "The UUID of the project to sync")]
    pub project_id: Uuid,
    #[schemars(
        description = "Git ref to start analysis from (tag, commit SHA, or branch). If not provided, analyzes the last 200 commits."
    )]
    #[serde(default)]
    pub since: Option<String>,
}

// ============================================================
// Version Request Types
// ============================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListVersionsRequest {
    #[schemars(description = "The UUID of the project to list versions for")]
    pub project_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateVersionRequest {
    #[schemars(description = "The UUID of the project")]
    pub project_id: Uuid,
    #[schemars(
        description = "Version name in MAJOR.MINOR.PATCH format (e.g., '0.2.0', '1.0.0'). Optional 'v' prefix (e.g., 'v0.2.0'). No freeform text—only semantic versions."
    )]
    pub name: String,
    #[schemars(description = "Optional description of what this version includes")]
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetFeatureVersionRequest {
    #[schemars(description = "Feature ID (UUID, display ID like 'MAN-42', or UUID prefix)")]
    pub feature_id: String,
    #[schemars(
        description = "The UUID of the target version (must be unreleased), or null to unassign"
    )]
    pub version_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReleaseVersionRequest {
    #[schemars(description = "The UUID of the version to release")]
    pub version_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetActiveFeatureRequest {
    #[schemars(description = "The UUID of the project to check focus for")]
    pub project_id: Uuid,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ActiveFeatureResponse {
    /// The focused feature ID, or null if no feature is focused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_id: Option<Uuid>,
    /// The focused feature title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_title: Option<String>,
    /// The focused feature state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feature_state: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFeatureProofRequest {
    #[schemars(description = "Feature ID (UUID, display ID like 'MAN-42', or UUID prefix)")]
    pub feature_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetProjectHistoryRequest {
    #[schemars(description = "The UUID of the project to get activity for")]
    pub project_id: Uuid,
    #[schemars(
        description = "Optional feature ID to filter history to a feature and its descendants."
    )]
    #[serde(default)]
    pub feature_id: Option<String>,
    #[schemars(description = "Max entries to return (default 20).")]
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetProjectInstructionsRequest {
    #[schemars(description = "The UUID of the project to get full instructions for")]
    pub project_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListProjectsRequest {
    #[schemars(
        description = "Optional directory path to filter by. If provided, returns only the project containing this directory. If the directory is not linked to any project, returns an empty list with a hint to use init_project."
    )]
    #[serde(default)]
    pub directory_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectListResponse {
    pub projects: Vec<ProjectInfo>,
    /// Hint message when directory_path filter finds no project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct InitProjectRequest {
    #[schemars(
        description = "Absolute path to the directory to initialize as a project. This will be analyzed and associated with the project."
    )]
    pub directory_path: String,
    #[schemars(
        description = "Optional: existing project name or UUID to link this directory to. If not provided, a new project is created with a name derived from the directory analysis."
    )]
    pub project: Option<String>,
    #[schemars(
        description = "Include documentation content (README, CLAUDE.md). Defaults to true."
    )]
    #[serde(default = "default_true")]
    pub include_docs: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GenerateFeatureTreeRequest {
    #[schemars(
        description = "Absolute path to the directory to analyze (must be a git repository)"
    )]
    pub directory_path: String,
    #[schemars(
        description = "Only analyze commits since this tag or commit SHA (optional). Example: 'v1.0.0' or 'abc1234'"
    )]
    #[serde(default)]
    pub since: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GenerateFeatureTreeResponse {
    /// The generated markdown document describing the feature tree.
    pub document: String,
    /// Summary statistics about the extraction.
    pub stats: GenerateFeatureTreeStats,
    /// Warnings encountered during analysis.
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GenerateFeatureTreeStats {
    /// Total number of top-level chapters.
    pub total_chapters: u32,
    /// Total number of features extracted.
    pub total_features: u32,
    /// Features marked as implemented.
    pub implemented_count: u32,
    /// Features marked as proposed.
    pub proposed_count: u32,
    /// Features marked as deprecated (removed).
    pub deprecated_count: u32,
    /// Number of git commits analyzed.
    pub commits_analyzed: u32,
}

// ============================================================
// Project Analysis (for AI feature planning)
// ============================================================

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectAnalysis {
    /// Detected project name (from package.json/Cargo.toml)
    pub name: Option<String>,
    /// Detected project description
    pub description: Option<String>,
    /// Detected project type (language, frameworks, build tool)
    pub project_type: ProjectType,
    /// Git remote URL if available
    pub git_remote: Option<String>,
    /// Significant directories in the project
    pub directories: Vec<DirectorySignal>,
    /// Detected modules/components
    pub modules: Vec<ModuleSignal>,
    /// Documentation content (README, CLAUDE.md)
    pub documentation: Option<DocumentationContent>,
    /// AI-friendly hints for feature suggestions
    pub hints: Vec<FeatureHint>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectType {
    /// Primary language (rust, typescript, fsharp, python, go, etc.)
    pub language: String,
    /// Detected frameworks (axum, sveltekit, react, etc.)
    pub frameworks: Vec<String>,
    /// Build tool (cargo, pnpm, npm, dotnet, etc.)
    pub build_tool: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DirectorySignal {
    /// Path relative to project root
    pub path: String,
    /// Kind of directory (source, tests, docs, config, unknown)
    pub kind: String,
    /// Number of files in this directory (non-recursive)
    pub file_count: u32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ModuleSignal {
    /// Module name
    pub name: String,
    /// Path relative to project root
    pub path: String,
    /// Whether this appears to be a major module (by file count or naming)
    pub is_major: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DocumentationContent {
    /// README content (truncated to ~500 lines)
    pub readme: Option<String>,
    /// CLAUDE.md content if present
    pub claude_md: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureHint {
    /// Suggested feature name
    pub title: String,
    /// Why this was detected
    pub reason: String,
    /// Related paths in the project
    pub paths: Vec<String>,
}

// ============================================================
// Type Conversions (models → MCP types)
// ============================================================

impl From<&Feature> for FeatureInfo {
    fn from(f: &Feature) -> Self {
        Self {
            id: f.id.into(),
            title: f.title.clone(),
            details: f.details.clone(),
            desired_details: f.desired_details.clone(),
            state: f.state.as_str().to_string(),
            priority: f.priority,
            target_version_id: f.target_version_id.map(Into::into),
        }
    }
}

impl From<&FeatureSummaryContext> for FeatureSummaryContextInfo {
    fn from(f: &FeatureSummaryContext) -> Self {
        Self {
            id: f.id.into(),
            display_id: None, // Populated by tool handlers with project key_prefix
            title: f.title.clone(),
            state: f.state.as_str().to_string(),
        }
    }
}

impl From<&BreadcrumbItem> for BreadcrumbItemInfo {
    fn from(b: &BreadcrumbItem) -> Self {
        Self {
            id: b.id.into(),
            display_id: None, // Populated by tool handlers with project key_prefix
            title: b.title.clone(),
            details: b.details.clone(),
        }
    }
}

impl From<&FeatureWithContext> for FeatureInfoWithContext {
    fn from(ctx: &FeatureWithContext) -> Self {
        Self {
            id: ctx.feature.id.into(),
            display_id: None, // Populated by tool handlers with project key_prefix
            title: ctx.feature.title.clone(),
            details: ctx.feature.details.clone(),
            desired_details: ctx.feature.desired_details.clone(),
            state: ctx.feature.state.as_str().to_string(),
            priority: ctx.feature.priority,
            target_version_id: ctx.feature.target_version_id.map(Into::into),
            parent: ctx.parent.as_ref().map(Into::into),
            siblings: ctx.siblings.iter().map(Into::into).collect(),
            children: ctx.children.iter().map(Into::into).collect(),
            breadcrumb: ctx.breadcrumb.iter().map(Into::into).collect(),
        }
    }
}
