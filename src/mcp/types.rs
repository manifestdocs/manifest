//! Request and response types for MCP tools.

use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ============================================================
// Request Types
// ============================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetTaskContextRequest {
    #[schemars(description = "The UUID of the task assigned to you")]
    pub task_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartTaskRequest {
    #[schemars(description = "The UUID of the task to start working on")]
    pub task_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteTaskRequest {
    #[schemars(description = "The UUID of the task to mark as complete")]
    pub task_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateSessionRequest {
    #[schemars(
        description = "The UUID of the feature to start a session on (must be a leaf feature with no children)"
    )]
    pub feature_id: String,
    #[schemars(
        description = "The goal of this session - what will be accomplished when the session ends"
    )]
    pub goal: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateTaskRequest {
    #[schemars(description = "The UUID of the session to create the task in")]
    pub session_id: String,
    #[schemars(description = "Short title describing what this task accomplishes")]
    pub title: String,
    #[schemars(
        description = "Detailed scope of work - be specific about what to implement, test, or verify"
    )]
    pub scope: String,
    #[schemars(
        description = "Which agent type should handle this task: 'claude', 'gemini', or 'codex'"
    )]
    pub agent_type: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListSessionTasksRequest {
    #[schemars(description = "The UUID of the session to list tasks for")]
    pub session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFeatureSessionRequest {
    #[schemars(description = "The UUID of the feature to get the active session for")]
    pub feature_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteSessionRequest {
    #[schemars(description = "The UUID of the session to complete")]
    pub session_id: String,
    #[schemars(
        description = "Summary of work done (git-style format). First line is a concise headline shown in list views. Add details after a blank line if needed."
    )]
    pub summary: String,
    #[schemars(description = "Git commits created during this session")]
    #[serde(default)]
    pub commits: Vec<CommitRefInput>,
    #[schemars(
        description = "Whether to mark the feature as 'implemented'. Defaults to true. Set to false if work is partial or feature needs more sessions."
    )]
    #[serde(default = "default_true")]
    pub mark_implemented: bool,
}

/// A reference to a git commit for MCP input.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CommitRefInput {
    #[schemars(description = "The commit SHA (short or full)")]
    pub sha: String,
    #[schemars(description = "The commit message (first line)")]
    pub message: String,
    #[schemars(description = "The commit author")]
    #[serde(default)]
    pub author: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListFeaturesRequest {
    #[schemars(description = "Optional project UUID to filter features by project")]
    pub project_id: Option<String>,
    #[schemars(
        description = "Optional state filter: 'proposed', 'specified', 'implemented', or 'deprecated'"
    )]
    pub state: Option<String>,
    #[schemars(description = "Maximum number of features to return. Defaults to no limit.")]
    pub limit: Option<u32>,
    #[schemars(description = "Number of features to skip for pagination. Defaults to 0.")]
    pub offset: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchFeaturesRequest {
    #[schemars(description = "Search term to match against title and details")]
    pub query: String,
    #[schemars(description = "Optional project UUID to limit search to a specific project")]
    pub project_id: Option<String>,
    #[schemars(description = "Maximum number of results to return. Defaults to 10.")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFeatureRequest {
    #[schemars(description = "The UUID of the feature to retrieve")]
    pub feature_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFeatureHistoryRequest {
    #[schemars(description = "The UUID of the feature to get history for")]
    pub feature_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetProjectContextRequest {
    #[schemars(
        description = "The directory path to look up (e.g., current working directory). Returns the project that contains this directory."
    )]
    pub directory_path: String,
}

/// Request to get the active feature from the Manifest desktop app.
/// This is a parameterless request - the active feature is determined by
/// the desktop app's current selection, stored in ~/.manifest/active_context.json.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetActiveFeatureRequest {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateFeatureStateRequest {
    #[schemars(description = "The UUID of the feature to update")]
    pub feature_id: String,
    #[schemars(
        description = "The new state: 'proposed', 'specified', 'implemented', or 'deprecated'"
    )]
    #[serde(default)]
    pub state: Option<String>,
    #[schemars(description = "New title for the feature")]
    #[serde(default)]
    pub title: Option<String>,
    #[schemars(
        description = "New details for the feature. Use this to update the living documentation when implementation reveals new information."
    )]
    #[serde(default)]
    pub details: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateProjectRequest {
    #[schemars(description = "The project name (e.g., 'RocketShip', 'MyApp')")]
    pub name: String,
    #[schemars(description = "Optional description of the project")]
    #[serde(default)]
    pub description: Option<String>,
    #[schemars(
        description = "Optional project-wide instructions for AI agents (coding guidelines, conventions)"
    )]
    #[serde(default)]
    pub instructions: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddProjectDirectoryRequest {
    #[schemars(description = "The UUID of the project to add this directory to")]
    pub project_id: String,
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
    pub project_id: String,
    #[schemars(description = "Optional parent feature UUID for hierarchical features")]
    #[serde(default)]
    pub parent_id: Option<String>,
    #[schemars(description = "Short title for the feature (e.g., 'User Authentication')")]
    pub title: String,
    #[schemars(
        description = "Optional feature details including user stories, implementation notes, and technical context"
    )]
    #[serde(default)]
    pub details: Option<String>,
    #[schemars(
        description = "Initial state: 'proposed' (default), 'specified', 'implemented', or 'deprecated'"
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
    pub project_id: String,
    #[schemars(
        description = "The proposed feature tree. Apply the user story test before proposing: 'As a [user], I can [feature]...'"
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
pub struct TaskContextResponse {
    /// The task you are assigned to complete
    pub task: TaskInfo,
    /// The feature this task implements
    pub feature: FeatureInfo,
    /// The session goal describing the overall objective
    pub session_goal: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TaskInfo {
    pub id: String,
    pub title: String,
    pub scope: String,
    pub status: String,
    pub agent_type: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInfo {
    pub id: String,
    pub title: String,
    /// Feature details including user stories, implementation notes, and technical context.
    pub details: Option<String>,
    /// Desired details for pending changes. When non-null, indicates edits awaiting implementation.
    pub desired_details: Option<String>,
    pub state: String,
    /// Priority for ordering within parent. Lower values appear first.
    pub priority: i32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SessionInfo {
    pub id: String,
    pub feature_id: String,
    pub goal: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TaskListResponse {
    pub session_id: String,
    pub tasks: Vec<TaskInfo>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureSessionResponse {
    pub feature_id: String,
    /// The active session for this feature, or null if no active session exists
    pub session: Option<SessionInfo>,
    /// Message explaining the result
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CompleteSessionResponse {
    pub session_id: String,
    pub feature_id: String,
    pub feature_state: String,
    pub history_entry_id: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureListResponse {
    pub features: Vec<FeatureInfo>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureHistoryResponse {
    pub feature_id: String,
    pub entries: Vec<HistoryEntryInfo>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct HistoryEntryInfo {
    pub id: String,
    /// The version this work was done for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
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
/// Uses string IDs to match MCP convention.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureSummaryInfo {
    pub id: String,
    pub title: String,
    pub state: String,
    pub priority: i32,
    pub parent_id: Option<String>,
}

/// Response for list_features in summary mode (default).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureListSummaryResponse {
    pub features: Vec<FeatureSummaryInfo>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectContextResponse {
    pub project: ProjectInfo,
    pub directory: DirectoryInfo,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// Project-wide instructions for AI agents (coding guidelines, conventions).
    pub instructions: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DirectoryInfo {
    pub id: String,
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
    pub created_feature_ids: Vec<String>,
}

// ============================================================
// Version Response Types
// ============================================================

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct VersionListResponse {
    pub versions: Vec<VersionInfo>,
    /// ID of the first unreleased version (current focus)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now: Option<String>,
    /// ID of the second unreleased version (queued up)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct VersionInfo {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// When this version was released, or null if unreleased
    #[serde(skip_serializing_if = "Option::is_none")]
    pub released_at: Option<String>,
    /// Number of features targeting this version
    pub feature_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProposedFeature {
    /// Short capability name (2-5 words). What users can DO.
    pub title: String,
    /// Feature details: user story, technical notes, constraints, acceptance criteria.
    /// User stories can be in "As a \[user\], I can \[capability\] so that \[benefit\]" format.
    #[serde(default)]
    pub details: Option<String>,
    /// Priority for ordering. Lower values = implement first.
    #[serde(default)]
    pub priority: i32,
    /// Child features (for hierarchical structure)
    #[serde(default)]
    pub children: Vec<ProposedFeature>,
}

// ============================================================
// Breakdown Feature (Session + Tasks in one call)
// ============================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BreakdownFeatureRequest {
    #[schemars(description = "The UUID of the feature to break down into tasks")]
    pub feature_id: String,
    #[schemars(
        description = "The session goal - what will be accomplished when all tasks are complete"
    )]
    pub goal: String,
    #[schemars(
        description = "The tasks to create. Each task should be completable by one agent (1-3 story points)."
    )]
    pub tasks: Vec<TaskInputItem>,
}

/// A task to create as part of feature breakdown.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct TaskInputItem {
    #[schemars(description = "Short title describing what this task accomplishes (2-5 words)")]
    pub title: String,
    #[schemars(
        description = "Detailed scope of work - be specific about what to implement, test, or verify"
    )]
    pub scope: String,
    #[schemars(
        description = "Which agent type should handle this task: 'claude', 'gemini', or 'codex'. Defaults to 'claude'."
    )]
    #[serde(default = "default_claude")]
    pub agent_type: String,
}

fn default_claude() -> String {
    "claude".to_string()
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct BreakdownFeatureResponse {
    /// The created session
    pub session: SessionInfo,
    /// The created tasks, ready for agent assignment
    pub tasks: Vec<TaskInfo>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RenderFeatureTreeRequest {
    #[schemars(description = "The UUID of the project to render the feature tree for")]
    pub project_id: String,
}

// ============================================================
// Version Request Types
// ============================================================

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListVersionsRequest {
    #[schemars(description = "The UUID of the project to list versions for")]
    pub project_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateVersionRequest {
    #[schemars(description = "The UUID of the project")]
    pub project_id: String,
    #[schemars(description = "Version name (e.g., 'v0.2', '2024.1', 'MVP')")]
    pub name: String,
    #[schemars(description = "Optional description of what this version includes")]
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetFeatureVersionRequest {
    #[schemars(description = "The UUID of the feature to update")]
    pub feature_id: String,
    #[schemars(description = "The UUID of the target version, or null to unassign")]
    pub version_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReleaseVersionRequest {
    #[schemars(description = "The UUID of the version to release")]
    pub version_id: String,
}
