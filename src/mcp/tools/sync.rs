//! Sync MCP tool — reconcile feature tree with git history.
//!
//! Analyzes git commits since a reference point, compares against
//! the project's feature tree, and returns structured proposals for
//! the agent to review and apply using existing tools.

use std::path::Path;

use rmcp::{
    model::{CallToolResult, Content},
    ErrorData as McpError,
};

use crate::analysis::git_history::{self, CommitType, FeatureCommit};
use crate::mcp::client::ManifestClient;
use crate::mcp::types::SyncRequest;
use crate::models::FeatureState;

/// Maximum commits to analyze per directory.
const MAX_COMMITS: u32 = 200;

/// Run sync analysis: gather git history, match against feature tree, return proposals.
pub async fn sync(client: &ManifestClient, req: SyncRequest) -> Result<CallToolResult, McpError> {
    let project = client
        .get_project(req.project_id)
        .await
        .map_err(super::client_err)?;

    // Find git directories
    let git_dirs: Vec<&str> = project
        .directories
        .iter()
        .filter(|d| Path::new(&d.path).join(".git").exists())
        .map(|d| d.path.as_str())
        .collect();

    if git_dirs.is_empty() {
        return Err(McpError::invalid_params(
            "No git repositories found in project directories",
            None,
        ));
    }

    // Analyze git history across all directories
    let mut all_commits: Vec<FeatureCommit> = Vec::new();
    for dir in &git_dirs {
        let analysis =
            git_history::analyze_git_history(Path::new(dir), req.since.as_deref(), MAX_COMMITS);
        all_commits.extend(analysis.feature_commits);
    }

    if all_commits.is_empty() {
        let msg = match req.since.as_deref() {
            Some(since) => format!(
                "No feature-related commits found since '{}'. Looked for conventional commits (feat:, fix:) and patterns (Add, Implement, etc.).",
                since
            ),
            None => "No feature-related commits found. Looked for conventional commits (feat:, fix:) and patterns (Add, Implement, etc.).".to_string(),
        };
        return Ok(CallToolResult::success(vec![Content::text(msg)]));
    }

    // Fetch all features for the project
    let features = client
        .list_features(Some(req.project_id), None, None, None)
        .await
        .map_err(super::client_err)?;

    // Match commits against features
    let mut proposals: Vec<SyncProposal> = Vec::new();
    let mut matched_feature_ids: Vec<uuid::Uuid> = Vec::new();
    let mut unmatched_commits: Vec<&FeatureCommit> = Vec::new();

    for commit in &all_commits {
        // Skip non-feature commits (fixes, refactors) for matching
        if commit.commit_type == CommitType::Fix || commit.commit_type == CommitType::Refactor {
            continue;
        }

        let normalized_name = normalize(&commit.feature_name);
        if normalized_name.is_empty() {
            continue;
        }

        // Try to find a matching feature
        let matched = features.iter().find(|f| {
            let normalized_title = normalize(&f.title);
            titles_match(&normalized_name, &normalized_title)
        });

        match matched {
            Some(feature) => {
                let feature_id: uuid::Uuid = feature.id.into();
                // Avoid duplicate proposals for the same feature
                if matched_feature_ids.contains(&feature_id) {
                    // Add evidence to existing proposal
                    if let Some(p) = proposals
                        .iter_mut()
                        .find(|p| p.feature_id == Some(feature_id))
                    {
                        p.evidence.push(format_evidence(commit));
                    }
                    continue;
                }

                matched_feature_ids.push(feature_id);

                match feature.state {
                    FeatureState::Proposed => {
                        proposals.push(SyncProposal {
                            proposal_type: ProposalType::MarkImplemented,
                            feature_id: Some(feature_id),
                            feature_title: feature.title.clone(),
                            suggested_title: None,
                            suggested_parent_title: None,
                            evidence: vec![format_evidence(commit)],
                        });
                    }
                    FeatureState::Implemented => {
                        proposals.push(SyncProposal {
                            proposal_type: ProposalType::UpdateDetails,
                            feature_id: Some(feature_id),
                            feature_title: feature.title.clone(),
                            suggested_title: None,
                            suggested_parent_title: None,
                            evidence: vec![format_evidence(commit)],
                        });
                    }
                    _ => {
                        // InProgress, Blocked, Archived — no proposal needed
                    }
                }
            }
            None => {
                unmatched_commits.push(commit);
            }
        }
    }

    // Group unmatched feature commits into create proposals
    let mut create_groups: std::collections::HashMap<String, Vec<&FeatureCommit>> =
        std::collections::HashMap::new();
    for commit in &unmatched_commits {
        let key = normalize(&commit.feature_name);
        create_groups.entry(key).or_default().push(commit);
    }

    for commits in create_groups.values() {
        // Use the first commit's feature name as the suggested title
        let title = &commits[0].feature_name;
        // Try to guess a parent from file paths
        let parent_hint = guess_parent_from_files(commits, &features);

        proposals.push(SyncProposal {
            proposal_type: ProposalType::CreateFeature,
            feature_id: None,
            feature_title: String::new(),
            suggested_title: Some(title.clone()),
            suggested_parent_title: parent_hint,
            evidence: commits.iter().map(|c| format_evidence(c)).collect(),
        });
    }

    // Format output
    let total_commits = all_commits.len();
    let mark_count = proposals
        .iter()
        .filter(|p| p.proposal_type == ProposalType::MarkImplemented)
        .count();
    let update_count = proposals
        .iter()
        .filter(|p| p.proposal_type == ProposalType::UpdateDetails)
        .count();
    let create_count = proposals
        .iter()
        .filter(|p| p.proposal_type == ProposalType::CreateFeature)
        .count();

    let mut output = String::new();
    output.push_str(&format!(
        "# Sync Analysis\n\n**{} feature commits** analyzed across {} director{}\n\n",
        total_commits,
        git_dirs.len(),
        if git_dirs.len() == 1 { "y" } else { "ies" }
    ));

    if proposals.is_empty() {
        output.push_str("No sync proposals — feature tree appears up to date.\n");
        return Ok(CallToolResult::success(vec![Content::text(output)]));
    }

    output.push_str(&format!(
        "**{} proposals**: {} mark implemented, {} update details, {} create new\n\n",
        proposals.len(),
        mark_count,
        update_count,
        create_count
    ));

    // Group proposals by type
    if mark_count > 0 {
        output.push_str("## Mark as Implemented\n\n");
        output.push_str("These proposed features have matching commits and may be done:\n\n");
        for p in proposals
            .iter()
            .filter(|p| p.proposal_type == ProposalType::MarkImplemented)
        {
            output.push_str(&format!(
                "- **{}** ({})\n",
                p.feature_title,
                p.feature_id.map(|id| id.to_string()).unwrap_or_default()
            ));
            for e in &p.evidence {
                output.push_str(&format!("  - {}\n", e));
            }
        }
        output.push('\n');
    }

    if update_count > 0 {
        output.push_str("## Update Details\n\n");
        output.push_str(
            "These implemented features have recent commits — details may need updating:\n\n",
        );
        for p in proposals
            .iter()
            .filter(|p| p.proposal_type == ProposalType::UpdateDetails)
        {
            output.push_str(&format!(
                "- **{}** ({})\n",
                p.feature_title,
                p.feature_id.map(|id| id.to_string()).unwrap_or_default()
            ));
            for e in &p.evidence {
                output.push_str(&format!("  - {}\n", e));
            }
        }
        output.push('\n');
    }

    if create_count > 0 {
        output.push_str("## Create New Features\n\n");
        output.push_str("These commits describe capabilities not in the feature tree:\n\n");
        for p in proposals
            .iter()
            .filter(|p| p.proposal_type == ProposalType::CreateFeature)
        {
            let title = p.suggested_title.as_deref().unwrap_or("(unknown)");
            output.push_str(&format!("- **{}**", title));
            if let Some(ref parent) = p.suggested_parent_title {
                output.push_str(&format!(" (under {})", parent));
            }
            output.push('\n');
            for e in &p.evidence {
                output.push_str(&format!("  - {}\n", e));
            }
        }
        output.push('\n');
    }

    output.push_str("---\n\n");
    output.push_str("To apply these proposals, use the existing tools:\n");
    output.push_str("- `update_feature` with `state: 'implemented'` to mark features done\n");
    output.push_str("- `update_feature` with `details` to update documentation\n");
    output.push_str("- `create_feature` to add new capabilities to the tree\n");
    output.push_str("- `complete_feature` to record history with commit SHAs\n");

    Ok(CallToolResult::success(vec![Content::text(output)]))
}

// ============================================================
// Internal types
// ============================================================

#[derive(Debug, PartialEq)]
enum ProposalType {
    MarkImplemented,
    UpdateDetails,
    CreateFeature,
}

#[derive(Debug)]
struct SyncProposal {
    proposal_type: ProposalType,
    feature_id: Option<uuid::Uuid>,
    feature_title: String,
    suggested_title: Option<String>,
    suggested_parent_title: Option<String>,
    evidence: Vec<String>,
}

// ============================================================
// Matching helpers
// ============================================================

/// Normalize a string for comparison: lowercase, strip punctuation, collapse whitespace.
fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Check if two normalized titles match.
///
/// Uses substring matching and word overlap. Returns true if:
/// - One is a substring of the other, or
/// - They share >50% of their words
fn titles_match(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }

    // Exact match
    if a == b {
        return true;
    }

    // Substring match (either direction)
    if a.contains(b) || b.contains(a) {
        return true;
    }

    // Word overlap: >50% of the shorter set's words appear in the longer
    let words_a: std::collections::HashSet<&str> = a.split_whitespace().collect();
    let words_b: std::collections::HashSet<&str> = b.split_whitespace().collect();

    let overlap = words_a.intersection(&words_b).count();
    let min_len = words_a.len().min(words_b.len());

    if min_len == 0 {
        return false;
    }

    // At least 2 words must match, and they must be >50% of the shorter set
    overlap >= 2 && (overlap * 2) > min_len
}

/// Format a commit as evidence text.
fn format_evidence(commit: &FeatureCommit) -> String {
    format!("{}: {}", commit.sha, commit.message)
}

/// Try to guess a parent feature from commit file paths.
///
/// If all files share a common directory prefix, look for a feature
/// whose title matches that directory name.
fn guess_parent_from_files(
    commits: &[&FeatureCommit],
    features: &[crate::models::FeatureSummary],
) -> Option<String> {
    // Collect all file paths
    let files: Vec<&str> = commits
        .iter()
        .flat_map(|c| c.files.iter().map(|f| f.as_str()))
        .collect();

    if files.is_empty() {
        return None;
    }

    // Find common directory prefix
    let first_dir = files[0].rsplit('/').nth(1)?;
    let all_same_dir = files.iter().all(|f| {
        f.rsplit('/')
            .nth(1)
            .map(|d| d == first_dir)
            .unwrap_or(false)
    });

    if !all_same_dir {
        return None;
    }

    // Look for a feature whose title matches the directory name
    let normalized_dir = normalize(first_dir);
    features
        .iter()
        .find(|f| normalize(&f.title) == normalized_dir)
        .map(|f| f.title.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize() {
        assert_eq!(normalize("OAuth Login"), "oauth login");
        assert_eq!(normalize("user-authentication"), "user authentication");
        assert_eq!(normalize("  Multiple   Spaces  "), "multiple spaces");
        assert_eq!(normalize("feat: add something"), "feat add something");
    }

    #[test]
    fn test_titles_match_exact() {
        assert!(titles_match("oauth login", "oauth login"));
    }

    #[test]
    fn test_titles_match_substring() {
        assert!(titles_match("oauth login flow", "oauth login"));
        assert!(titles_match("oauth", "oauth login"));
    }

    #[test]
    fn test_titles_match_word_overlap() {
        assert!(titles_match(
            "user authentication system",
            "user authentication"
        ));
    }

    #[test]
    fn test_titles_no_match() {
        assert!(!titles_match("oauth login", "database migration"));
        assert!(!titles_match("", "something"));
        assert!(!titles_match("a", "b"));
    }

    #[test]
    fn test_titles_single_word_no_false_positive() {
        // Single word matches should work via substring
        assert!(titles_match("auth", "authentication"));
        // But unrelated single words should not
        assert!(!titles_match("auth", "router"));
    }
}
