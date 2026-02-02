//! Spec analysis for MCP tool responses.
//!
//! Pure analysis — no I/O, no database. Checks the `details` field for
//! non-trivial content. Provides tier-aware, level-aware guidance for project,
//! feature set, and leaf features.

use manifest_core::models::{AcFormat, GuidanceLevel};

/// Project-level configuration that controls guidance text and thresholds.
#[derive(Debug, Clone, Copy)]
pub struct SpecConfig {
    /// Controls guidance for feature sets and project-level details.
    pub detail_level: GuidanceLevel,
    /// Controls guidance for leaf feature acceptance criteria.
    pub ac_level: GuidanceLevel,
    /// Controls the output format of acceptance criteria.
    pub ac_format: AcFormat,
}

impl Default for SpecConfig {
    fn default() -> Self {
        Self {
            detail_level: GuidanceLevel::Standard,
            ac_level: GuidanceLevel::Standard,
            ac_format: AcFormat::Checkbox,
        }
    }
}

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
    /// Threshold adapts to `ac_level`.
    pub fn has_warnings(&self, config: &SpecConfig) -> bool {
        if self.tier != FeatureTier::Leaf || !self.has_details {
            return false;
        }
        let threshold = match config.ac_level {
            GuidanceLevel::Concise => 10,
            GuidanceLevel::Standard => 20,
            GuidanceLevel::Thorough => 40,
        };
        self.word_count < threshold
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
    /// Text adapts to the project's configured guidance levels.
    pub fn guidance(&self, config: &SpecConfig) -> Option<String> {
        match self.tier {
            FeatureTier::Project => self.project_guidance(config),
            FeatureTier::FeatureSet => self.feature_set_guidance(config),
            FeatureTier::Leaf => self.leaf_guidance(config),
        }
    }

    fn project_guidance(&self, config: &SpecConfig) -> Option<String> {
        if self.has_details {
            return None;
        }
        Some(match config.detail_level {
            GuidanceLevel::Concise => {
                "This is the project root. Consider adding a brief note on tech stack and key conventions."
                    .to_string()
            }
            GuidanceLevel::Standard => {
                "This is the project root \u{2014} all agents read this before working on any feature. \
                 Add project-wide instructions: tech stack, conventions, architectural decisions, \
                 and security boundaries."
                    .to_string()
            }
            GuidanceLevel::Thorough => {
                "This is the project root \u{2014} all agents read this before working on any feature. \
                 Add comprehensive project instructions:\n\
                 - Tech stack and key dependencies\n\
                 - Architectural decisions with rationale and alternatives considered\n\
                 - Coding conventions, patterns, and anti-patterns\n\
                 - Security boundaries and constraints\n\
                 - Testing expectations and coverage goals\n\
                 - Domain terminology and glossary"
                    .to_string()
            }
        })
    }

    fn feature_set_guidance(&self, config: &SpecConfig) -> Option<String> {
        if self.has_details {
            return None;
        }
        Some(match config.detail_level {
            GuidanceLevel::Concise => {
                "This feature set has no shared context. Consider adding a brief architectural note \u{2014} \
                 key patterns or constraints that apply to child features."
                    .to_string()
            }
            GuidanceLevel::Standard => {
                "This feature set has no shared context. Consider adding architectural decisions, \
                 shared patterns, or constraints that apply to all child features."
                    .to_string()
            }
            GuidanceLevel::Thorough => {
                "This feature set has no shared context. Consider adding detailed design rationale:\n\
                 - Architectural decisions with alternatives considered\n\
                 - Shared patterns and interfaces\n\
                 - Cross-cutting constraints (security, performance, compatibility)\n\
                 - Conventions for child features"
                    .to_string()
            }
        })
    }

    fn leaf_guidance(&self, config: &SpecConfig) -> Option<String> {
        if !self.has_details {
            return Some(match config.ac_level {
                GuidanceLevel::Concise => {
                    "This feature has no specification. Use update_feature to add details before starting.\n\n\
                     Write a brief specification (~30-80 words):\n\
                     - Goal: what the feature does and why\n\
                     - Key constraint: the most important limitation or requirement"
                        .to_string()
                }
                GuidanceLevel::Standard => {
                    "This feature has no specification. Use update_feature to add details before starting.\n\n\
                     Write a concise specification (~50-150 words):\n\
                     - Goal: what the feature does and why\n\
                     - Constraints: performance, security, compatibility\n\
                     - Key interfaces: function signatures with types (if applicable)\n\
                     - Examples: 1-3 concrete examples of expected behavior (if helpful)"
                        .to_string()
                }
                GuidanceLevel::Thorough => {
                    "This feature has no specification. Use update_feature to add details before starting.\n\n\
                     Write a detailed specification (~100-300 words):\n\
                     - Story: As a [user], I can [capability] so that [benefit]\n\
                     - Acceptance criteria: Given/When/Then for key scenarios\n\
                     - Constraints: performance, security, compatibility\n\
                     - Key interfaces: function signatures with types\n\
                     - Examples: concrete examples of expected behavior"
                        .to_string()
                }
            });
        }

        let threshold = match config.ac_level {
            GuidanceLevel::Concise => 10,
            GuidanceLevel::Standard => 20,
            GuidanceLevel::Thorough => 40,
        };

        if self.word_count < threshold {
            return Some(
                "Specification exists but is brief. Consider adding key constraints \
                 or examples before implementing."
                    .to_string(),
            );
        }
        None
    }
}

/// Analyze a feature's details for specification completeness.
///
/// Uses word count as the primary heuristic. Warning thresholds adapt to
/// the project's configured `ac_level`.
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

    fn default_config() -> SpecConfig {
        SpecConfig::default()
    }

    fn concise_config() -> SpecConfig {
        SpecConfig {
            detail_level: GuidanceLevel::Concise,
            ac_level: GuidanceLevel::Concise,
            ac_format: AcFormat::Checkbox,
        }
    }

    fn thorough_config() -> SpecConfig {
        SpecConfig {
            detail_level: GuidanceLevel::Thorough,
            ac_level: GuidanceLevel::Thorough,
            ac_format: AcFormat::Checkbox,
        }
    }

    // --- Leaf feature tests (standard) ---

    #[test]
    fn empty_details_should_block() {
        let config = default_config();
        let status = analyze_spec(None, false, false);
        assert!(status.should_block());
        assert!(!status.has_warnings(&config));
        assert!(status.guidance(&config).is_some());
    }

    #[test]
    fn whitespace_only_should_block() {
        let status = analyze_spec(Some("   \n\t  "), false, false);
        assert!(status.should_block());
    }

    #[test]
    fn sparse_details_warns() {
        let config = default_config();
        let status = analyze_spec(Some("Handle login."), false, false);
        assert!(!status.should_block());
        assert!(status.has_warnings(&config));
        assert!(status.guidance(&config).unwrap().contains("brief"));
    }

    #[test]
    fn sufficient_details_no_warnings() {
        let config = default_config();
        let spec = "Accepts an email and password, validates credentials against the database, \
                     returns a JWT on success. Must rate-limit to 5 attempts per minute. \
                     Returns 401 with generic error on failure.";
        let status = analyze_spec(Some(spec), false, false);
        assert!(!status.should_block());
        assert!(!status.has_warnings(&config));
        assert!(status.guidance(&config).is_none());
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
        let config = default_config();
        let status = analyze_spec(None, false, false);
        let guidance = status.guidance(&config).unwrap();
        assert!(guidance.contains("Goal"));
        assert!(guidance.contains("Constraints"));
    }

    // --- Feature set tests ---

    #[test]
    fn feature_set_never_blocks() {
        let config = default_config();
        let status = analyze_spec(None, true, false);
        assert!(!status.should_block());
        assert!(!status.has_warnings(&config));
        assert_eq!(status.tier, FeatureTier::FeatureSet);
    }

    #[test]
    fn feature_set_no_details_shows_guidance() {
        let config = default_config();
        let status = analyze_spec(None, true, false);
        let guidance = status.guidance(&config);
        assert!(guidance.is_some());
        assert!(guidance.unwrap().contains("shared context"));
    }

    #[test]
    fn feature_set_with_details_no_guidance() {
        let config = default_config();
        let status = analyze_spec(Some("Architecture notes"), true, false);
        assert!(status.guidance(&config).is_none());
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
        let config = default_config();
        let status = analyze_spec(None, true, true);
        assert!(!status.should_block());
        assert!(!status.has_warnings(&config));
        assert_eq!(status.tier, FeatureTier::Project);
    }

    #[test]
    fn project_no_details_shows_guidance() {
        let config = default_config();
        let status = analyze_spec(None, true, true);
        let guidance = status.guidance(&config);
        assert!(guidance.is_some());
        assert!(guidance.unwrap().contains("project root"));
    }

    #[test]
    fn project_with_details_no_guidance() {
        let config = default_config();
        let status = analyze_spec(Some("Tech stack: Rust + Axum"), true, true);
        assert!(status.guidance(&config).is_none());
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

    // --- Concise level tests ---

    #[test]
    fn concise_leaf_lower_warning_threshold() {
        let config = concise_config();
        // 5 words — below concise threshold of 10
        let status = analyze_spec(Some("Handle login with email"), false, false);
        assert!(status.has_warnings(&config));

        // 12 words — above concise threshold of 10
        let status = analyze_spec(
            Some("Handle login with email and password then return a JWT token"),
            false,
            false,
        );
        assert!(!status.has_warnings(&config));
    }

    #[test]
    fn concise_no_details_guidance_shorter() {
        let config = concise_config();
        let status = analyze_spec(None, false, false);
        let guidance = status.guidance(&config).unwrap();
        assert!(guidance.contains("~30-80 words"));
        assert!(guidance.contains("Key constraint"));
    }

    #[test]
    fn concise_feature_set_guidance() {
        let config = concise_config();
        let status = analyze_spec(None, true, false);
        let guidance = status.guidance(&config).unwrap();
        assert!(guidance.contains("brief architectural note"));
    }

    #[test]
    fn concise_project_guidance() {
        let config = concise_config();
        let status = analyze_spec(None, true, true);
        let guidance = status.guidance(&config).unwrap();
        assert!(guidance.contains("brief note"));
    }

    // --- Thorough level tests ---

    #[test]
    fn thorough_leaf_higher_warning_threshold() {
        let config = thorough_config();
        // 25 words — above standard (20) but below thorough (40)
        let status = analyze_spec(
            Some("Handle login with email and password. Validate credentials against the database. Return a JWT on success. Rate limit to five attempts per minute. Return 401 on failure."),
            false,
            false,
        );
        assert!(status.has_warnings(&config));
    }

    #[test]
    fn thorough_no_details_guidance_structured() {
        let config = thorough_config();
        let status = analyze_spec(None, false, false);
        let guidance = status.guidance(&config).unwrap();
        assert!(guidance.contains("~100-300 words"));
        assert!(guidance.contains("Story"));
        assert!(guidance.contains("Acceptance criteria"));
    }

    #[test]
    fn thorough_feature_set_guidance() {
        let config = thorough_config();
        let status = analyze_spec(None, true, false);
        let guidance = status.guidance(&config).unwrap();
        assert!(guidance.contains("detailed design rationale"));
        assert!(guidance.contains("alternatives considered"));
    }

    #[test]
    fn thorough_project_guidance() {
        let config = thorough_config();
        let status = analyze_spec(None, true, true);
        let guidance = status.guidance(&config).unwrap();
        assert!(guidance.contains("Domain terminology"));
        assert!(guidance.contains("alternatives considered"));
    }
}
