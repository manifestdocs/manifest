//! GraphQL client for fetching issues, milestones, and sub-issues from GitHub.
//!
//! Uses raw `reqwest` with hand-built GraphQL queries rather than a code-gen crate.
//! All queries are read-only for the initial implementation.

use anyhow::{Context, Result};
use serde::Deserialize;

/// A GitHub issue as returned by the GraphQL API.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubIssue {
    /// GitHub's internal node ID (for GraphQL mutations).
    pub id: String,
    /// Issue number (e.g., 42).
    pub number: i64,
    pub title: String,
    /// Issue body (Markdown).
    pub body: Option<String>,
    /// Issue state: "OPEN" or "CLOSED".
    pub state: String,
    /// Label names.
    pub labels: Vec<String>,
    /// Milestone title, if assigned.
    pub milestone_title: Option<String>,
    /// Milestone node ID, if assigned.
    pub milestone_id: Option<String>,
    /// Parent issue number (from sub-issues), if this is a sub-issue.
    pub parent_number: Option<i64>,
    /// Sub-issue numbers.
    pub sub_issue_numbers: Vec<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// A GitHub milestone as returned by the GraphQL API.
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubMilestone {
    /// GraphQL node ID.
    pub id: String,
    /// Milestone number.
    pub number: i64,
    pub title: String,
    pub description: Option<String>,
    /// "OPEN" or "CLOSED".
    pub state: String,
    pub due_on: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Result of a full sync fetch — all issues and milestones for the repo.
#[derive(Debug, Clone)]
pub struct SyncData {
    pub issues: Vec<GitHubIssue>,
    pub milestones: Vec<GitHubMilestone>,
}

/// GraphQL client for the GitHub API.
pub struct GitHubClient {
    client: reqwest::Client,
    token: String,
    owner: String,
    repo: String,
}

impl GitHubClient {
    /// Create a new GitHub client.
    ///
    /// `repo_full_name` should be "owner/repo" format.
    pub fn new(repo_full_name: &str, token: String) -> Result<Self> {
        let parts: Vec<&str> = repo_full_name.splitn(2, '/').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid repo format: expected 'owner/repo', got '{repo_full_name}'");
        }

        let client = reqwest::Client::builder()
            .user_agent("manifest-github-backend")
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            token,
            owner: parts[0].to_string(),
            repo: parts[1].to_string(),
        })
    }

    /// Fetch all Manifest-labeled issues and milestones from the repository.
    ///
    /// Uses GraphQL to fetch in bulk rather than per-issue REST calls.
    /// Paginates through all results.
    pub async fn fetch_all(&self) -> Result<SyncData> {
        let issues = self.fetch_all_issues().await?;
        let milestones = self.fetch_all_milestones().await?;
        Ok(SyncData { issues, milestones })
    }

    /// Fetch all issues with `manifest:` labels.
    async fn fetch_all_issues(&self) -> Result<Vec<GitHubIssue>> {
        let mut all_issues = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let after_clause = match &cursor {
                Some(c) => format!(r#", after: "{c}""#),
                None => String::new(),
            };

            let query = format!(
                r#"query {{
  repository(owner: "{owner}", name: "{repo}") {{
    issues(first: 100, labels: ["manifest:proposed", "manifest:in_progress", "manifest:implemented", "manifest:archived"]{after}, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{
      pageInfo {{
        hasNextPage
        endCursor
      }}
      nodes {{
        id
        number
        title
        body
        state
        createdAt
        updatedAt
        labels(first: 20) {{
          nodes {{
            name
          }}
        }}
        milestone {{
          id
          title
        }}
      }}
    }}
  }}
}}"#,
                owner = self.owner,
                repo = self.repo,
                after = after_clause,
            );

            let response = self.graphql_query(&query).await?;
            let data = &response["data"]["repository"]["issues"];

            let nodes = data["nodes"]
                .as_array()
                .context("Expected issues.nodes array")?;

            for node in nodes {
                let issue = parse_issue_node(node)?;
                all_issues.push(issue);
            }

            let has_next = data["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false);
            if !has_next {
                break;
            }

            cursor = data["pageInfo"]["endCursor"]
                .as_str()
                .map(|s| s.to_string());
        }

        // Fetch sub-issue relationships for all issues
        self.populate_sub_issues(&mut all_issues).await?;

        Ok(all_issues)
    }

    /// Fetch sub-issue relationships using the REST API.
    ///
    /// The sub-issues API is REST-only (not available in GraphQL as of 2025).
    async fn populate_sub_issues(&self, issues: &mut [GitHubIssue]) -> Result<()> {
        // Collect issue numbers that might have sub-issues (those with manifest:feature_set label)
        let parent_numbers: Vec<i64> = issues
            .iter()
            .filter(|i| i.labels.iter().any(|l| l == "manifest:feature_set"))
            .map(|i| i.number)
            .collect();

        for parent_number in parent_numbers {
            let sub_issues = self.fetch_sub_issues(parent_number).await?;

            // Set parent_number on sub-issues
            for sub_number in &sub_issues {
                if let Some(issue) = issues.iter_mut().find(|i| i.number == *sub_number) {
                    issue.parent_number = Some(parent_number);
                }
            }

            // Set sub_issue_numbers on parent
            if let Some(parent) = issues.iter_mut().find(|i| i.number == parent_number) {
                parent.sub_issue_numbers = sub_issues;
            }
        }

        Ok(())
    }

    /// Fetch sub-issue numbers for a given parent issue.
    async fn fetch_sub_issues(&self, parent_number: i64) -> Result<Vec<i64>> {
        let url = format!(
            "https://api.github.com/repos/{}/{}/issues/{}/sub_issues",
            self.owner, self.repo, parent_number
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .context("Failed to fetch sub-issues")?;

        if !response.status().is_success() {
            let status = response.status();
            // Sub-issues API might not be available — treat as empty
            if status.as_u16() == 404 {
                tracing::debug!("Sub-issues API returned 404 for issue #{parent_number}");
                return Ok(Vec::new());
            }
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("GitHub sub-issues API error {status}: {body}");
        }

        let items: Vec<serde_json::Value> = response.json().await?;
        let numbers: Vec<i64> = items
            .iter()
            .filter_map(|item| item["number"].as_i64())
            .collect();

        Ok(numbers)
    }

    /// Fetch all milestones from the repository.
    async fn fetch_all_milestones(&self) -> Result<Vec<GitHubMilestone>> {
        let mut all_milestones = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let after_clause = match &cursor {
                Some(c) => format!(r#", after: "{c}""#),
                None => String::new(),
            };

            let query = format!(
                r#"query {{
  repository(owner: "{owner}", name: "{repo}") {{
    milestones(first: 100{after}, orderBy: {{field: CREATED_AT, direction: ASC}}) {{
      pageInfo {{
        hasNextPage
        endCursor
      }}
      nodes {{
        id
        number
        title
        description
        state
        dueOn
        createdAt
        updatedAt
      }}
    }}
  }}
}}"#,
                owner = self.owner,
                repo = self.repo,
                after = after_clause,
            );

            let response = self.graphql_query(&query).await?;
            let data = &response["data"]["repository"]["milestones"];

            let nodes = data["nodes"]
                .as_array()
                .context("Expected milestones.nodes array")?;

            for node in nodes {
                let milestone = parse_milestone_node(node)?;
                all_milestones.push(milestone);
            }

            let has_next = data["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false);
            if !has_next {
                break;
            }

            cursor = data["pageInfo"]["endCursor"]
                .as_str()
                .map(|s| s.to_string());
        }

        Ok(all_milestones)
    }

    /// Execute a GraphQL query against the GitHub API.
    async fn graphql_query(&self, query: &str) -> Result<serde_json::Value> {
        let body = serde_json::json!({ "query": query });

        let response = self
            .client
            .post("https://api.github.com/graphql")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .json(&body)
            .send()
            .await
            .context("Failed to send GraphQL request")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();

            if status.as_u16() == 401 || status.as_u16() == 403 {
                anyhow::bail!("GitHub authentication failed ({status}): {body}");
            }

            anyhow::bail!("GitHub GraphQL error ({status}): {body}");
        }

        let json: serde_json::Value = response.json().await?;

        // Check for GraphQL-level errors
        if let Some(errors) = json.get("errors") {
            if let Some(arr) = errors.as_array() {
                if !arr.is_empty() {
                    let messages: Vec<&str> =
                        arr.iter().filter_map(|e| e["message"].as_str()).collect();
                    anyhow::bail!("GitHub GraphQL errors: {}", messages.join("; "));
                }
            }
        }

        Ok(json)
    }

    /// Get the repo full name ("owner/repo").
    pub fn repo_full_name(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }
}

/// Parse a GraphQL issue node into a `GitHubIssue`.
fn parse_issue_node(node: &serde_json::Value) -> Result<GitHubIssue> {
    let labels: Vec<String> = node["labels"]["nodes"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|l| l["name"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(GitHubIssue {
        id: node["id"].as_str().context("Missing issue id")?.to_string(),
        number: node["number"].as_i64().context("Missing issue number")?,
        title: node["title"]
            .as_str()
            .context("Missing issue title")?
            .to_string(),
        body: node["body"].as_str().map(|s| s.to_string()),
        state: node["state"]
            .as_str()
            .context("Missing issue state")?
            .to_string(),
        labels,
        milestone_title: node["milestone"]["title"].as_str().map(|s| s.to_string()),
        milestone_id: node["milestone"]["id"].as_str().map(|s| s.to_string()),
        parent_number: None,           // Populated later via sub-issues API
        sub_issue_numbers: Vec::new(), // Populated later
        created_at: node["createdAt"]
            .as_str()
            .context("Missing createdAt")?
            .to_string(),
        updated_at: node["updatedAt"]
            .as_str()
            .context("Missing updatedAt")?
            .to_string(),
    })
}

/// Parse a GraphQL milestone node into a `GitHubMilestone`.
fn parse_milestone_node(node: &serde_json::Value) -> Result<GitHubMilestone> {
    Ok(GitHubMilestone {
        id: node["id"]
            .as_str()
            .context("Missing milestone id")?
            .to_string(),
        number: node["number"]
            .as_i64()
            .context("Missing milestone number")?,
        title: node["title"]
            .as_str()
            .context("Missing milestone title")?
            .to_string(),
        description: node["description"].as_str().map(|s| s.to_string()),
        state: node["state"]
            .as_str()
            .context("Missing milestone state")?
            .to_string(),
        due_on: node["dueOn"].as_str().map(|s| s.to_string()),
        created_at: node["createdAt"]
            .as_str()
            .context("Missing createdAt")?
            .to_string(),
        updated_at: node["updatedAt"]
            .as_str()
            .context("Missing updatedAt")?
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_issue_node_extracts_fields() {
        let node = serde_json::json!({
            "id": "I_abc123",
            "number": 42,
            "title": "OAuth Login",
            "body": "<!-- manifest:feature -->\nDetails here",
            "state": "OPEN",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-02T00:00:00Z",
            "labels": {
                "nodes": [
                    { "name": "manifest:in_progress" },
                    { "name": "bug" }
                ]
            },
            "milestone": {
                "id": "MI_xyz",
                "title": "0.1.0"
            }
        });

        let issue = parse_issue_node(&node).unwrap();
        assert_eq!(issue.number, 42);
        assert_eq!(issue.title, "OAuth Login");
        assert_eq!(issue.labels, vec!["manifest:in_progress", "bug"]);
        assert_eq!(issue.milestone_title.as_deref(), Some("0.1.0"));
        assert_eq!(issue.milestone_id.as_deref(), Some("MI_xyz"));
    }

    #[test]
    fn parse_issue_node_handles_no_milestone() {
        let node = serde_json::json!({
            "id": "I_abc",
            "number": 1,
            "title": "Test",
            "body": null,
            "state": "OPEN",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "labels": { "nodes": [] },
            "milestone": null
        });

        let issue = parse_issue_node(&node).unwrap();
        assert!(issue.milestone_title.is_none());
        assert!(issue.body.is_none());
    }

    #[test]
    fn parse_milestone_node_extracts_fields() {
        let node = serde_json::json!({
            "id": "MI_123",
            "number": 1,
            "title": "0.1.0",
            "description": "<!-- manifest:id:550e8400-e29b-41d4-a716-446655440000 -->\nFirst release",
            "state": "OPEN",
            "dueOn": "2026-06-01T00:00:00Z",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z"
        });

        let milestone = parse_milestone_node(&node).unwrap();
        assert_eq!(milestone.title, "0.1.0");
        assert_eq!(milestone.number, 1);
        assert!(milestone.description.unwrap().contains("manifest:id"));
    }

    #[test]
    fn github_client_rejects_bad_repo_format() {
        let result = GitHubClient::new("invalid-no-slash", "token".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn github_client_accepts_valid_repo() {
        let result = GitHubClient::new("owner/repo", "token".to_string());
        assert!(result.is_ok());
        let client = result.unwrap();
        assert_eq!(client.repo_full_name(), "owner/repo");
    }
}
