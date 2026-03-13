//! Maps between Manifest feature states and GitHub labels.
//!
//! The label schema uses a `manifest:` prefix so they don't collide with
//! user-created labels. Each feature issue has exactly one state label.

use crate::models::FeatureState;

/// A label definition for creating labels on the GitHub repo.
#[derive(Debug, Clone)]
pub struct LabelDef {
    pub name: &'static str,
    pub color: &'static str,
    pub description: &'static str,
}

/// All Manifest-managed labels that should exist on the repository.
pub const MANIFEST_LABELS: &[LabelDef] = &[
    LabelDef {
        name: "manifest:proposed",
        color: "E8D44D",
        description: "Manifest: proposed feature",
    },
    LabelDef {
        name: "manifest:in_progress",
        color: "1D76DB",
        description: "Manifest: feature in progress",
    },
    LabelDef {
        name: "manifest:implemented",
        color: "0E8A16",
        description: "Manifest: implemented feature",
    },
    LabelDef {
        name: "manifest:archived",
        color: "6A737D",
        description: "Manifest: archived feature",
    },
    LabelDef {
        name: "manifest:feature_set",
        color: "D4C5F9",
        description: "Manifest: parent feature with sub-features",
    },
];

/// Convert a GitHub label name to a Manifest feature state.
///
/// Returns `None` for labels that aren't Manifest state labels
/// (e.g., `manifest:feature_set` or user labels).
pub fn label_to_state(label: &str) -> Option<FeatureState> {
    match label {
        "manifest:proposed" => Some(FeatureState::Proposed),
        "manifest:in_progress" => Some(FeatureState::InProgress),
        "manifest:implemented" => Some(FeatureState::Implemented),
        "manifest:archived" => Some(FeatureState::Archived),
        _ => None,
    }
}

/// Convert a Manifest feature state to the corresponding GitHub label name.
pub fn state_to_label(state: FeatureState) -> &'static str {
    match state {
        FeatureState::Proposed => "manifest:proposed",
        FeatureState::Blocked => "manifest:proposed", // Blocked renders as proposed in GitHub
        FeatureState::InProgress => "manifest:in_progress",
        FeatureState::Implemented => "manifest:implemented",
        FeatureState::Archived => "manifest:archived",
    }
}

/// Check if a label name is a Manifest-managed label.
pub fn is_manifest_label(label: &str) -> bool {
    label.starts_with("manifest:")
}

/// Extract the Manifest state from a list of label names.
///
/// Returns the first valid state label found, or `None` if no state label exists.
/// Logs a warning if multiple state labels are present (per RFC: "Manifest enforces
/// this on write and warns on read if multiple are found").
pub fn state_from_labels(labels: &[String]) -> Option<FeatureState> {
    let states: Vec<FeatureState> = labels.iter().filter_map(|l| label_to_state(l)).collect();

    if states.len() > 1 {
        tracing::warn!(
            "Issue has multiple Manifest state labels: {:?}. Using first.",
            labels
                .iter()
                .filter(|l| label_to_state(l).is_some())
                .collect::<Vec<_>>()
        );
    }

    states.into_iter().next()
}

/// Check if label list contains the `manifest:feature_set` label.
pub fn is_feature_set(labels: &[String]) -> bool {
    labels.iter().any(|l| l == "manifest:feature_set")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_to_state_maps_all_states() {
        assert_eq!(
            label_to_state("manifest:proposed"),
            Some(FeatureState::Proposed)
        );
        assert_eq!(
            label_to_state("manifest:in_progress"),
            Some(FeatureState::InProgress)
        );
        assert_eq!(
            label_to_state("manifest:implemented"),
            Some(FeatureState::Implemented)
        );
        assert_eq!(
            label_to_state("manifest:archived"),
            Some(FeatureState::Archived)
        );
    }

    #[test]
    fn label_to_state_returns_none_for_non_state() {
        assert_eq!(label_to_state("manifest:feature_set"), None);
        assert_eq!(label_to_state("bug"), None);
        assert_eq!(label_to_state(""), None);
    }

    #[test]
    fn state_to_label_roundtrips() {
        for state in [
            FeatureState::Proposed,
            FeatureState::InProgress,
            FeatureState::Implemented,
            FeatureState::Archived,
        ] {
            let label = state_to_label(state);
            assert_eq!(label_to_state(label), Some(state));
        }
    }

    #[test]
    fn blocked_maps_to_proposed_label() {
        assert_eq!(state_to_label(FeatureState::Blocked), "manifest:proposed");
    }

    #[test]
    fn state_from_labels_picks_first() {
        let labels = vec!["manifest:in_progress".to_string(), "bug".to_string()];
        assert_eq!(state_from_labels(&labels), Some(FeatureState::InProgress));
    }

    #[test]
    fn state_from_labels_returns_none_when_no_state() {
        let labels = vec!["bug".to_string(), "manifest:feature_set".to_string()];
        assert_eq!(state_from_labels(&labels), None);
    }

    #[test]
    fn is_manifest_label_checks_prefix() {
        assert!(is_manifest_label("manifest:proposed"));
        assert!(is_manifest_label("manifest:feature_set"));
        assert!(!is_manifest_label("bug"));
    }

    #[test]
    fn is_feature_set_detects_label() {
        let labels = vec![
            "manifest:proposed".to_string(),
            "manifest:feature_set".to_string(),
        ];
        assert!(is_feature_set(&labels));

        let labels = vec!["manifest:proposed".to_string()];
        assert!(!is_feature_set(&labels));
    }

    #[test]
    fn manifest_labels_has_expected_count() {
        assert_eq!(MANIFEST_LABELS.len(), 5);
    }
}
