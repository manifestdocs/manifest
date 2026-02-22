use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{FeatureId, ProjectId};

/// Unique identifier for a project memory entry.
pub type MemoryId = Uuid;

/// A project-scoped memory entry — facts, decisions, and architectural observations
/// that survive across sessions and agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemory {
    pub id: MemoryId,
    pub project_id: ProjectId,
    /// Plain text or markdown content of the memory.
    pub content: String,
    /// Optional tags for categorisation (e.g. ["auth", "sqlite"]).
    pub tags: Vec<String>,
    /// Optional link to the feature this memory originated from.
    pub source_feature_id: Option<FeatureId>,
    /// Who created this memory: "agent" or "human".
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for creating a new memory entry.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CreateMemoryInput {
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub source_feature_id: Option<FeatureId>,
    /// "agent" (default) or "human"
    #[serde(default = "default_created_by")]
    pub created_by: String,
}

fn default_created_by() -> String {
    "agent".to_string()
}

/// Query parameters for searching memories.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SearchMemoriesQuery {
    /// Search query — FTS5 full-text or LIKE fallback.
    pub q: Option<String>,
    /// Maximum results to return (default 10).
    pub limit: Option<u32>,
}
