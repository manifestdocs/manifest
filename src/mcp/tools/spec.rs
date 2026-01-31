//! Spec analysis for MCP tool responses.
//!
//! Pure analysis — no I/O, no database. Checks the `details` field for
//! non-trivial content. Provides tier-aware guidance for project, feature set,
//! and leaf features.

/// The tier of a feature in the hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureTier {
    /// Root feature — project-level instructions, decisions, conventions.
    Project,
    /// Parent feature with children — shared architectural context.
    FeatureSet,
    /// Leaf feature — implementable unit with concise specification.
    Leaf,
}

impl FeatureTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            FeatureTier::Project => "project",
            FeatureTier::FeatureSet => "feature_set",
            FeatureTier::Leaf => "leaf",
        }
    }
}

/// Status of a feature's specification completeness.
pub struct SpecStatus {
    pub has_details: bool,
    pub word_count: usize,
    pub tier: FeatureTier,
}

impl SpecStatus {
    /// True when a leaf feature has no details at all — blocks `start_feature`.
    pub fn should_block(&self) -> bool {
        self.tier == FeatureTier::Leaf && !self.has_details
    }

    /// True when details exist but are very sparse (leaf only).
    pub fn has_warnings(&self) -> bool {
        self.tier == FeatureTier::Leaf && self.has_details && self.word_count < 20
    }

    /// One-line status string for MCP responses.
    pub fn summary(&self) -> String {
        match self.tier {
            FeatureTier::Project => {
                if !self.has_details {
                    "Project: no instructions yet".to_string()
                } else {
                    "Project: has instructions".to_string()
                }
            }
            FeatureTier::FeatureSet => {
                if !self.has_details {
                    "Feature set: no shared context yet".to_string()
                } else {
                    "Feature set: has shared context".to_string()
                }
            }
            FeatureTier::Leaf => {
                if !self.has_details {
                    "Spec: no details".to_string()
                } else {
                    format!("Spec: has details (~{} words)", self.word_count)
                }
            }
        }
    }

    /// Actionable guidance when the spec is incomplete. `None` when complete.
    pub fn guidance(&self) -> Option<String> {
        match self.tier {
            FeatureTier::Project => {
                if !self.has_details {
                    Some(
                        "This is the project root — all agents read this before working on any feature. \
                         Add project-wide instructions: tech stack, conventions, architectural decisions, \
                         and security boundaries."
                            .to_string(),
                    )
                } else {
                    None
                }
            }
            FeatureTier::FeatureSet => {
                if !self.has_details {
                    Some(
                        "This feature set has no shared context. Consider adding architectural decisions, \
                         shared patterns, or constraints that apply to all child features."
                            .to_string(),
                    )
                } else {
                    None
                }
            }
            FeatureTier::Leaf => {
                if !self.has_details {
                    return Some(
                        "This feature has no specification. Use update_feature to add details before starting.\n\n\
                         Write a concise specification (~50-150 words):\n\
                         - Goal: what the feature does and why\n\
                         - Constraints: performance, security, compatibility\n\
                         - Key interfaces: function signatures with types (if applicable)\n\
                         - Examples: 1-3 concrete examples of expected behavior (if helpful)"
                            .to_string(),
                    );
                }
                if self.word_count < 20 {
                    return Some(
                        "Specification exists but is brief. Consider adding key constraints \
                         or examples before implementing."
                            .to_string(),
                    );
                }
                None
            }
        }
    }
}

/// Analyze a feature's details for specification completeness.
///
/// Uses word count as the primary heuristic: specs under 20 words are
/// considered sparse and trigger a warning for leaf features.
pub fn analyze_spec(details: Option<&str>, has_children: bool, is_root: bool) -> SpecStatus {
    let text = details.unwrap_or("").trim();
    let has_details = !text.is_empty();
    let word_count = text.split_whitespace().count();

    let tier = if is_root {
        FeatureTier::Project
    } else if has_children {
        FeatureTier::FeatureSet
    } else {
        FeatureTier::Leaf
    };

    SpecStatus {
        has_details,
        word_count,
        tier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Leaf feature tests ---

    #[test]
    fn empty_details_should_block() {
        let status = analyze_spec(None, false, false);
        assert!(status.should_block());
        assert!(!status.has_warnings());
        assert!(status.guidance().is_some());
    }

    #[test]
    fn whitespace_only_should_block() {
        let status = analyze_spec(Some("   \n\t  "), false, false);
        assert!(status.should_block());
    }

    #[test]
    fn sparse_details_warns() {
        let status = analyze_spec(Some("Handle login."), false, false);
        assert!(!status.should_block());
        assert!(status.has_warnings());
        assert!(status.guidance().unwrap().contains("brief"));
    }

    #[test]
    fn sufficient_details_no_warnings() {
        let spec = "Accepts an email and password, validates credentials against the database, \
                     returns a JWT on success. Must rate-limit to 5 attempts per minute. \
                     Returns 401 with generic error on failure.";
        let status = analyze_spec(Some(spec), false, false);
        assert!(!status.should_block());
        assert!(!status.has_warnings());
        assert!(status.guidance().is_none());
    }

    #[test]
    fn summary_with_details_shows_word_count() {
        let status = analyze_spec(
            Some("Handle user login with email and password"),
            false,
            false,
        );
        let summary = status.summary();
        assert!(summary.starts_with("Spec: has details"));
        assert!(summary.contains("words"));
    }

    #[test]
    fn summary_no_details() {
        let status = analyze_spec(None, false, false);
        assert_eq!(status.summary(), "Spec: no details");
    }

    #[test]
    fn no_details_guidance_mentions_goal_and_constraints() {
        let status = analyze_spec(None, false, false);
        let guidance = status.guidance().unwrap();
        assert!(guidance.contains("Goal"));
        assert!(guidance.contains("Constraints"));
    }

    // --- Feature set tests ---

    #[test]
    fn feature_set_never_blocks() {
        let status = analyze_spec(None, true, false);
        assert!(!status.should_block());
        assert!(!status.has_warnings());
        assert_eq!(status.tier, FeatureTier::FeatureSet);
    }

    #[test]
    fn feature_set_no_details_shows_guidance() {
        let status = analyze_spec(None, true, false);
        let guidance = status.guidance();
        assert!(guidance.is_some());
        assert!(guidance.unwrap().contains("shared context"));
    }

    #[test]
    fn feature_set_with_details_no_guidance() {
        let status = analyze_spec(Some("Architecture notes"), true, false);
        assert!(status.guidance().is_none());
    }

    #[test]
    fn feature_set_summary_no_details() {
        let status = analyze_spec(None, true, false);
        assert_eq!(status.summary(), "Feature set: no shared context yet");
    }

    #[test]
    fn feature_set_summary_with_details() {
        let status = analyze_spec(Some("Shared patterns"), true, false);
        assert_eq!(status.summary(), "Feature set: has shared context");
    }

    // --- Project (root) tests ---

    #[test]
    fn project_never_blocks() {
        let status = analyze_spec(None, true, true);
        assert!(!status.should_block());
        assert!(!status.has_warnings());
        assert_eq!(status.tier, FeatureTier::Project);
    }

    #[test]
    fn project_no_details_shows_guidance() {
        let status = analyze_spec(None, true, true);
        let guidance = status.guidance();
        assert!(guidance.is_some());
        assert!(guidance.unwrap().contains("project root"));
    }

    #[test]
    fn project_with_details_no_guidance() {
        let status = analyze_spec(Some("Tech stack: Rust + Axum"), true, true);
        assert!(status.guidance().is_none());
    }

    #[test]
    fn project_summary_no_details() {
        let status = analyze_spec(None, true, true);
        assert_eq!(status.summary(), "Project: no instructions yet");
    }

    #[test]
    fn project_summary_with_details() {
        let status = analyze_spec(Some("Use reqwest for HTTP"), true, true);
        assert_eq!(status.summary(), "Project: has instructions");
    }

    // --- Tier detection ---

    #[test]
    fn tier_root_is_project() {
        let status = analyze_spec(None, true, true);
        assert_eq!(status.tier, FeatureTier::Project);
    }

    #[test]
    fn tier_parent_is_feature_set() {
        let status = analyze_spec(None, true, false);
        assert_eq!(status.tier, FeatureTier::FeatureSet);
    }

    #[test]
    fn tier_leaf_is_leaf() {
        let status = analyze_spec(None, false, false);
        assert_eq!(status.tier, FeatureTier::Leaf);
    }

    #[test]
    fn root_without_children_still_project() {
        let status = analyze_spec(None, false, true);
        assert_eq!(status.tier, FeatureTier::Project);
    }
}
