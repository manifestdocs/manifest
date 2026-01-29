use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A project containing features.
///
/// Projects are the top-level organizational unit. Each project can have
/// multiple associated directories (e.g., frontend/backend repos) and
/// contains a tree of features describing its capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    /// URL-friendly identifier (e.g., "manifest", "rocketship"). Unique across all projects.
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    /// Project-wide instructions for AI agents (coding guidelines, conventions, etc.).
    pub instructions: Option<String>,
    /// The current/active version for this project (explicitly set by user).
    pub current_version_id: Option<Uuid>,
    /// The root feature for this project. All other features are descendants of this.
    /// The root feature's title = project name, details = project documentation,
    /// and its history tracks project-level events (version releases, etc.).
    pub root_feature_id: Option<Uuid>,
    /// Where new features go by default: "backlog" (NULL version) or "now" (first unreleased).
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
    pub id: Uuid,
    pub project_id: Uuid,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectInput {
    pub name: String,
    /// URL-friendly identifier. If not provided, auto-generated from name.
    pub slug: Option<String>,
    pub description: Option<String>,
    pub instructions: Option<String>,
}

/// Input for updating an existing project. All fields are optional for partial updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProjectInput {
    pub name: Option<String>,
    /// URL-friendly identifier. Must be unique.
    pub slug: Option<String>,
    pub description: Option<String>,
    pub instructions: Option<String>,
    /// Set the current/active version for this project.
    pub current_version_id: Option<Uuid>,
    /// Where new features go by default: "backlog" or "now".
    pub default_feature_destination: Option<String>,
}

/// Input for adding a directory to a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddDirectoryInput {
    pub path: String,
    pub git_remote: Option<String>,
    #[serde(default)]
    pub is_primary: bool,
    pub instructions: Option<String>,
}

/// Input for updating an existing directory. All fields are optional for partial updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateDirectoryInput {
    pub path: Option<String>,
    pub git_remote: Option<String>,
    pub is_primary: Option<bool>,
    pub instructions: Option<String>,
}

/// A project with its associated directories, used for detailed responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectWithDirectories {
    #[serde(flatten)]
    pub project: Project,
    pub directories: Vec<ProjectDirectory>,
}
