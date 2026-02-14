//! Git helper functions for branch management in MCP tools.
//!
//! All operations use `std::process::Command` and return `Result<T, String>`
//! for clean error messages. These are best-effort — callers should treat
//! failures as warnings rather than hard errors.

use std::process::Command;

/// Check if a directory is a git repository.
pub fn is_git_repo(dir: &str) -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(dir)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get the current branch name.
pub fn current_branch(dir: &str) -> Result<String, String> {
    run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// Detect the default branch (main or master).
///
/// Checks `origin/HEAD` first, then falls back to checking if `main` or
/// `master` exists locally.
pub fn default_branch(dir: &str) -> Result<String, String> {
    // Try origin/HEAD symbolic ref
    if let Ok(ref_path) = run_git(dir, &["symbolic-ref", "refs/remotes/origin/HEAD"]) {
        if let Some(branch) = ref_path.strip_prefix("refs/remotes/origin/") {
            return Ok(branch.to_string());
        }
    }

    // Fall back: check if main exists
    if branch_exists(dir, "main")? {
        return Ok("main".to_string());
    }

    // Fall back: check if master exists
    if branch_exists(dir, "master")? {
        return Ok("master".to_string());
    }

    Err("could not detect default branch (no main or master found)".to_string())
}

/// Check if a branch exists locally.
pub fn branch_exists(dir: &str, name: &str) -> Result<bool, String> {
    let output = Command::new("git")
        .args(["branch", "--list", name])
        .current_dir(dir)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

/// Create and checkout a new branch (or checkout if it already exists).
pub fn create_and_checkout(dir: &str, name: &str) -> Result<(), String> {
    if branch_exists(dir, name)? {
        checkout(dir, name)
    } else {
        run_git(dir, &["checkout", "-b", name]).map(|_| ())
    }
}

/// Checkout an existing branch.
pub fn checkout(dir: &str, name: &str) -> Result<(), String> {
    run_git(dir, &["checkout", name]).map(|_| ())
}

/// Merge a branch into the current branch with `--no-ff`.
pub fn merge_branch(dir: &str, branch: &str) -> Result<(), String> {
    run_git(dir, &["merge", "--no-ff", branch]).map(|_| ())
}

/// Delete a branch (safe delete with `-d`).
pub fn delete_branch(dir: &str, name: &str) -> Result<(), String> {
    run_git(dir, &["branch", "-d", name]).map(|_| ())
}

/// Convert a feature title to a branch-safe slug.
///
/// Lowercase, replace non-alphanumeric with `-`, collapse consecutive
/// hyphens, and trim leading/trailing hyphens.
pub fn slugify(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse consecutive hyphens
    let mut result = String::with_capacity(slug.len());
    let mut prev_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push('-');
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }

    result.trim_matches('-').to_string()
}

/// Run a git command and return trimmed stdout on success.
fn run_git(dir: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(stderr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("OAuth Integration"), "oauth-integration");
        assert_eq!(slugify("Fix bug #42"), "fix-bug-42");
        assert_eq!(slugify("  Lots   of   spaces  "), "lots-of-spaces");
        assert_eq!(slugify("Special!@#$chars"), "special-chars");
        assert_eq!(slugify("Already-Slugged"), "already-slugged");
    }
}
