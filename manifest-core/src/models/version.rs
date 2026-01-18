use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A version for release planning.
///
/// Versions are first-class entities that allow features to be grouped for
/// release planning. Features can target a specific version, and projects
/// track their current version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub id: Uuid,
    pub project_id: Uuid,
    /// Version name (e.g., "1.0.0", "2.0.0-beta")
    pub name: String,
    /// Optional description of the version
    pub description: Option<String>,
    /// When this version was released (if released)
    pub released_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for creating a new version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateVersionInput {
    pub name: String,
    pub description: Option<String>,
}

/// Input for updating an existing version. All fields are optional for partial updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateVersionInput {
    pub name: Option<String>,
    pub description: Option<String>,
    /// Set to mark the version as released
    pub released_at: Option<DateTime<Utc>>,
}
