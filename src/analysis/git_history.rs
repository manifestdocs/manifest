//! Git history analysis for feature extraction.
//!
//! Extracts feature-relevant information from git commit history:
//! - Commits with feat:/fix:/refactor: prefixes or "Add"/"Implement" patterns
//! - Files that frequently change together (co-change clusters)
//! - Deleted files (potential deprecations)

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;

use chrono::{DateTime, Utc};

/// Analysis results from git history.
#[derive(Debug, Clone, Default)]
pub struct GitAnalysis {
    /// Commits that appear to introduce features.
    pub feature_commits: Vec<FeatureCommit>,
    /// Files that frequently change together.
    pub file_clusters: Vec<FileCluster>,
    /// Files that were deleted (potential deprecations).
    pub deletions: Vec<Deletion>,
    /// Current branch name.
    pub branch: Option<String>,
    /// Current commit SHA (short).
    pub head_sha: Option<String>,
}

/// A commit that introduces or modifies a feature.
#[derive(Debug, Clone)]
pub struct FeatureCommit {
    /// Short SHA.
    pub sha: String,
    /// Full commit message (first line).
    pub message: String,
    /// Extracted feature name from the commit message.
    pub feature_name: String,
    /// Type of commit (feat, fix, refactor, add, implement, etc.).
    pub commit_type: CommitType,
    /// Files changed in this commit.
    pub files: Vec<String>,
    /// Commit timestamp.
    pub date: DateTime<Utc>,
}

/// Type of commit based on conventional commit prefix or pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitType {
    /// feat: prefix or "Add" pattern
    Feature,
    /// fix: prefix
    Fix,
    /// refactor: prefix
    Refactor,
    /// remove/delete pattern
    Remove,
    /// Other patterns (implement, update, etc.)
    Other,
}

impl CommitType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CommitType::Feature => "feat",
            CommitType::Fix => "fix",
            CommitType::Refactor => "refactor",
            CommitType::Remove => "remove",
            CommitType::Other => "other",
        }
    }
}

/// Files that frequently change together.
#[derive(Debug, Clone)]
pub struct FileCluster {
    /// Files in this cluster.
    pub files: Vec<String>,
    /// How many commits these files appeared together.
    pub commit_count: u32,
}

/// A file that was deleted.
#[derive(Debug, Clone)]
pub struct Deletion {
    /// Path of the deleted file.
    pub path: String,
    /// Commit where it was deleted.
    pub sha: String,
    /// Commit message.
    pub message: String,
    /// When it was deleted.
    pub date: DateTime<Utc>,
}

/// Analyze git history for a repository.
///
/// # Arguments
/// * `root` - Path to the git repository root
/// * `since` - Optional git ref (tag, commit, or date) to start from
/// * `max_commits` - Maximum number of commits to analyze
pub fn analyze_git_history(root: &Path, since: Option<&str>, max_commits: u32) -> GitAnalysis {
    let mut analysis = GitAnalysis::default();

    // Get current branch and HEAD
    analysis.branch = get_current_branch(root);
    analysis.head_sha = get_head_sha(root);

    // Get commit log
    let commits = get_commit_log(root, since, max_commits);

    // Extract feature commits
    for (sha, message, date, files) in commits.iter() {
        if let Some((feature_name, commit_type)) = parse_commit_message(message) {
            analysis.feature_commits.push(FeatureCommit {
                sha: sha.clone(),
                message: message.clone(),
                feature_name,
                commit_type,
                files: files.clone(),
                date: *date,
            });
        }
    }

    // Find file clusters (files that change together)
    analysis.file_clusters = find_file_clusters(&commits);

    // Find deletions
    analysis.deletions = find_deletions(root, since);

    analysis
}

/// Get the current branch name.
fn get_current_branch(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Get the HEAD commit SHA (short form).
fn get_head_sha(root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(root)
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Get commit log with files changed.
fn get_commit_log(
    root: &Path,
    since: Option<&str>,
    max_commits: u32,
) -> Vec<(String, String, DateTime<Utc>, Vec<String>)> {
    // Format: SHA|message|timestamp
    // followed by list of files
    let mut args = vec![
        "log".to_string(),
        "--pretty=format:%h|%s|%aI".to_string(),
        "--name-only".to_string(),
        format!("-n{}", max_commits),
    ];

    if let Some(ref_spec) = since {
        args.push(format!("{}..HEAD", ref_spec));
    }

    let output = match Command::new("git").args(&args).current_dir(root).output() {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_commit_log(&stdout)
}

/// Parse the git log output into structured data.
fn parse_commit_log(output: &str) -> Vec<(String, String, DateTime<Utc>, Vec<String>)> {
    let mut commits = Vec::new();
    let mut current_commit: Option<(String, String, DateTime<Utc>)> = None;
    let mut current_files = Vec::new();

    for line in output.lines() {
        if line.is_empty() {
            continue;
        }

        // Check if this is a commit line (contains |)
        if line.contains('|') {
            // Save previous commit if any
            if let Some((sha, message, date)) = current_commit.take() {
                commits.push((sha, message, date, std::mem::take(&mut current_files)));
            }

            // Parse new commit line
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() == 3 {
                let sha = parts[0].to_string();
                let message = parts[1].to_string();
                let date = DateTime::parse_from_rfc3339(parts[2])
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
                current_commit = Some((sha, message, date));
            }
        } else {
            // This is a file path
            current_files.push(line.to_string());
        }
    }

    // Don't forget the last commit
    if let Some((sha, message, date)) = current_commit {
        commits.push((sha, message, date, current_files));
    }

    commits
}

/// Parse a commit message to extract feature name and type.
fn parse_commit_message(message: &str) -> Option<(String, CommitType)> {
    let message = message.trim();

    // Try conventional commit format first: type(scope): description
    // or type: description
    if let Some(rest) = message.strip_prefix("feat") {
        let feature_name = extract_after_colon(rest);
        return Some((feature_name, CommitType::Feature));
    }
    if let Some(rest) = message.strip_prefix("fix") {
        let feature_name = extract_after_colon(rest);
        return Some((feature_name, CommitType::Fix));
    }
    if let Some(rest) = message.strip_prefix("refactor") {
        let feature_name = extract_after_colon(rest);
        return Some((feature_name, CommitType::Refactor));
    }

    // Try informal patterns
    let lower = message.to_lowercase();
    if lower.starts_with("add ") {
        let feature_name = message[4..].trim().to_string();
        return Some((feature_name, CommitType::Feature));
    }
    if lower.starts_with("implement ") {
        let feature_name = message[10..].trim().to_string();
        return Some((feature_name, CommitType::Feature));
    }
    if lower.starts_with("remove ") || lower.starts_with("delete ") {
        let feature_name = message
            .split_at(message.find(' ').unwrap_or(0))
            .1
            .trim()
            .to_string();
        return Some((feature_name, CommitType::Remove));
    }
    if lower.starts_with("update ") || lower.starts_with("improve ") {
        let feature_name = message
            .split_at(message.find(' ').unwrap_or(0))
            .1
            .trim()
            .to_string();
        return Some((feature_name, CommitType::Other));
    }

    None
}

/// Extract feature name after colon in conventional commit.
fn extract_after_colon(s: &str) -> String {
    // Handle optional scope: (scope): description or : description
    let s = s.trim_start();

    // Skip scope if present: (something)
    let s = if s.starts_with('(') {
        s.find(')').map(|i| &s[i + 1..]).unwrap_or(s)
    } else {
        s
    };

    // Skip colon and any whitespace
    let s = s.trim_start_matches(':').trim_start_matches('!').trim();

    s.to_string()
}

/// Find clusters of files that frequently change together.
fn find_file_clusters(
    commits: &[(String, String, DateTime<Utc>, Vec<String>)],
) -> Vec<FileCluster> {
    // Count how often pairs of files appear together
    let mut pair_counts: HashMap<(String, String), u32> = HashMap::new();

    for (_, _, _, files) in commits {
        // Only consider commits with 2-10 files (likely related changes)
        if files.len() < 2 || files.len() > 10 {
            continue;
        }

        // Count pairs
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                let a = &files[i];
                let b = &files[j];
                // Normalize order
                let pair = if a < b {
                    (a.clone(), b.clone())
                } else {
                    (b.clone(), a.clone())
                };
                *pair_counts.entry(pair).or_insert(0) += 1;
            }
        }
    }

    // Find clusters where pairs appear together >= 3 times
    let threshold = 3;
    let mut clusters: Vec<FileCluster> = Vec::new();
    let mut used: HashSet<String> = HashSet::new();

    // Sort by count descending
    let mut pairs: Vec<_> = pair_counts.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1));

    for ((a, b), count) in pairs {
        if count < threshold {
            continue;
        }
        if used.contains(&a) || used.contains(&b) {
            continue;
        }

        used.insert(a.clone());
        used.insert(b.clone());
        clusters.push(FileCluster {
            files: vec![a, b],
            commit_count: count,
        });

        // Limit clusters to avoid noise
        if clusters.len() >= 20 {
            break;
        }
    }

    clusters
}

/// Find files that were deleted.
fn find_deletions(root: &Path, since: Option<&str>) -> Vec<Deletion> {
    let mut args = vec![
        "log".to_string(),
        "--diff-filter=D".to_string(),
        "--pretty=format:%h|%s|%aI".to_string(),
        "--name-only".to_string(),
        "-n100".to_string(),
    ];

    if let Some(ref_spec) = since {
        args.push(format!("{}..HEAD", ref_spec));
    }

    let output = match Command::new("git").args(&args).current_dir(root).output() {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let commits = parse_commit_log(&stdout);

    let mut deletions = Vec::new();
    for (sha, message, date, files) in commits {
        for file in files {
            // Only track source file deletions
            if is_source_file(&file) {
                deletions.push(Deletion {
                    path: file,
                    sha: sha.clone(),
                    message: message.clone(),
                    date,
                });
            }
        }
    }

    deletions
}

/// Check if a file is a source file (not config, docs, etc.).
fn is_source_file(path: &str) -> bool {
    let source_extensions = [
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "fs", "fsi", "fsx", "rb", "java", "kt", "cs",
        "cpp", "c", "h", "hpp", "swift", "scala",
    ];

    path.split('.')
        .last()
        .map(|ext| source_extensions.contains(&ext))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_conventional_commit() {
        let (name, typ) = parse_commit_message("feat: add user authentication").unwrap();
        assert_eq!(name, "add user authentication");
        assert_eq!(typ, CommitType::Feature);

        let (name, typ) = parse_commit_message("feat(auth): implement OAuth").unwrap();
        assert_eq!(name, "implement OAuth");
        assert_eq!(typ, CommitType::Feature);

        let (name, typ) = parse_commit_message("fix: resolve race condition").unwrap();
        assert_eq!(name, "resolve race condition");
        assert_eq!(typ, CommitType::Fix);
    }

    #[test]
    fn test_parse_informal_commit() {
        let (name, typ) = parse_commit_message("Add user registration").unwrap();
        assert_eq!(name, "user registration");
        assert_eq!(typ, CommitType::Feature);

        let (name, typ) = parse_commit_message("Implement caching layer").unwrap();
        assert_eq!(name, "caching layer");
        assert_eq!(typ, CommitType::Feature);

        let (name, typ) = parse_commit_message("Remove deprecated API").unwrap();
        assert_eq!(name, "deprecated API");
        assert_eq!(typ, CommitType::Remove);
    }

    #[test]
    fn test_non_feature_commit() {
        assert!(parse_commit_message("Merge branch 'main'").is_none());
        assert!(parse_commit_message("WIP").is_none());
        assert!(parse_commit_message("bump version").is_none());
    }

    #[test]
    fn test_is_source_file() {
        assert!(is_source_file("src/main.rs"));
        assert!(is_source_file("lib/auth.ts"));
        assert!(!is_source_file("README.md"));
        assert!(!is_source_file("Cargo.toml"));
        assert!(!is_source_file(".gitignore"));
    }
}
