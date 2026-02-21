use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{FeatureId, ProjectId, VersionId};

/// A minimal feature reference used in portfolio lanes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioFeatureRef {
    pub id: FeatureId,
    pub title: String,
}

/// Version progress summary for the portfolio lane header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioVersionSummary {
    pub id: VersionId,
    pub name: String,
    /// Total leaf features assigned to this version.
    pub feature_count: i64,
    /// Leaf features in this version with state `implemented`.
    pub implemented_count: i64,
}

/// The single next actionable feature for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioNextFeature {
    pub id: FeatureId,
    pub title: String,
    /// Whether this feature is assigned to the next version (vs backlog).
    pub in_version: bool,
}

/// A recent feature completion for the activity section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioCompletion {
    pub id: FeatureId,
    pub title: String,
    pub completed_at: DateTime<Utc>,
}

/// Aggregated health snapshot for a single project in the portfolio view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioProject {
    pub id: ProjectId,
    pub name: String,
    pub slug: String,
    /// The next unreleased version, with leaf feature progress counts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_version: Option<PortfolioVersionSummary>,
    /// The highest-priority proposed leaf feature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_feature: Option<PortfolioNextFeature>,
    /// Up to 5 in-progress leaf features.
    pub in_progress: Vec<PortfolioFeatureRef>,
    /// Total count of in-progress leaf features (may exceed `in_progress.len()`).
    pub in_progress_total: i64,
    /// All blocked leaf features.
    pub blocked: Vec<PortfolioFeatureRef>,
    /// Total count of blocked leaf features.
    pub blocked_count: i64,
    /// Up to 5 features completed in the last 7 days.
    pub recent_completions: Vec<PortfolioCompletion>,
    /// Timestamp of the most recent completion (for stalled detection).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<DateTime<Utc>>,
}

/// The full portfolio response: health snapshots for all projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Portfolio {
    pub projects: Vec<PortfolioProject>,
}
