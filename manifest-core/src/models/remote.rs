use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use validator::Validate;

use super::{ProjectId, RemoteId};

/// A configured remote backend (e.g., a Turso database).
///
/// Remotes are local-only configuration — they are never synced to a shared
/// backend. Each developer's machine maintains its own set of remote connections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Remote {
    pub id: RemoteId,
    /// Human-readable name (e.g., "adhoc", "personal"). Unique per machine.
    pub name: String,
    /// Backend provider type (e.g., "turso").
    pub provider: String,
    /// Connection URL (e.g., `libsql://mydb.turso.io`).
    pub url: String,
    /// Whether sync is enabled for this remote.
    pub sync_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Sync state for a project-remote binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    /// Actively syncing.
    Active,
    /// Sync paused by the user.
    Paused,
    /// Remote was removed; link is orphaned.
    Orphaned,
}

impl SyncState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Orphaned => "orphaned",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "orphaned" => Some(Self::Orphaned),
            _ => None,
        }
    }
}

/// A binding between a project and a remote backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectRemote {
    pub project_id: ProjectId,
    pub remote_id: RemoteId,
    pub sync_state: SyncState,
    pub last_synced_at: Option<DateTime<Utc>>,
}

/// Input for creating a new remote.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateRemoteInput {
    /// Human-readable name (unique per machine).
    #[validate(length(min = 1, max = 100))]
    pub name: String,
    /// Backend provider (defaults to "turso").
    #[validate(length(min = 1, max = 50))]
    pub provider: Option<String>,
    /// Connection URL.
    #[validate(length(min = 1, max = 2000))]
    pub url: String,
    /// Auth token (will be stored encrypted).
    #[validate(length(min = 1, max = 10_000))]
    pub token: String,
}

/// Input for updating an existing remote.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Validate)]
pub struct UpdateRemoteInput {
    /// New connection URL.
    #[validate(length(min = 1, max = 2000))]
    pub url: Option<String>,
    /// New auth token (will be stored encrypted).
    #[validate(length(min = 1, max = 10_000))]
    pub token: Option<String>,
    /// Enable or disable sync.
    pub sync_enabled: Option<bool>,
}
