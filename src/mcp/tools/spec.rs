//! Spec analysis for MCP tool responses.
//!
//! Pure analysis — no I/O, no database. Checks the `details` field for
//! non-trivial content. Provides tier-aware guidance for project,
//! feature set, and leaf features.

/// Project-level configuration for spec guidance.
#[derive(Debug, Clone, Default)]
pub struct SpecConfig {
    /// The default template content for this project, if one exists.
    pub default_template: Option<String>,
}

impl SpecConfig {
    /// Generate TDD guidance based on testable criteria count.
    pub fn testing_guidance(&self, testable_criteria_count: usize) -> Option<String> {
        if testable_criteria_count > 0 {
            Some(format!(
                "REQUIRED: Write failing tests for the {} testable criteria BEFORE writing any implementation. \
                 Call prove_feature with the failing results, then implement to make them pass.",
                testable_criteria_count,
            ))
        } else {
            Some(
                "REQUIRED: Write failing tests that encode the expected behavior BEFORE writing any implementation. \
                 Call prove_feature with the failing results, then implement to make them pass."
                    .to_string(),
            )
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
    /// Number of testable criteria detected in the spec.
    pub testable_criteria_count: usize,
}

impl SpecStatus {
    /// True when a leaf feature has no details at all — blocks `start_feature`.
    pub fn should_block(&self) -> bool {
        self.tier == FeatureTier::Leaf && !self.has_details
    }

    /// True when spec-level warnings apply (missing testable criteria for leaves with details).
    pub fn has_warnings(&self) -> bool {
        self.tier == FeatureTier::Leaf && self.has_details && self.testable_criteria_count == 0
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
    pub fn guidance(&self, config: &SpecConfig) -> Option<String> {
        match self.tier {
            FeatureTier::Project => self.project_guidance(),
            FeatureTier::FeatureSet => self.feature_set_guidance(),
            FeatureTier::Leaf => self.leaf_guidance(config),
        }
    }

    fn project_guidance(&self) -> Option<String> {
        if self.has_details {
            return None;
        }
        Some(
            "This is the project root \u{2014} all agents read this before working on any feature. \
             Add project-wide instructions: tech stack, conventions, architectural decisions, \
             and security boundaries."
                .to_string(),
        )
    }

    fn feature_set_guidance(&self) -> Option<String> {
        if self.has_details {
            return None;
        }
        Some(
            "This feature set has no shared context. Consider adding architectural decisions, \
             shared patterns, or constraints that apply to all child features."
                .to_string(),
        )
    }

    fn leaf_guidance(&self, config: &SpecConfig) -> Option<String> {
        if !self.has_details {
            // No spec — provide template content or generic guidance
            if let Some(template) = &config.default_template {
                return Some(format!(
                    "This feature has no specification. Use update_feature to add details before starting.\n\n\
                     Use this template as a starting point:\n\n{}\n\n\
                     Do NOT include: file paths, codebase structure, or implementation approach \u{2014} agents discover these from code.",
                    template,
                ));
            }
            return Some(
                "This feature has no specification. Use update_feature to add details before starting.\n\n\
                 Write a focused specification with:\n\
                 1. User story: As a [user], I can [capability] so that [benefit].\n\
                 2. Brief context: key behavior, constraints, or edge cases.\n\
                 3. Acceptance criteria as checkbox items \u{2014} each verifiable in a test.\n\n\
                 Do NOT include: file paths, codebase structure, or implementation approach \u{2014} agents discover these from code."
                    .to_string(),
            );
        }

        // Warn when no testable criteria are present
        if self.testable_criteria_count == 0 {
            return Some(
                "No testable criteria detected. Consider adding acceptance criteria as \
                 checkbox items with concrete assertions verifiable in specs and tests \
                 (e.g. \"returns 200\", \"creates a record\", \"rejects invalid input\"). \
                 Each criterion maps directly to a test case."
                    .to_string(),
            );
        }

        None
    }
}

/// Count testable criteria in a spec's text.
///
/// Detects three patterns:
/// 1. Gherkin `Given`/`When`/`Then` lines — each `Then` counts as one criterion
/// 2. Markdown checkbox items with assertion language (returns, creates, rejects, etc.)
/// 3. Numbered list items with measurable outcomes
///
/// Returns the total count of testable criteria found.
pub fn count_testable_criteria(text: &str) -> usize {
    let mut count = 0;

    // Assertion verbs that indicate testable outcomes
    let assertion_verbs = [
        "returns",
        "creates",
        "deletes",
        "updates",
        "rejects",
        "accepts",
        "validates",
        "sends",
        "receives",
        "fails",
        "succeeds",
        "throws",
        "emits",
        "sets",
        "clears",
        "adds",
        "removes",
        "blocks",
        "allows",
        "denies",
        "redirects",
        "renders",
        "displays",
        "hides",
        "shows",
        "stores",
        "logs",
        "triggers",
        "produces",
        "generates",
        "contains",
        "includes",
        "excludes",
        "matches",
        "equals",
    ];

    for line in text.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();

        // Pattern 1: Gherkin Then lines (each Then is a testable assertion)
        if lower.starts_with("then ") || lower.starts_with("- then ") {
            count += 1;
            continue;
        }

        // Pattern 2: Checkbox items with assertion verbs
        // Matches: - [ ] returns 200, - [x] creates a record
        if (trimmed.starts_with("- [ ] ") || trimmed.starts_with("- [x] "))
            && assertion_verbs.iter().any(|v| lower.contains(v))
        {
            count += 1;
            continue;
        }

        // Pattern 3: Numbered items with assertion verbs (e.g. "1. Returns 200")
        if let Some(rest) = strip_numbered_prefix(trimmed) {
            if assertion_verbs
                .iter()
                .any(|v| rest.to_ascii_lowercase().contains(v))
            {
                count += 1;
                continue;
            }
        }

        // Pattern 2b: Plain dash list items with assertion verbs in AC sections
        if trimmed.starts_with("- ")
            && !trimmed.starts_with("- [ ")
            && assertion_verbs.iter().any(|v| lower.contains(v))
        {
            count += 1;
        }
    }

    count
}

/// Strip a numbered list prefix like "1. ", "2) ", returning the rest.
fn strip_numbered_prefix(s: &str) -> Option<&str> {
    let mut chars = s.chars();
    // Must start with a digit
    if !chars.next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    // Consume remaining digits
    let rest = chars.as_str();
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
    // Must be followed by ". " or ") "
    rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") "))
}

/// Analyze a feature's details for specification completeness.
///
/// Also counts testable criteria for leaf features.
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

    let testable_criteria_count = if tier == FeatureTier::Leaf && has_details {
        count_testable_criteria(text)
    } else {
        0
    };

    SpecStatus {
        has_details,
        word_count,
        tier,
        testable_criteria_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> SpecConfig {
        SpecConfig::default()
    }

    fn config_with_template() -> SpecConfig {
        SpecConfig {
            default_template: Some("## User Story\n\nAs a [user], I can [capability].".to_string()),
        }
    }

    // --- Leaf feature tests ---

    #[test]
    fn empty_details_should_block() {
        let config = default_config();
        let status = analyze_spec(None, false, false);
        assert!(status.should_block());
        assert!(!status.has_warnings());
        assert!(status.guidance(&config).is_some());
    }

    #[test]
    fn whitespace_only_should_block() {
        let status = analyze_spec(Some("   \n\t  "), false, false);
        assert!(status.should_block());
    }

    #[test]
    fn sufficient_details_no_warnings() {
        let config = default_config();
        let spec = "Accepts an email and password, validates credentials against the database, \
                     returns a JWT on success. Must rate-limit to 5 attempts per minute. \
                     Returns 401 with generic error on failure.\n- [ ] returns JWT on success";
        let status = analyze_spec(Some(spec), false, false);
        assert!(!status.should_block());
        assert!(!status.has_warnings());
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
    fn no_details_guidance_mentions_user_story() {
        let config = default_config();
        let status = analyze_spec(None, false, false);
        let guidance = status.guidance(&config).unwrap();
        assert!(guidance.contains("User story"));
    }

    #[test]
    fn no_details_guidance_with_template() {
        let config = config_with_template();
        let status = analyze_spec(None, false, false);
        let guidance = status.guidance(&config).unwrap();
        assert!(guidance.contains("## User Story"));
        assert!(guidance.contains("template"));
    }

    #[test]
    fn no_details_guidance_discourages_file_paths() {
        let config = default_config();
        let status = analyze_spec(None, false, false);
        let guidance = status.guidance(&config).unwrap();
        assert!(guidance.contains("Do NOT include"));
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
        let status = analyze_spec(None, true, true);
        assert!(!status.should_block());
        assert!(!status.has_warnings());
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

    #[test]
    fn testing_guidance_always_emits_required_before() {
        let config = default_config();
        let guidance = config.testing_guidance(0).unwrap();
        assert!(guidance.contains("REQUIRED"));
        assert!(guidance.contains("BEFORE"));
        assert!(guidance.contains("prove_feature"));
    }

    #[test]
    fn testing_guidance_includes_criteria_count() {
        let config = default_config();
        let guidance = config.testing_guidance(5).unwrap();
        assert!(guidance.contains("5 testable criteria"));
    }

    #[test]
    fn testing_guidance_zero_criteria_no_count_in_text() {
        let config = default_config();
        let guidance = config.testing_guidance(0).unwrap();
        assert!(!guidance.contains("0 testable"));
    }

    // --- Testable criteria detection tests ---

    #[test]
    fn detects_gherkin_then_as_testable() {
        let spec = "Given a user exists\nWhen they log in\nThen they see the dashboard\nThen they receive a session cookie";
        assert_eq!(count_testable_criteria(spec), 2);
    }

    #[test]
    fn detects_checkbox_with_assertion_verbs() {
        let spec =
            "- [ ] returns 200 on success\n- [ ] creates a new record\n- [ ] rejects invalid input";
        assert_eq!(count_testable_criteria(spec), 3);
    }

    #[test]
    fn detects_checked_checkbox_items() {
        let spec = "- [x] returns 200 on success\n- [ ] creates a new record";
        assert_eq!(count_testable_criteria(spec), 2);
    }

    #[test]
    fn detects_numbered_items_with_assertions() {
        let spec =
            "1. Returns 200 on success\n2. Creates a database record\n3. Sends confirmation email";
        assert_eq!(count_testable_criteria(spec), 3);
    }

    #[test]
    fn detects_dash_list_items_with_assertions() {
        let spec = "- Returns 200 on valid request\n- Rejects malformed JSON\n- Logs the event";
        assert_eq!(count_testable_criteria(spec), 3);
    }

    #[test]
    fn ignores_non_assertion_items() {
        let spec = "- Use Redis for caching\n- Should be fast\n- Must be user-friendly";
        assert_eq!(count_testable_criteria(spec), 0);
    }

    #[test]
    fn mixed_testable_and_non_testable() {
        let spec = "Goal: handle user login\n\n\
                     - [ ] returns JWT on success\n\
                     - Use bcrypt for hashing\n\
                     - [ ] rejects expired tokens";
        assert_eq!(count_testable_criteria(spec), 2);
    }

    #[test]
    fn analyze_spec_populates_count_for_leaf() {
        let spec = "- [ ] returns 200\n- [ ] creates a record";
        let status = analyze_spec(Some(spec), false, false);
        assert_eq!(status.testable_criteria_count, 2);
    }

    #[test]
    fn analyze_spec_zero_count_for_feature_set() {
        let spec = "- [ ] returns 200\n- [ ] creates a record";
        let status = analyze_spec(Some(spec), true, false);
        assert_eq!(status.testable_criteria_count, 0);
    }

    #[test]
    fn no_testable_criteria_warns() {
        let config = default_config();
        let spec = "Handle user login with appropriate validation and security measures. \
                     The system should authenticate users against the database and manage \
                     sessions correctly with proper error handling throughout.";
        let status = analyze_spec(Some(spec), false, false);
        assert_eq!(status.testable_criteria_count, 0);
        assert!(status.has_warnings());
        let guidance = status.guidance(&config).unwrap();
        assert!(guidance.contains("No testable criteria"));
    }

    #[test]
    fn testable_criteria_present_no_warning() {
        let config = default_config();
        let spec = "Handle user login with appropriate validation and security measures \
                     for the authentication subsystem.\n\
                     - [ ] returns JWT on success\n\
                     - [ ] rejects invalid credentials";
        let status = analyze_spec(Some(spec), false, false);
        assert_eq!(status.testable_criteria_count, 2);
        assert!(!status.has_warnings());
        assert!(status.guidance(&config).is_none());
    }
}
