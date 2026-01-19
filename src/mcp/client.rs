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
    DirectoryInfo, FeatureInfo, PlanFeaturesResponse, ProjectContextResponse, ProjectInfo,
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
    /// Create client from environment variables.
    pub fn from_env() -> Self {
        let base_url = std::env::var("MANIFEST_URL").unwrap_or_else(|_| DEFAULT_URL.to_string());
        let api_key = std::env::var("MANIFEST_API_KEY").ok();
        Self::new(base_url, api_key)
    }

    /// Create with explicit configuration.
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            base_url: base_url.into(),
            api_key,
            client,
        }
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
        let mut url = "/features/search".to_string();

        // Build query string with manual percent-encoding for the query
        let encoded_query: String = query
            .chars()
            .map(|c| match c {
                ' ' => "%20".to_string(),
                '&' => "%26".to_string(),
                '=' => "%3D".to_string(),
                '?' => "%3F".to_string(),
                '#' => "%23".to_string(),
                '%' => "%25".to_string(),
                _ => c.to_string(),
            })
            .collect();
        let mut params = vec![format!("q={}", encoded_query)];
        if let Some(pid) = project_id {
            params.push(format!("project_id={}", pid));
        }
        if let Some(l) = limit {
            params.push(format!("limit={}", l));
        }
        url.push('?');
        url.push_str(&params.join("&"));

        let response = self.request(reqwest::Method::GET, &url).send().await?;
        self.handle_response(response).await
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
        features: &[ProposedFeature],
        confirm: bool,
    ) -> Result<PlanFeaturesResponse, ClientError> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/projects/{}/features/bulk", project_id),
            )
            .json(&serde_json::json!({
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
        let url = format!("{}/projects/by-directory", self.base_url);
        let mut req = self.client.get(&url).query(&[("path", path)]);
        if let Some(ref key) = self.api_key {
            req = req.bearer_auth(key);
        }
        let response = req.send().await?;
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
}

// ============================================================
// Helpers for MCP Types
// ============================================================

impl ManifestClient {
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

        Ok(ProjectContextResponse {
            project: ProjectInfo {
                id: project_with_dirs.project.id.to_string(),
                name: project_with_dirs.project.name,
                description: project_with_dirs.project.description,
                instructions: project_with_dirs.project.instructions,
            },
            directory: DirectoryInfo {
                id: matching_dir.id.to_string(),
                path: matching_dir.path.clone(),
                git_remote: matching_dir.git_remote.clone(),
                is_primary: matching_dir.is_primary,
                instructions: matching_dir.instructions.clone(),
            },
        })
    }

    /// Convert Feature to FeatureInfo for MCP response.
    pub fn feature_to_info(feature: &Feature) -> FeatureInfo {
        FeatureInfo {
            id: feature.id.to_string(),
            title: feature.title.clone(),
            details: feature.details.clone(),
            desired_details: feature.desired_details.clone(),
            state: feature.state.as_str().to_string(),
            priority: feature.priority,
        }
    }
}
