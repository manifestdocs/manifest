use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use validator::Validate;

use super::{FeatureId, ProjectId, VersionId};

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
    use uuid::Uuid;

    #[test]
    fn derive_all_proposed_returns_proposed() {
        let states = vec![FeatureState::Proposed, FeatureState::Proposed];
        assert_eq!(
            FeatureState::derive_from_children(&states),
            Some(FeatureState::Proposed)
        );
    }

    #[test]
    fn derive_all_implemented_returns_implemented() {
        let states = vec![FeatureState::Implemented, FeatureState::Implemented];
        assert_eq!(
            FeatureState::derive_from_children(&states),
            Some(FeatureState::Implemented)
        );
    }

    #[test]
    fn derive_mix_proposed_and_implemented_returns_in_progress() {
        let states = vec![FeatureState::Proposed, FeatureState::Implemented];
        assert_eq!(
            FeatureState::derive_from_children(&states),
            Some(FeatureState::InProgress)
        );
    }

    #[test]
    fn derive_any_in_progress_returns_in_progress() {
        let states = vec![
            FeatureState::Proposed,
            FeatureState::InProgress,
            FeatureState::Implemented,
        ];
        assert_eq!(
            FeatureState::derive_from_children(&states),
            Some(FeatureState::InProgress)
        );
    }

    #[test]
    fn derive_all_archived_returns_none() {
        let states = vec![FeatureState::Archived, FeatureState::Archived];
        assert_eq!(FeatureState::derive_from_children(&states), None);
    }

    #[test]
    fn derive_empty_returns_none() {
        assert_eq!(FeatureState::derive_from_children(&[]), None);
    }

    #[test]
    fn derive_ignores_archived_children() {
        // One implemented + one archived = all active are implemented
        let states = vec![FeatureState::Implemented, FeatureState::Archived];
        assert_eq!(
            FeatureState::derive_from_children(&states),
            Some(FeatureState::Implemented)
        );
    }

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

    #[test]
    fn test_update_feature_input_serialize_none_omits_field() {
        // When target_version_id is None (don't update), it should NOT appear in JSON
        let input = UpdateFeatureInput {
            parent_id: None,
            title: Some("Test".to_string()),
            details: None,
            desired_details: None,
            details_summary: None,
            state: Some(FeatureState::InProgress),
            priority: None,
            target_version_id: None,
            blocked_by: None,
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(
            !json.contains("target_version_id"),
            "JSON should NOT contain target_version_id when it's None, got: {}",
            json
        );
    }

    #[test]
    fn test_update_feature_input_serialize_some_none_produces_null() {
        // When target_version_id is Some(None) (clear the field), it should be "null"
        let input = UpdateFeatureInput {
            parent_id: None,
            title: None,
            details: None,
            desired_details: None,
            details_summary: None,
            state: None,
            priority: None,
            target_version_id: Some(None),
            blocked_by: None,
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(
            json.contains(r#""target_version_id":null"#),
            "JSON should contain target_version_id: null when it's Some(None), got: {}",
            json
        );
    }

    #[test]
    fn test_update_feature_input_serialize_some_uuid() {
        // When target_version_id is Some(Some(uuid)), it should be the UUID string
        let uuid = Uuid::parse_str("fb5e9bc0-6202-4617-92f0-3eb6d943bc4f").unwrap();
        let input = UpdateFeatureInput {
            parent_id: None,
            title: None,
            details: None,
            desired_details: None,
            details_summary: None,
            state: None,
            priority: None,
            target_version_id: Some(Some(VersionId::from(uuid))),
            blocked_by: None,
        };
        let json = serde_json::to_string(&input).unwrap();
        assert!(
            json.contains(r#""target_version_id":"fb5e9bc0-6202-4617-92f0-3eb6d943bc4f""#),
            "JSON should contain the UUID, got: {}",
            json
        );
    }

    #[test]
    fn derive_blocked_children_treated_as_proposed() {
        // All blocked children → no implemented/in_progress → Proposed
        let states = vec![FeatureState::Blocked, FeatureState::Blocked];
        assert_eq!(
            FeatureState::derive_from_children(&states),
            Some(FeatureState::Proposed)
        );
    }

    #[test]
    fn derive_mix_blocked_and_implemented() {
        // Blocked + implemented → any_implemented = true, not all_implemented → InProgress
        let states = vec![FeatureState::Blocked, FeatureState::Implemented];
        assert_eq!(
            FeatureState::derive_from_children(&states),
            Some(FeatureState::InProgress)
        );
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
/// Features progress through states: Proposed → InProgress → Implemented → (Living).
/// Features can also be Blocked (waiting on dependencies) which auto-resolves to Proposed
/// when all blockers are implemented. The "living" phase is implicit—implemented features
/// remain active documentation until archived.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub id: FeatureId,
    pub project_id: ProjectId,
    pub parent_id: Option<FeatureId>,
    pub title: String,
    /// Feature details including user stories, implementation notes, and technical context.
    /// User stories can be embedded here in "As a... I want... So that..." format.
    pub details: Option<String>,
    /// Desired details for pending changes. When non-null, indicates edits awaiting implementation.
    /// Session completion promotes `desired_details` → `details` when `mark_implemented=true`.
    pub desired_details: Option<String>,
    /// Short summary of details (~200 words) for root features.
    /// Used in breadcrumbs and project listings to avoid sending full instructions on every request.
    pub details_summary: Option<String>,
    pub state: FeatureState,
    /// Priority for ordering features within a parent. Lower values appear first.
    /// Use this to indicate implementation order without polluting feature titles.
    pub priority: i32,
    /// Per-project sequential number for display IDs (e.g., 42 in MAN-42).
    pub feature_number: Option<i32>,
    /// Target version for this feature (for release planning).
    /// Null for implemented features or features not yet assigned to a version.
    pub target_version_id: Option<VersionId>,
    /// Structured comments from the most recent `record_verification` call.
    /// None if this feature has never been verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_result: Option<Vec<VerificationComment>>,
    /// Timestamp of the most recent verification run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<DateTime<Utc>>,
    /// Agent type that claimed this feature (e.g., "claude", "gemini", "codex", "copilot").
    /// Set when `start_feature` transitions to `in_progress`, cleared on `complete_feature`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    /// When the feature was claimed (RFC3339). Used for staleness detection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_at: Option<DateTime<Utc>>,
    /// Agent-specific metadata as JSON (e.g., branch name, worktree path, container ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_metadata: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The lifecycle state of a feature.
///
/// - `Proposed`: Initial idea, in backlog
/// - `Blocked`: Cannot proceed until blocker features are implemented. Only proposed features
///   can transition to blocked, and `blocked_by` must contain at least one feature ID.
///   Auto-transitions back to `proposed` when all blockers reach `implemented`.
/// - `InProgress`: Actively being worked on
/// - `Implemented`: Built and deployed (enters "living" phase)
/// - `Archived`: Soft-deleted, kept for historical reference
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureState {
    Proposed,
    Blocked,
    InProgress,
    Implemented,
    Archived,
}

impl FeatureState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Blocked => "blocked",
            Self::InProgress => "in_progress",
            Self::Implemented => "implemented",
            Self::Archived => "archived",
        }
    }

    /// Derive parent state from children's states.
    /// Only considers non-archived children. Returns None if no non-archived children exist,
    /// meaning the caller should keep the DB state as-is.
    pub fn derive_from_children(children_states: &[FeatureState]) -> Option<FeatureState> {
        let active: Vec<_> = children_states
            .iter()
            .filter(|s| **s != FeatureState::Archived)
            .collect();

        if active.is_empty() {
            return None;
        }

        let all_implemented = active.iter().all(|s| **s == FeatureState::Implemented);
        let any_in_progress = active.iter().any(|s| **s == FeatureState::InProgress);
        let any_implemented = active.iter().any(|s| **s == FeatureState::Implemented);

        Some(if all_implemented {
            FeatureState::Implemented
        } else if any_in_progress || any_implemented {
            FeatureState::InProgress // partial progress
        } else {
            FeatureState::Proposed
        })
    }
}

impl FromStr for FeatureState {
    type Err = super::ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "proposed" => Ok(Self::Proposed),
            "blocked" => Ok(Self::Blocked),
            "in_progress" => Ok(Self::InProgress),
            "implemented" => Ok(Self::Implemented),
            "archived" => Ok(Self::Archived),
            _ => Err(super::ParseEnumError(s.to_string())),
        }
    }
}

/// Input for creating a new feature.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CreateFeatureInput {
    /// Pre-generated feature ID. If not provided, a new UUID will be generated.
    /// Use this for bulk creation where parent-child relationships need known IDs.
    #[serde(default)]
    pub id: Option<FeatureId>,
    /// Parent feature ID for nesting. `None` creates a root feature.
    pub parent_id: Option<FeatureId>,
    #[validate(length(min = 1, max = 500))]
    pub title: String,
    /// Feature details including user stories, implementation notes, and technical context.
    #[validate(length(max = 100_000))]
    pub details: Option<String>,
    /// Initial state. Defaults to `Proposed` if not specified.
    pub state: Option<FeatureState>,
    /// Priority for ordering within parent. Lower values first. Defaults to 0.
    pub priority: Option<i32>,
    /// Target version for release planning.
    pub target_version_id: Option<VersionId>,
}

/// Input for updating an existing feature. All fields are optional for partial updates.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct UpdateFeatureInput {
    /// Move feature under a different parent.
    pub parent_id: Option<FeatureId>,
    #[validate(length(min = 1, max = 500))]
    pub title: Option<String>,
    #[validate(length(max = 100_000))]
    pub details: Option<String>,
    /// Desired details for pending changes. Set to implement declarative editing workflow.
    /// Uses double Option to distinguish "field absent" (None) from "set to null" (Some(None)).
    #[serde(
        default,
        deserialize_with = "double_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub desired_details: Option<Option<String>>,
    /// Short summary of details for root features (~200 words).
    /// Uses double Option to distinguish "field absent" (None) from "set to null" (Some(None)).
    #[serde(
        default,
        deserialize_with = "double_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub details_summary: Option<Option<String>>,
    pub state: Option<FeatureState>,
    /// Update priority for ordering within parent.
    pub priority: Option<i32>,
    /// Target version for release planning.
    /// Uses double Option to distinguish "field absent" (None) from "set to null" (Some(None)).
    #[serde(
        default,
        deserialize_with = "double_option::deserialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub target_version_id: Option<Option<VersionId>>,
    /// Feature IDs that block this feature. Required when transitioning to `blocked` state.
    /// The feature will auto-transition back to `proposed` when all blockers are `implemented`.
    #[serde(default)]
    pub blocked_by: Option<Vec<FeatureId>>,
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
    /// True if this is the project's root feature (contains project instructions).
    #[serde(default)]
    pub is_root: bool,
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
    pub id: FeatureId,
    pub project_id: ProjectId,
    pub parent_id: Option<FeatureId>,
    pub title: String,
    pub state: FeatureState,
    pub priority: i32,
    /// Per-project sequential number for display IDs (e.g., 42 in MAN-42).
    pub feature_number: Option<i32>,
    /// Target version for release planning.
    pub target_version_id: Option<VersionId>,
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
            feature_number: f.feature_number,
            target_version_id: f.target_version_id,
        }
    }
}

/// Query parameters for listing features.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ListFeaturesQuery {
    /// Project ID to filter features (required in cloud mode).
    pub project_id: Option<ProjectId>,
    /// Version ID to filter features by their target version assignment.
    pub version_id: Option<VersionId>,
    /// Maximum number of features to return.
    pub limit: Option<u32>,
    /// Number of features to skip for pagination.
    pub offset: Option<u32>,
}

/// Lightweight feature summary for context (parent, siblings, children).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureSummaryContext {
    pub id: FeatureId,
    pub title: String,
    pub state: FeatureState,
}

/// Breadcrumb item for navigation path (root → feature).
/// Includes details for ancestor context that flows down to children.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreadcrumbItem {
    pub id: FeatureId,
    pub title: String,
    /// Contextual details from this ancestor (architectural decisions, conventions, constraints).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

/// Severity level for a verification comment.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationSeverity {
    /// Core spec requirement not implemented; blocks honest completion.
    Critical,
    /// Significant gap or behavioural deviation; should be fixed before shipping.
    Major,
    /// Polish, naming, or style drift from spec intent.
    Minor,
}

impl VerificationSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Major => "major",
            Self::Minor => "minor",
        }
    }
}

impl std::str::FromStr for VerificationSeverity {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "critical" => Ok(Self::Critical),
            "major" => Ok(Self::Major),
            "minor" => Ok(Self::Minor),
            other => Err(format!("Unknown severity: {other}")),
        }
    }
}

/// A single comment produced by implementation verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationComment {
    pub severity: VerificationSeverity,
    /// One-line summary of the gap.
    pub title: String,
    /// Actionable explanation with suggested fix.
    pub body: String,
    /// Affected file path if the gap is localized to a single file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// Response from `POST /features/:id/verify` — assembled context for the calling agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyFeatureContextResponse {
    /// Feature specification + breadcrumb context formatted as markdown.
    pub spec: String,
    /// Filtered implementation diff (None if no diff was provided or no uncommitted changes).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    /// True if the diff was truncated due to size limit.
    pub diff_truncated: bool,
    /// Instruction text for the calling agent on how to proceed.
    pub instructions: String,
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
