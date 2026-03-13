//! Parses GitHub issue bodies and extracts Manifest metadata.
//!
//! Issue bodies follow a structured format with HTML comments carrying
//! Manifest metadata (invisible in GitHub's UI). The parser extracts:
//!
//! - `manifest:id` — UUID for round-trip fidelity
//! - `manifest:feature` — marker identifying this as a Manifest-managed issue
//! - `details` — the main issue body (minus metadata and desired_details)
//! - `desired_details` — content inside the `<details>` block
//! - `details_summary` — first paragraph before the first `---`

use uuid::Uuid;

use crate::models::FeatureId;

/// Metadata extracted from a GitHub issue body.
#[derive(Debug, Clone, Default)]
pub struct IssueMetadata {
    /// The Manifest feature UUID, if present.
    pub manifest_id: Option<FeatureId>,
    /// Whether this issue is Manifest-managed (has `<!-- manifest:feature -->` comment).
    pub is_manifest_issue: bool,
    /// The full feature details (body minus metadata comments and desired_details block).
    pub details: Option<String>,
    /// The desired details from the `<details>` block, if present.
    pub desired_details: Option<String>,
    /// The summary — first paragraph before the first `---`.
    pub details_summary: Option<String>,
}

/// Parse a GitHub issue body and extract Manifest metadata.
pub fn parse_issue_body(body: &str) -> IssueMetadata {
    let mut meta = IssueMetadata::default();

    // Check for manifest:feature marker
    meta.is_manifest_issue = body.contains("<!-- manifest:feature -->");

    // Extract manifest:id
    meta.manifest_id = extract_manifest_id(body);

    // Extract desired_details from <details> block
    meta.desired_details = extract_desired_details(body);

    // Build details by removing metadata comments and desired_details block
    let details = strip_metadata(body);
    let details = strip_desired_details_block(&details);
    let trimmed = details.trim();

    if !trimmed.is_empty() {
        meta.details = Some(trimmed.to_string());
    }

    // Extract summary (text before first ---)
    if let Some(ref details_text) = meta.details {
        meta.details_summary = extract_summary(details_text);
    }

    meta
}

/// Extract the UUID from `<!-- manifest:id:UUID -->`.
fn extract_manifest_id(body: &str) -> Option<FeatureId> {
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("<!-- manifest:id:") {
            if let Some(uuid_str) = rest.strip_suffix(" -->") {
                if let Ok(uuid) = Uuid::parse_str(uuid_str.trim()) {
                    return Some(FeatureId::from(uuid));
                }
            }
        }
    }
    None
}

/// Extract desired details from `<details><summary>Desired state</summary>...</details>`.
fn extract_desired_details(body: &str) -> Option<String> {
    // Find the <details> block containing "Desired state"
    let lower = body.to_lowercase();
    let details_start = lower.find("<details>")?;

    // Find the matching </details>
    let details_end = lower[details_start..].find("</details>")?;
    let block = &body[details_start..details_start + details_end + "</details>".len()];

    // Check it contains "Desired state" in the summary
    if !block.to_lowercase().contains("desired state") {
        return None;
    }

    // Extract content after </summary> and before </details>
    let content_start = block.find("</summary>")?;
    let content = &block[content_start + "</summary>".len()..block.len() - "</details>".len()];
    let trimmed = content.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Remove manifest metadata HTML comments from the body.
fn strip_metadata(body: &str) -> String {
    body.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("<!-- manifest:") || !trimmed.ends_with(" -->")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Remove the entire `<details>..Desired state..</details>` block.
fn strip_desired_details_block(body: &str) -> String {
    let lower = body.to_lowercase();

    let Some(details_start) = lower.find("<details>") else {
        return body.to_string();
    };

    let Some(rel_end) = lower[details_start..].find("</details>") else {
        return body.to_string();
    };

    let details_end = details_start + rel_end + "</details>".len();
    let block = &body[details_start..details_end];

    // Only strip if it's the "Desired state" block
    if !block.to_lowercase().contains("desired state") {
        return body.to_string();
    }

    let mut result = String::with_capacity(body.len());
    result.push_str(&body[..details_start]);
    result.push_str(&body[details_end..]);
    result
}

/// Extract the summary — text before the first `---` separator.
fn extract_summary(details: &str) -> Option<String> {
    let parts: Vec<&str> = details.splitn(2, "\n---\n").collect();
    if parts.len() == 2 {
        let summary = parts[0].trim();
        if !summary.is_empty() {
            return Some(summary.to_string());
        }
    }
    None
}

/// Render an issue body from Manifest feature data.
///
/// Produces the structured format that `parse_issue_body` can round-trip.
pub fn render_issue_body(
    manifest_id: &FeatureId,
    details: Option<&str>,
    desired_details: Option<&str>,
) -> String {
    let mut body = String::new();

    body.push_str("<!-- manifest:feature -->\n");
    body.push_str(&format!("<!-- manifest:id:{manifest_id} -->\n"));
    body.push('\n');

    if let Some(d) = details {
        body.push_str(d);
        body.push('\n');
    }

    if let Some(dd) = desired_details {
        body.push_str("\n<details>\n<summary>Desired state</summary>\n\n");
        body.push_str(dd);
        body.push_str("\n\n</details>\n");
    }

    body
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_BODY: &str = r#"<!-- manifest:feature -->
<!-- manifest:id:550e8400-e29b-41d4-a716-446655440000 -->

As a user, I can log in via GitHub OAuth so that I don't need a separate password.

---

- [x] GitHub OAuth button on login page
- [x] Callback handler exchanges code for token
- [ ] Auto-create account on first OAuth login

<details>
<summary>Desired state</summary>

Full OAuth 2.0 PKCE flow with automatic account provisioning,
refresh token rotation, and session management.

</details>"#;

    #[test]
    fn parse_detects_manifest_issue() {
        let meta = parse_issue_body(SAMPLE_BODY);
        assert!(meta.is_manifest_issue);
    }

    #[test]
    fn parse_extracts_manifest_id() {
        let meta = parse_issue_body(SAMPLE_BODY);
        let id = meta.manifest_id.unwrap();
        assert_eq!(id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn parse_extracts_desired_details() {
        let meta = parse_issue_body(SAMPLE_BODY);
        let desired = meta.desired_details.unwrap();
        assert!(desired.contains("Full OAuth 2.0 PKCE flow"));
        assert!(desired.contains("session management"));
    }

    #[test]
    fn parse_extracts_details_without_metadata_or_desired() {
        let meta = parse_issue_body(SAMPLE_BODY);
        let details = meta.details.unwrap();
        assert!(details.contains("As a user, I can log in"));
        assert!(details.contains("Auto-create account"));
        assert!(!details.contains("manifest:feature"));
        assert!(!details.contains("manifest:id"));
        assert!(!details.contains("<details>"));
        assert!(!details.contains("Full OAuth 2.0 PKCE flow"));
    }

    #[test]
    fn parse_extracts_summary() {
        let meta = parse_issue_body(SAMPLE_BODY);
        let summary = meta.details_summary.unwrap();
        assert!(summary.contains("As a user, I can log in"));
        assert!(!summary.contains("GitHub OAuth button"));
    }

    #[test]
    fn parse_handles_empty_body() {
        let meta = parse_issue_body("");
        assert!(!meta.is_manifest_issue);
        assert!(meta.manifest_id.is_none());
        assert!(meta.details.is_none());
        assert!(meta.desired_details.is_none());
        assert!(meta.details_summary.is_none());
    }

    #[test]
    fn parse_handles_body_without_metadata() {
        let body = "This is a regular GitHub issue.\n\nNothing special here.";
        let meta = parse_issue_body(body);
        assert!(!meta.is_manifest_issue);
        assert!(meta.manifest_id.is_none());
        assert_eq!(meta.details.as_deref(), Some(body));
    }

    #[test]
    fn parse_handles_body_without_desired_details() {
        let body = "<!-- manifest:feature -->\n<!-- manifest:id:550e8400-e29b-41d4-a716-446655440000 -->\n\nSome details here.";
        let meta = parse_issue_body(body);
        assert!(meta.is_manifest_issue);
        assert!(meta.desired_details.is_none());
        assert_eq!(meta.details.as_deref(), Some("Some details here."));
    }

    #[test]
    fn render_and_parse_roundtrip() {
        let id = FeatureId::from(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap());
        let details = "As a user, I can do things.\n\n---\n\n- [ ] Criterion 1";
        let desired = "Improved version of things.";

        let rendered = render_issue_body(&id, Some(details), Some(desired));
        let parsed = parse_issue_body(&rendered);

        assert!(parsed.is_manifest_issue);
        assert_eq!(parsed.manifest_id.unwrap(), id);
        assert!(parsed
            .details
            .unwrap()
            .contains("As a user, I can do things"));
        assert!(parsed.desired_details.unwrap().contains("Improved version"));
    }

    #[test]
    fn render_without_desired_details() {
        let id = FeatureId::from(Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap());
        let rendered = render_issue_body(&id, Some("Just details"), None);
        assert!(!rendered.contains("<details>"));
        assert!(rendered.contains("Just details"));
    }
}
