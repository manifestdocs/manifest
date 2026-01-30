//! Spec analysis for MCP tool responses.
//!
//! Pure analysis — no I/O, no database. Checks the `details` markdown field
//! for structured sections (story, acceptance criteria, constraints).

/// Status of a feature's specification completeness.
pub struct SpecStatus {
    pub has_details: bool,
    pub has_story: bool,
    pub has_acceptance_criteria: bool,
    pub has_constraints: bool,
    pub is_parent: bool,
}

impl SpecStatus {
    /// True when a leaf feature has no details at all — blocks `start_feature`.
    pub fn should_block(&self) -> bool {
        !self.is_parent && !self.has_details
    }

    /// True when details exist but acceptance criteria are missing (leaf only).
    pub fn has_warnings(&self) -> bool {
        !self.is_parent && self.has_details && !self.has_acceptance_criteria
    }

    /// One-line status string for MCP responses.
    pub fn summary(&self) -> String {
        if self.is_parent {
            return "Spec: parent feature (exempt from spec requirements)".to_string();
        }
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

    /// Actionable guidance when the spec is incomplete. `None` when complete.
    pub fn guidance(&self) -> Option<String> {
        if self.is_parent || (self.has_details && self.has_acceptance_criteria) {
            return None;
        }

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

        // Has details but missing AC
        Some(
            "Specification exists but is missing acceptance criteria. \
             Consider adding a '## Acceptance Criteria' section or \
             Given/When/Then scenarios before implementation."
                .to_string(),
        )
    }
}

/// Analyze a feature's details for structured spec sections.
///
/// Detection heuristics (case-insensitive):
/// - **Story**: contains `"as a "` or has `## Story` header
/// - **Acceptance Criteria**: has `## Acceptance Criteria` header, OR contains
///   all three of `given`/`when`/`then`
/// - **Constraints**: has `## Constraints` header
pub fn analyze_spec(details: Option<&str>, has_children: bool) -> SpecStatus {
    let text = details.unwrap_or("").trim();
    let has_details = !text.is_empty();
    let lower = text.to_lowercase();

    let has_story = lower.contains("as a ") || lower.contains("## story");

    let has_acceptance_criteria = lower.contains("## acceptance criteria")
        || (lower.contains("given") && lower.contains("when") && lower.contains("then"));

    let has_constraints = lower.contains("## constraints");

    SpecStatus {
        has_details,
        has_story,
        has_acceptance_criteria,
        has_constraints,
        is_parent: has_children,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_details_should_block() {
        let status = analyze_spec(None, false);
        assert!(status.should_block());
        assert!(!status.has_warnings());
        assert!(status.guidance().is_some());
    }

    #[test]
    fn whitespace_only_should_block() {
        let status = analyze_spec(Some("   \n\t  "), false);
        assert!(status.should_block());
    }

    #[test]
    fn parent_features_never_block() {
        let status = analyze_spec(None, true);
        assert!(!status.should_block());
        assert!(!status.has_warnings());
        assert!(status.guidance().is_none());
    }

    #[test]
    fn parent_with_no_details_no_warnings() {
        let status = analyze_spec(Some("Architecture notes"), true);
        assert!(!status.should_block());
        assert!(!status.has_warnings());
    }

    #[test]
    fn details_without_ac_warns() {
        let status = analyze_spec(Some("As a user, I can do things."), false);
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

        let status = analyze_spec(Some(spec), false);
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
        let status = analyze_spec(Some(spec), false);
        assert!(status.has_acceptance_criteria);
        assert!(!status.has_warnings());
    }

    #[test]
    fn summary_shows_checks() {
        let status = analyze_spec(Some("As a user, I can do things."), false);
        assert!(status.summary().contains("story ✓"));
        assert!(status.summary().contains("acceptance criteria ✗"));
    }

    #[test]
    fn summary_parent_exempt() {
        let status = analyze_spec(None, true);
        assert!(status.summary().contains("exempt"));
    }

    #[test]
    fn summary_no_details() {
        let status = analyze_spec(None, false);
        assert_eq!(status.summary(), "Spec: no details");
    }

    #[test]
    fn case_insensitive_headers() {
        let spec =
            "## STORY\nAs A developer\n## ACCEPTANCE CRITERIA\n- test\n## CONSTRAINTS\n- none";
        let status = analyze_spec(Some(spec), false);
        assert!(status.has_story);
        assert!(status.has_acceptance_criteria);
        assert!(status.has_constraints);
    }
}
