use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use validator::Validate;

use super::{DirectoryId, FeatureId, ProjectId, VersionId};

/// A project containing features.
///
/// Projects are the top-level organizational unit. Each project can have
/// multiple associated directories (e.g., frontend/backend repos) and
/// contains a tree of features describing its capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    /// URL-friendly identifier (e.g., "manifest", "rocketship"). Unique across all projects.
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    /// Project-wide instructions for AI agents (coding guidelines, conventions, etc.).
    pub instructions: Option<String>,
    /// The current/active version for this project (explicitly set by user).
    pub current_version_id: Option<VersionId>,
    /// The root feature for this project. All other features are descendants of this.
    /// The root feature's title = project name, details = project documentation,
    /// and its history tracks project-level events (version releases, etc.).
    pub root_feature_id: Option<FeatureId>,
    /// Where new features go by default: "backlog" (NULL version) or "next" (first unreleased).
    pub default_feature_destination: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A file system directory associated with a project.
///
/// Projects can span multiple directories (e.g., separate repos for frontend
/// and backend). One directory is marked as primary for default operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDirectory {
    pub id: DirectoryId,
    pub project_id: ProjectId,
    /// Absolute path to the directory on the local file system.
    pub path: String,
    /// Git remote URL (e.g., `git@github.com:org/repo.git`).
    pub git_remote: Option<String>,
    /// Whether this is the primary directory for the project.
    pub is_primary: bool,
    /// Directory-specific instructions for AI agents (build commands, test commands, etc.).
    pub instructions: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Input for creating a new project.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateProjectInput {
    /// Display name for the project.
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    /// URL-friendly identifier. If not provided, auto-generated from name.
    #[validate(length(max = 200))]
    pub slug: Option<String>,
    /// Brief description of the project's purpose.
    #[validate(length(max = 10_000))]
    pub description: Option<String>,
    /// Project-wide instructions for AI agents (coding guidelines, conventions, etc.).
    #[validate(length(max = 50_000))]
    pub instructions: Option<String>,
}

/// Input for updating an existing project. All fields are optional for partial updates.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct UpdateProjectInput {
    /// New display name for the project.
    #[validate(length(min = 1, max = 200))]
    pub name: Option<String>,
    /// URL-friendly identifier. Must be unique.
    #[validate(length(max = 200))]
    pub slug: Option<String>,
    /// Brief description of the project's purpose.
    #[validate(length(max = 10_000))]
    pub description: Option<String>,
    /// Project-wide instructions for AI agents (coding guidelines, conventions, etc.).
    #[validate(length(max = 50_000))]
    pub instructions: Option<String>,
    /// Set the current/active version for this project.
    pub current_version_id: Option<VersionId>,
    /// Where new features go by default: "backlog" or "next".
    pub default_feature_destination: Option<String>,
}

/// Input for adding a directory to a project.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct AddDirectoryInput {
    /// Absolute path to the directory on the local file system.
    #[validate(length(min = 1, max = 4_096))]
    pub path: String,
    /// Git remote URL (e.g., `git@github.com:org/repo.git`).
    #[validate(length(max = 1_000))]
    pub git_remote: Option<String>,
    /// Whether this is the primary directory for the project. Defaults to false.
    #[serde(default)]
    pub is_primary: bool,
    /// Directory-specific instructions for AI agents (build commands, test commands, etc.).
    #[validate(length(max = 10_000))]
    pub instructions: Option<String>,
}

/// Input for updating an existing directory. All fields are optional for partial updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateDirectoryInput {
    /// Absolute path to the directory on the local file system.
    pub path: Option<String>,
    /// Git remote URL (e.g., `git@github.com:org/repo.git`).
    pub git_remote: Option<String>,
    /// Whether this is the primary directory for the project.
    pub is_primary: Option<bool>,
    /// Directory-specific instructions for AI agents (build commands, test commands, etc.).
    pub instructions: Option<String>,
}

/// A project with its associated directories, used for detailed responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectWithDirectories {
    #[serde(flatten)]
    pub project: Project,
    pub directories: Vec<ProjectDirectory>,
}
