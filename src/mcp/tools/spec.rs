//! Spec analysis for MCP tool responses.
//!
//! Pure analysis — no I/O, no database. Checks the `details` markdown field
//! for structured sections (story, acceptance criteria, constraints).
//! Provides tier-aware guidance for project, feature set, and leaf features.

/// The tier of a feature in the hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureTier {
    /// Root feature — project-level instructions, decisions, conventions.
    Project,
    /// Parent feature with children — shared architectural context.
    FeatureSet,
    /// Leaf feature — implementable unit with story, AC, constraints.
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
    pub has_story: bool,
    pub has_acceptance_criteria: bool,
    pub has_constraints: bool,
    pub tier: FeatureTier,
}

impl SpecStatus {
    /// True when a leaf feature has no details at all — blocks `start_feature`.
    pub fn should_block(&self) -> bool {
        self.tier == FeatureTier::Leaf && !self.has_details
    }

    /// True when details exist but acceptance criteria are missing (leaf only).
    pub fn has_warnings(&self) -> bool {
        self.tier == FeatureTier::Leaf && self.has_details && !self.has_acceptance_criteria
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
                    return "Spec: no details".to_string();
                }
                let check = |ok: bool| if ok { "✓" } else { "✗" };
                format!(
                    "Spec: story {}, acceptance criteria {}, constraints {}",
                    check(self.has_story),
                    check(self.has_acceptance_criteria),
                    check(self.has_constraints),
                )
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
                         Expected format:\n\n\
                         ## Story\n\
                         As a [user], I can [capability] so that [benefit].\n\n\
                         ## Acceptance Criteria\n\
                         - Given [precondition], when [action], then [expected outcome]\n\n\
                         ## Constraints\n\
                         - [Technical constraints, performance requirements, security considerations]"
                            .to_string(),
                    );
                }
                if !self.has_acceptance_criteria {
                    return Some(
                        "Specification exists but is missing acceptance criteria. \
                         Consider adding a '## Acceptance Criteria' section or \
                         Given/When/Then scenarios before implementation."
                            .to_string(),
                    );
                }
                None
            }
        }
    }
}

/// Analyze a feature's details for structured spec sections.
///
/// Detection heuristics (case-insensitive):
/// - **Story**: contains `"as a "` or has `## Story` header
/// - **Acceptance Criteria**: has `## Acceptance Criteria` header, OR contains
///   all three of `given`/`when`/`then`
/// - **Constraints**: has `## Constraints` header
pub fn analyze_spec(details: Option<&str>, has_children: bool, is_root: bool) -> SpecStatus {
    let text = details.unwrap_or("").trim();
    let has_details = !text.is_empty();
    let lower = text.to_lowercase();

    let has_story = lower.contains("as a ") || lower.contains("## story");

    let has_acceptance_criteria = lower.contains("## acceptance criteria")
        || (lower.contains("given") && lower.contains("when") && lower.contains("then"));

    let has_constraints = lower.contains("## constraints");

    let tier = if is_root {
        FeatureTier::Project
    } else if has_children {
        FeatureTier::FeatureSet
    } else {
        FeatureTier::Leaf
    };

    SpecStatus {
        has_details,
        has_story,
        has_acceptance_criteria,
        has_constraints,
        tier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Leaf feature tests (existing behavior) ---

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
    fn details_without_ac_warns() {
        let status = analyze_spec(Some("As a user, I can do things."), false, false);
        assert!(!status.should_block());
        assert!(status.has_warnings());
        assert!(status.guidance().is_some());
    }

    #[test]
    fn full_spec_no_block_no_warnings() {
        let spec = "\
## Story
As a developer, I can index code so that I can navigate quickly.

## Acceptance Criteria
- Given a Rust project, when I run `index`, then all symbols are indexed

## Constraints
- Must complete in under 5 seconds for 100k LOC";

        let status = analyze_spec(Some(spec), false, false);
        assert!(!status.should_block());
        assert!(!status.has_warnings());
        assert!(status.guidance().is_none());
        assert!(status.has_story);
        assert!(status.has_acceptance_criteria);
        assert!(status.has_constraints);
    }

    #[test]
    fn gherkin_style_detects_ac() {
        let spec = "As a user, I can log in.\n\nGiven I am on the login page\nWhen I enter credentials\nThen I am authenticated";
        let status = analyze_spec(Some(spec), false, false);
        assert!(status.has_acceptance_criteria);
        assert!(!status.has_warnings());
    }

    #[test]
    fn summary_shows_checks() {
        let status = analyze_spec(Some("As a user, I can do things."), false, false);
        assert!(status.summary().contains("story ✓"));
        assert!(status.summary().contains("acceptance criteria ✗"));
    }

    #[test]
    fn summary_no_details() {
        let status = analyze_spec(None, false, false);
        assert_eq!(status.summary(), "Spec: no details");
    }

    #[test]
    fn case_insensitive_headers() {
        let spec =
            "## STORY\nAs A developer\n## ACCEPTANCE CRITERIA\n- test\n## CONSTRAINTS\n- none";
        let status = analyze_spec(Some(spec), false, false);
        assert!(status.has_story);
        assert!(status.has_acceptance_criteria);
        assert!(status.has_constraints);
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
        // Edge case: root feature with no children yet
        let status = analyze_spec(None, false, true);
        assert_eq!(status.tier, FeatureTier::Project);
    }
}
