use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Serde helper for Option<Option<T>> - distinguishes "field absent" from "field is null".
/// - JSON field absent → None (don't update)
/// - JSON field is null → Some(None) (set to null)
/// - JSON field has value → Some(Some(value)) (set to value)
mod double_option {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
    where
        T: Deserialize<'de>,
        D: Deserializer<'de>,
    {
        // If this function is called, the field was present in JSON
        // Deserialize the value (which may be null)
        Ok(Some(Option::deserialize(deserializer)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_feature_input_deserialize_with_version() {
        let json = r#"{"target_version_id": "fb5e9bc0-6202-4617-92f0-3eb6d943bc4f"}"#;
        let input: UpdateFeatureInput = serde_json::from_str(json).unwrap();
        assert!(input.target_version_id.is_some());
        assert!(input.target_version_id.unwrap().is_some());
    }

    #[test]
    fn test_update_feature_input_deserialize_with_null() {
        let json = r#"{"target_version_id": null}"#;
        let input: UpdateFeatureInput = serde_json::from_str(json).unwrap();
        assert!(input.target_version_id.is_some());
        assert!(input.target_version_id.unwrap().is_none());
    }

    #[test]
    fn test_update_feature_input_deserialize_without_field() {
        let json = r#"{}"#;
        let input: UpdateFeatureInput = serde_json::from_str(json).unwrap();
        assert!(input.target_version_id.is_none());
    }
}

/// A living description of a system capability.
///
/// Unlike traditional issue trackers where items are "closed" and forgotten,
/// features are permanent documentation that evolves with the codebase.
/// Features form a hierarchical tree structure via `parent_id`, where any node
/// can have content, but only leaf nodes can have active sessions.
///
/// # Lifecycle
/// Features progress through states: Proposed → Specified → Implemented → (Living).
/// The "living" phase is implicit—implemented features remain active documentation
/// until deprecated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub id: Uuid,
    pub project_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub title: String,
    /// Feature details including user stories, implementation notes, and technical context.
    /// User stories can be embedded here in "As a... I want... So that..." format.
    pub details: Option<String>,
    /// Desired details for pending changes. When non-null, indicates edits awaiting implementation.
    /// Session completion promotes `desired_details` → `details` when `mark_implemented=true`.
    pub desired_details: Option<String>,
    pub state: FeatureState,
    /// Priority for ordering features within a parent. Lower values appear first.
    /// Use this to indicate implementation order without polluting feature titles.
    pub priority: i32,
    /// Target version for this feature (for release planning).
    /// Null for implemented features or features not yet assigned to a version.
    pub target_version_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The lifecycle state of a feature.
///
/// - `Proposed`: Initial idea, in backlog
/// - `InProgress`: Actively being worked on
/// - `Implemented`: Built and deployed (enters "living" phase)
/// - `Deprecated`: No longer active, kept for historical reference
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureState {
    Proposed,
    InProgress,
    Implemented,
    Deprecated,
}

impl FeatureState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::InProgress => "in_progress",
            Self::Implemented => "implemented",
            Self::Deprecated => "deprecated",
        }
    }
}

impl FromStr for FeatureState {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "proposed" => Ok(Self::Proposed),
            "in_progress" => Ok(Self::InProgress),
            "implemented" => Ok(Self::Implemented),
            "deprecated" => Ok(Self::Deprecated),
            _ => Err(()),
        }
    }
}

/// Input for creating a new feature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFeatureInput {
    /// Pre-generated feature ID. If not provided, a new UUID will be generated.
    /// Use this for bulk creation where parent-child relationships need known IDs.
    #[serde(default)]
    pub id: Option<Uuid>,
    /// Parent feature ID for nesting. `None` creates a root feature.
    pub parent_id: Option<Uuid>,
    pub title: String,
    /// Feature details including user stories, implementation notes, and technical context.
    pub details: Option<String>,
    /// Initial state. Defaults to `Proposed` if not specified.
    pub state: Option<FeatureState>,
    /// Priority for ordering within parent. Lower values first. Defaults to 0.
    pub priority: Option<i32>,
    /// Target version for release planning.
    pub target_version_id: Option<Uuid>,
}

/// Input for updating an existing feature. All fields are optional for partial updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateFeatureInput {
    /// Move feature under a different parent.
    pub parent_id: Option<Uuid>,
    pub title: Option<String>,
    pub details: Option<String>,
    /// Desired details for pending changes. Set to implement declarative editing workflow.
    pub desired_details: Option<String>,
    pub state: Option<FeatureState>,
    /// Update priority for ordering within parent.
    pub priority: Option<i32>,
    /// Target version for release planning.
    /// Uses double Option to distinguish "field absent" (None) from "set to null" (Some(None)).
    #[serde(default, deserialize_with = "double_option::deserialize")]
    pub target_version_id: Option<Option<Uuid>>,
}

/// A feature with its nested children, used for tree responses.
///
/// The `feature` fields are flattened into the JSON response, with an additional
/// `children` array containing nested `FeatureTreeNode` objects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureTreeNode {
    #[serde(flatten)]
    pub feature: Feature,
    pub children: Vec<FeatureTreeNode>,
}

/// Diff between current and desired feature details.
///
/// Used to show pending changes before implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureDiff {
    /// Whether there are pending changes (desired_details differs from details).
    pub has_changes: bool,
    /// Current details (what the feature IS).
    pub current: Option<String>,
    /// Desired details (what the feature SHOULD be).
    pub desired: Option<String>,
}

/// Lightweight feature summary without details (used for list operations).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSummary {
    pub id: Uuid,
    pub project_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub title: String,
    pub state: FeatureState,
    pub priority: i32,
    /// Target version for release planning.
    pub target_version_id: Option<Uuid>,
}

impl From<Feature> for FeatureSummary {
    fn from(f: Feature) -> Self {
        Self {
            id: f.id,
            project_id: f.project_id,
            parent_id: f.parent_id,
            title: f.title,
            state: f.state,
            priority: f.priority,
            target_version_id: f.target_version_id,
        }
    }
}

/// Query parameters for listing features.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ListFeaturesQuery {
    /// Maximum number of features to return.
    pub limit: Option<u32>,
    /// Number of features to skip for pagination.
    pub offset: Option<u32>,
}

/// Lightweight feature summary for context (parent, siblings, children).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSummaryContext {
    pub id: Uuid,
    pub title: String,
    pub state: FeatureState,
}

/// Breadcrumb item for navigation path (root → feature).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreadcrumbItem {
    pub id: Uuid,
    pub title: String,
}

/// A feature with its hierarchical context (parent, siblings, children, breadcrumb).
///
/// Used by `get_feature` MCP tool to provide navigation context for AI agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureWithContext {
    /// The feature itself with all details.
    #[serde(flatten)]
    pub feature: Feature,
    /// Parent feature (if not a root).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<FeatureSummaryContext>,
    /// Sibling features (same parent, excluding self).
    pub siblings: Vec<FeatureSummaryContext>,
    /// Direct children of this feature.
    pub children: Vec<FeatureSummaryContext>,
    /// Breadcrumb trail from root to this feature.
    pub breadcrumb: Vec<BreadcrumbItem>,
}
