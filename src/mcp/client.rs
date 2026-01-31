//! HTTP client for Manifest API.
//!
//! This client abstracts whether the MCP server talks to a local or remote API.
//! Configuration is via environment variables:
//! - `MANIFEST_URL` - Base URL (default: `http://localhost:17010/api/v1`)
//! - `MANIFEST_API_KEY` - API key for authentication (optional for local)

use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::de::DeserializeOwned;
use thiserror::Error;
use uuid::Uuid;

use crate::mcp::{
    DirectoryInfo, PlanFeaturesResponse, ProjectAnalysis, ProjectContextResponse, ProjectInfo,
    ProposedFeature,
};
use crate::models::*;

/// Default URL for local development.
const DEFAULT_URL: &str = "http://localhost:17010/api/v1";

/// HTTP client errors.
#[derive(Debug, Error)]
pub enum ClientError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized: API key required or invalid")]
    Unauthorized,

    #[error("Server error: {0}")]
    Server(String),
}

/// HTTP client for Manifest API.
#[derive(Debug, Clone)]
pub struct ManifestClient {
    base_url: String,
    api_key: Option<String>,
    client: Client,
}

impl ManifestClient {
    /// Create a client configured from environment variables.
    ///
    /// Uses `MANIFEST_URL` (default: `http://localhost:17010/api/v1`) and
    /// optional `MANIFEST_API_KEY` for authentication.
    pub fn from_env() -> Self {
        let base_url = std::env::var("MANIFEST_URL").unwrap_or_else(|_| DEFAULT_URL.to_string());
        let api_key = std::env::var("MANIFEST_API_KEY").ok();
        Self::new(base_url, api_key).expect("Failed to build HTTP client")
    }

    /// Create a client with explicit base URL and optional API key.
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
    ) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            base_url: base_url.into(),
            api_key,
            client,
        })
    }

    /// Build a request with optional auth header.
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.request(method, &url);
        if let Some(ref key) = self.api_key {
            req = req.bearer_auth(key);
        }
        req
    }

    /// Handle response, converting HTTP errors to ClientError.
    async fn handle_response<T: DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> Result<T, ClientError> {
        let status = response.status();
        if status.is_success() {
            Ok(response.json().await?)
        } else {
            let body = response.text().await.unwrap_or_default();
            match status {
                StatusCode::NOT_FOUND => Err(ClientError::NotFound(body)),
                StatusCode::BAD_REQUEST => Err(ClientError::BadRequest(body)),
                StatusCode::UNAUTHORIZED => Err(ClientError::Unauthorized),
                _ => Err(ClientError::Server(format!("{}: {}", status, body))),
            }
        }
    }

    // ============================================================
    // Feature Operations
    // ============================================================

    /// Get a feature by ID.
    pub async fn get_feature(&self, id: Uuid) -> Result<Feature, ClientError> {
        let response = self
            .request(reqwest::Method::GET, &format!("/features/{}", id))
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Get a feature with hierarchical context (parent, siblings, children, breadcrumb).
    pub async fn get_feature_with_context(
        &self,
        id: Uuid,
    ) -> Result<FeatureWithContext, ClientError> {
        let response = self
            .request(reqwest::Method::GET, &format!("/features/{}/context", id))
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Get the next workable feature for a project.
    pub async fn get_next_feature(
        &self,
        project_id: Uuid,
        version_id: Option<Uuid>,
    ) -> Result<Option<FeatureWithContext>, ClientError> {
        let mut url = format!("/projects/{}/features/next", project_id);
        if let Some(vid) = version_id {
            url.push_str(&format!("?version_id={}", vid));
        }
        let response = self.request(reqwest::Method::GET, &url).send().await?;
        self.handle_response(response).await
    }

    /// Get history for a feature.
    pub async fn get_feature_history(&self, id: Uuid) -> Result<Vec<FeatureHistory>, ClientError> {
        let response = self
            .request(reqwest::Method::GET, &format!("/features/{}/history", id))
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Create a history entry directly on a feature (CLI mode).
    /// Optionally marks the feature as implemented.
    pub async fn create_feature_history(
        &self,
        feature_id: Uuid,
        summary: &str,
        commits: &[CommitRef],
        mark_implemented: bool,
    ) -> Result<FeatureHistory, ClientError> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/features/{}/history", feature_id),
            )
            .json(&serde_json::json!({
                "summary": summary,
                "commits": commits,
                "mark_implemented": mark_implemented
            }))
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Get the full feature tree for a project.
    pub async fn get_feature_tree(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<FeatureTreeNode>, ClientError> {
        let response = self
            .request(
                reqwest::Method::GET,
                &format!("/projects/{}/features/tree", project_id),
            )
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// List all features with optional filtering.
    /// Always returns summaries only - use get_feature for full details.
    pub async fn list_features(
        &self,
        project_id: Option<Uuid>,
        state: Option<&str>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<FeatureSummary>, ClientError> {
        let mut url = match project_id {
            Some(pid) => format!("/projects/{}/features", pid),
            None => "/features".to_string(),
        };

        // Build query string
        let mut params = vec![];
        if let Some(s) = state {
            params.push(format!("state={}", s));
        }
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        if let Some(o) = offset {
            params.push(format!("offset={}", o));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        let response = self.request(reqwest::Method::GET, &url).send().await?;
        self.handle_response(response).await
    }

    /// Search features by title and details.
    /// Returns summaries ranked by relevance.
    pub async fn search_features(
        &self,
        query: &str,
        project_id: Option<Uuid>,
        limit: Option<u32>,
    ) -> Result<Vec<FeatureSummary>, ClientError> {
        let mut params: Vec<(&str, String)> = vec![("q", query.to_string())];
        if let Some(pid) = project_id {
            params.push(("project_id", pid.to_string()));
        }
        if let Some(l) = limit {
            params.push(("limit", l.to_string()));
        }

        let response = self
            .request(reqwest::Method::GET, "/features/search")
            .query(&params)
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Delete a feature and its descendants.
    pub async fn delete_feature(&self, id: Uuid) -> Result<(), ClientError> {
        let response = self
            .request(reqwest::Method::DELETE, &format!("/features/{}", id))
            .send()
            .await?;
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            let body = response.text().await.unwrap_or_default();
            match status {
                StatusCode::NOT_FOUND => Err(ClientError::NotFound(body)),
                StatusCode::BAD_REQUEST => Err(ClientError::BadRequest(body)),
                StatusCode::UNAUTHORIZED => Err(ClientError::Unauthorized),
                _ => Err(ClientError::Server(format!("{}: {}", status, body))),
            }
        }
    }

    /// Update a feature.
    pub async fn update_feature(
        &self,
        id: Uuid,
        input: &UpdateFeatureInput,
    ) -> Result<Feature, ClientError> {
        let response = self
            .request(reqwest::Method::PUT, &format!("/features/{}", id))
            .json(input)
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Create a feature.
    pub async fn create_feature(
        &self,
        project_id: Uuid,
        input: &CreateFeatureInput,
    ) -> Result<Feature, ClientError> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/projects/{}/features", project_id),
            )
            .json(input)
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Bulk create features (plan_features).
    pub async fn bulk_create_features(
        &self,
        project_id: Uuid,
        target_version_id: Option<Uuid>,
        features: &[ProposedFeature],
        confirm: bool,
    ) -> Result<PlanFeaturesResponse, ClientError> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/projects/{}/features/bulk", project_id),
            )
            .json(&serde_json::json!({
                "target_version_id": target_version_id,
                "features": features,
                "confirm": confirm
            }))
            .send()
            .await?;
        self.handle_response(response).await
    }

    // ============================================================
    // Project Operations
    // ============================================================

    /// Get a project by ID.
    pub async fn get_project(&self, id: Uuid) -> Result<ProjectWithDirectories, ClientError> {
        let response = self
            .request(reqwest::Method::GET, &format!("/projects/{}", id))
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Get project by directory path.
    pub async fn get_project_by_directory(
        &self,
        path: &str,
    ) -> Result<ProjectWithDirectories, ClientError> {
        let response = self
            .request(reqwest::Method::GET, "/projects/by-directory")
            .query(&[("path", path)])
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// List all projects.
    pub async fn list_projects(&self) -> Result<Vec<Project>, ClientError> {
        let response = self
            .request(reqwest::Method::GET, "/projects")
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Create a project.
    pub async fn create_project(&self, input: &CreateProjectInput) -> Result<Project, ClientError> {
        let response = self
            .request(reqwest::Method::POST, "/projects")
            .json(input)
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Add a directory to a project.
    pub async fn add_project_directory(
        &self,
        project_id: Uuid,
        input: &AddDirectoryInput,
    ) -> Result<ProjectDirectory, ClientError> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/projects/{}/directories", project_id),
            )
            .json(input)
            .send()
            .await?;
        self.handle_response(response).await
    }

    // ============================================================
    // Version Operations
    // ============================================================

    /// List versions for a project.
    pub async fn list_versions(&self, project_id: Uuid) -> Result<Vec<Version>, ClientError> {
        let response = self
            .request(
                reqwest::Method::GET,
                &format!("/projects/{}/versions", project_id),
            )
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Create a version.
    pub async fn create_version(
        &self,
        project_id: Uuid,
        input: &CreateVersionInput,
    ) -> Result<Version, ClientError> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/projects/{}/versions", project_id),
            )
            .json(input)
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Update a version.
    pub async fn update_version(
        &self,
        id: Uuid,
        input: &UpdateVersionInput,
    ) -> Result<Version, ClientError> {
        let response = self
            .request(reqwest::Method::PUT, &format!("/versions/{}", id))
            .json(input)
            .send()
            .await?;
        self.handle_response(response).await
    }

    // ============================================================
    // Project Analysis
    // ============================================================

    /// Analyze a project directory to discover structure and suggest features.
    pub async fn analyze_project(
        &self,
        directory_path: &str,
        include_docs: bool,
        max_depth: u32,
    ) -> Result<ProjectAnalysis, ClientError> {
        let response = self
            .request(reqwest::Method::GET, "/codebase/analyze")
            .query(&[
                ("path", directory_path),
                ("include_docs", &include_docs.to_string()),
                ("max_depth", &max_depth.to_string()),
            ])
            .send()
            .await?;
        self.handle_response(response).await
    }
}

// ============================================================
// Helpers for MCP Types
// ============================================================

impl ManifestClient {
    /// Get the instructions for a project.
    ///
    /// If the project has a root feature, returns its details (the source of truth).
    /// Otherwise falls back to project.instructions (legacy).
    pub async fn get_project_instructions(&self, project: &Project) -> Option<String> {
        if let Some(root_id) = project.root_feature_id {
            // Fetch root feature and use its details
            if let Ok(feature) = self.get_feature(root_id).await {
                return feature.details;
            }
        }
        // Fallback to legacy project.instructions
        project.instructions.clone()
    }

    /// Get project context for MCP response (project + matching directory info).
    pub async fn get_project_context(
        &self,
        directory_path: &str,
    ) -> Result<ProjectContextResponse, ClientError> {
        let project_with_dirs = self.get_project_by_directory(directory_path).await?;

        // Find the matching directory
        let matching_dir = project_with_dirs
            .directories
            .iter()
            .find(|d| {
                directory_path == d.path || directory_path.starts_with(&format!("{}/", d.path))
            })
            .ok_or_else(|| ClientError::Server("Directory match logic error".to_string()))?;

        // Get instructions from root feature (source of truth) or fallback to project.instructions
        let instructions = self
            .get_project_instructions(&project_with_dirs.project)
            .await;

        Ok(ProjectContextResponse {
            project: ProjectInfo {
                id: project_with_dirs.project.id,
                name: project_with_dirs.project.name,
                description: project_with_dirs.project.description,
                instructions,
            },
            directory: DirectoryInfo {
                id: matching_dir.id,
                path: matching_dir.path.clone(),
                git_remote: matching_dir.git_remote.clone(),
                is_primary: matching_dir.is_primary,
                instructions: matching_dir.instructions.clone(),
            },
        })
    }
}
