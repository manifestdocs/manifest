use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{FeatureId, HistoryId, VersionId};

/// An append-only log entry recording work done on a feature.
///
/// Feature history is like `git log` for a feature—it records what was done
/// during each implementation session. This is **not** version control for
/// the feature content itself (which is mutable); rather, it answers
/// "what work was done on this feature and when?"
///
/// History can be viewed chronologically (by date) or grouped by version
/// (for release notes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureHistory {
    pub id: HistoryId,
    pub feature_id: FeatureId,
    /// The version this work was done for (captured at time of completion).
    /// Enables grouping history by release version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<VersionId>,
    /// Structured details about the work done.
    pub details: HistoryDetails,
    pub created_at: DateTime<Utc>,
}

/// Structured details about work done in a history entry.
///
/// Stored as JSON to allow schema evolution without migrations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistoryDetails {
    /// Summary of the work done.
    pub summary: String,
    /// Git commits created during this work.
    #[serde(default)]
    pub commits: Vec<CommitRef>,
}

/// A reference to a git commit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitRef {
    /// The commit SHA (short or full).
    pub sha: String,
    /// The commit message (first line).
    pub message: String,
    /// The commit author, if different from the session author.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

/// Input for creating a history entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateHistoryInput {
    pub feature_id: FeatureId,
    /// The version this work was done for.
    /// If not specified, defaults to the feature's target_version_id.
    #[serde(default)]
    pub version_id: Option<VersionId>,
    pub details: HistoryDetails,
}

/// A project-wide history entry that includes feature context.
///
/// Used for the unified project history view, where PMs can see recent
/// changes across all features in a project. Can be filtered by version
/// to generate release notes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectHistoryEntry {
    pub id: HistoryId,
    pub feature_id: FeatureId,
    /// Feature title, denormalized for display.
    pub feature_title: String,
    /// Feature state at time of query.
    pub feature_state: super::FeatureState,
    /// The version this work was done for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<VersionId>,
    /// Version name, denormalized for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_name: Option<String>,
    /// Summary of the work done.
    pub summary: String,
    /// Git commits created during this work.
    #[serde(default)]
    pub commits: Vec<CommitRef>,
    pub created_at: DateTime<Utc>,
}
