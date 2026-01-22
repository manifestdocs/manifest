//! Request and response types for MCP tools.

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
    pub feature_id: Uuid,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteFeatureRequest {
    #[schemars(description = "The UUID of the feature to complete")]
    pub feature_id: Uuid,
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindFeaturesRequest {
    #[schemars(description = "Optional project UUID to filter features by project")]
    pub project_id: Option<Uuid>,
    #[schemars(
        description = "Optional state filter: 'proposed', 'in_progress', 'implemented', or 'archived'"
    )]
    pub state: Option<String>,
    #[schemars(
        description = "Optional search query to match against title and details. When provided, returns features ranked by relevance."
    )]
    pub query: Option<String>,
    #[schemars(description = "Maximum number of features to return. Defaults to no limit.")]
    pub limit: Option<u32>,
    #[schemars(description = "Number of features to skip for pagination. Defaults to 0.")]
    pub offset: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetFeatureRequest {
    #[schemars(description = "The UUID of the feature to retrieve")]
    pub feature_id: Uuid,
    #[schemars(
        description = "Include implementation history (past work summaries and commits). Defaults to false."
    )]
    #[serde(default)]
    pub include_history: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateFeatureStateRequest {
    #[schemars(description = "The UUID of the feature to update")]
    pub feature_id: Uuid,
    #[schemars(
        description = "The new state: 'proposed', 'in_progress', 'implemented', or 'archived'"
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
    #[schemars(description = "Optional parent feature UUID for hierarchical features")]
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[schemars(description = "Short title for the feature (e.g., 'User Authentication')")]
    pub title: String,
    #[schemars(
        description = "Optional feature details including user stories, implementation notes, and technical context"
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
pub struct FeatureListResponse {
    pub features: Vec<FeatureInfo>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureHistoryResponse {
    pub feature_id: Uuid,
    pub entries: Vec<HistoryEntryInfo>,
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
    pub title: String,
    pub state: String,
}

/// Breadcrumb item for navigation path (root → feature).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct BreadcrumbItemInfo {
    pub id: Uuid,
    pub title: String,
}

/// A feature with its hierarchical context (parent, siblings, children, breadcrumb).
/// Used by get_feature MCP tool to provide navigation context.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct FeatureInfoWithContext {
    /// The feature itself with all details.
    pub id: Uuid,
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
    /// ID of the first unreleased version (current focus)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now: Option<Uuid>,
    /// ID of the second unreleased version (queued up)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<Uuid>,
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

fn default_max_depth() -> u32 {
    3
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RenderFeatureTreeRequest {
    #[schemars(description = "The UUID of the project to render the feature tree for")]
    pub project_id: Uuid,
    #[schemars(
        description = "Maximum depth of the tree to render. Default is 3. Use 0 for unlimited depth."
    )]
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetNextFeatureRequest {
    #[schemars(description = "The UUID of the project to get the next feature for")]
    pub project_id: Uuid,
    #[schemars(
        description = "Optional version ID to filter features. If not provided, prioritizes the 'now' version (first unreleased)."
    )]
    pub version_id: Option<Uuid>,
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
    #[schemars(description = "Version name (e.g., 'v0.2', '2024.1', 'MVP')")]
    pub name: String,
    #[schemars(description = "Optional description of what this version includes")]
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SetFeatureVersionRequest {
    #[schemars(description = "The UUID of the feature to update")]
    pub feature_id: Uuid,
    #[schemars(description = "The UUID of the target version, or null to unassign")]
    pub version_id: Option<Uuid>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReleaseVersionRequest {
    #[schemars(description = "The UUID of the version to release")]
    pub version_id: Uuid,
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
            id: f.id,
            title: f.title.clone(),
            details: f.details.clone(),
            desired_details: f.desired_details.clone(),
            state: f.state.as_str().to_string(),
            priority: f.priority,
            target_version_id: f.target_version_id,
        }
    }
}

impl From<&FeatureSummaryContext> for FeatureSummaryContextInfo {
    fn from(f: &FeatureSummaryContext) -> Self {
        Self {
            id: f.id,
            title: f.title.clone(),
            state: f.state.as_str().to_string(),
        }
    }
}

impl From<&BreadcrumbItem> for BreadcrumbItemInfo {
    fn from(b: &BreadcrumbItem) -> Self {
        Self {
            id: b.id,
            title: b.title.clone(),
        }
    }
}

impl From<&FeatureWithContext> for FeatureInfoWithContext {
    fn from(ctx: &FeatureWithContext) -> Self {
        Self {
            id: ctx.feature.id,
            title: ctx.feature.title.clone(),
            details: ctx.feature.details.clone(),
            desired_details: ctx.feature.desired_details.clone(),
            state: ctx.feature.state.as_str().to_string(),
            priority: ctx.feature.priority,
            target_version_id: ctx.feature.target_version_id,
            parent: ctx.parent.as_ref().map(Into::into),
            siblings: ctx.siblings.iter().map(Into::into).collect(),
            children: ctx.children.iter().map(Into::into).collect(),
            breadcrumb: ctx.breadcrumb.iter().map(Into::into).collect(),
        }
    }
}
