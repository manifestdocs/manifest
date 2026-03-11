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

/// Response from the blocked-ancestor endpoint.
#[derive(Debug, serde::Deserialize)]
struct BlockedAncestorResponse {
    id: Uuid,
    title: String,
}

/// HTTP client errors.
#[derive(Debug, Error)]
pub enum ClientError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Conflict: {0}")]
    Conflict(String),

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
            Err(Self::error_from_response(status, response).await)
        }
    }

    /// Handle response for endpoints that return no body (e.g. DELETE → 204).
    async fn handle_response_empty(&self, response: reqwest::Response) -> Result<(), ClientError> {
        let status = response.status();
        if status.is_success() {
            Ok(())
        } else {
            Err(Self::error_from_response(status, response).await)
        }
    }

    /// Convert an error HTTP response into a ClientError.
    async fn error_from_response(status: StatusCode, response: reqwest::Response) -> ClientError {
        let body = response.text().await.unwrap_or_default();
        match status {
            StatusCode::NOT_FOUND => ClientError::NotFound(body),
            StatusCode::BAD_REQUEST => ClientError::BadRequest(body),
            StatusCode::CONFLICT => ClientError::Conflict(body),
            StatusCode::UNAUTHORIZED => ClientError::Unauthorized,
            _ => ClientError::Server(format!("{}: {}", status, body)),
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
    ) -> Result<FeatureHistory, ClientError> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/features/{}/history", feature_id),
            )
            .json(&serde_json::json!({
                "summary": summary,
                "commits": commits,
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
        version_id: Option<Uuid>,
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
        if let Some(v) = version_id {
            params.push(format!("version_id={}", v));
        }
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

    /// Resolve a feature ID from either a full UUID or a short prefix.
    ///
    /// Tries to parse as UUID first. If that fails, falls back to prefix
    /// resolution via the API (optionally scoped to a project).
    pub async fn resolve_feature_id(
        &self,
        id_or_prefix: &str,
        project_id: Option<Uuid>,
    ) -> Result<Uuid, ClientError> {
        // Try full UUID parse first
        if let Ok(uuid) = Uuid::parse_str(id_or_prefix) {
            return Ok(uuid);
        }

        // Try prefix resolution
        let mut params: Vec<(&str, String)> = vec![("prefix", id_or_prefix.to_string())];
        if let Some(pid) = project_id {
            params.push(("project_id", pid.to_string()));
        }

        let response = self
            .request(reqwest::Method::GET, "/features/resolve")
            .query(&params)
            .send()
            .await?;

        let feature: crate::models::Feature = self.handle_response(response).await?;
        Ok(feature.id.into())
    }

    /// Get the features that are blocking a given feature.
    pub async fn get_feature_blockers(&self, id: Uuid) -> Result<Vec<FeatureSummary>, ClientError> {
        let response = self
            .request(reqwest::Method::GET, &format!("/features/{}/blockers", id))
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Find a blocked ancestor feature set in the parent chain.
    pub async fn find_blocked_ancestor(
        &self,
        id: Uuid,
    ) -> Result<Option<(Uuid, String)>, ClientError> {
        let response = self
            .request(
                reqwest::Method::GET,
                &format!("/features/{}/blocked-ancestor", id),
            )
            .send()
            .await?;
        let result: Option<BlockedAncestorResponse> = self.handle_response(response).await?;
        Ok(result.map(|r| (r.id, r.title)))
    }

    /// Delete a feature and its descendants.
    pub async fn delete_feature(&self, id: Uuid) -> Result<(), ClientError> {
        let response = self
            .request(reqwest::Method::DELETE, &format!("/features/{}", id))
            .send()
            .await?;
        self.handle_response_empty(response).await
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
    // Template Operations
    // ============================================================

    /// Get the project's spec template.
    /// Returns `None` if no template exists.
    pub async fn get_default_template(
        &self,
        project_id: Uuid,
    ) -> Result<Option<crate::models::SpecTemplate>, ClientError> {
        let response = self
            .request(
                reqwest::Method::GET,
                &format!("/projects/{}/template", project_id),
            )
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
    // Project Focus
    // ============================================================

    /// Get the focused feature for a project.
    /// Returns `None` if no feature is focused (404 from API).
    pub async fn get_project_focus(
        &self,
        project_id: Uuid,
    ) -> Result<Option<(Uuid, String, String)>, ClientError> {
        let response = self
            .request(
                reqwest::Method::GET,
                &format!("/projects/{}/focus", project_id),
            )
            .send()
            .await?;

        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(ClientError::Server(format!("{}: {}", status, body)));
        }

        #[derive(serde::Deserialize)]
        struct FocusResp {
            feature_id: Uuid,
            feature_title: String,
            feature_state: String,
        }

        let resp: FocusResp = response.json().await?;
        Ok(Some((
            resp.feature_id,
            resp.feature_title,
            resp.feature_state,
        )))
    }

    // ============================================================
    // Project History
    // ============================================================

    /// Get project-wide history across all features.
    pub async fn get_project_history(
        &self,
        project_id: Uuid,
        limit: Option<u32>,
    ) -> Result<Vec<ProjectHistoryEntry>, ClientError> {
        let mut url = format!("/projects/{}/history", project_id);
        if let Some(l) = limit {
            url.push_str(&format!("?limit={}", l));
        }
        let response = self.request(reqwest::Method::GET, &url).send().await?;
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
    /// Get the full instructions for a project.
    ///
    /// If the project has a root feature, returns its details (the source of truth).
    /// Otherwise falls back to project.instructions (legacy).
    pub async fn get_project_instructions_full(&self, project: &Project) -> Option<String> {
        if let Some(root_id) = project.root_feature_id {
            if let Ok(feature) = self.get_feature(root_id.into()).await {
                return feature.details;
            }
        }
        project.instructions.clone()
    }

    /// Get a summary of the project instructions.
    ///
    /// Returns `details_summary` from the root feature if available, otherwise
    /// returns `None`. Does NOT fall back to full `project.instructions` which
    /// can be thousands of tokens. Agents should call `get_project_instructions`
    /// when they need the full text.
    pub async fn get_project_instructions_summary(&self, project: &Project) -> Option<String> {
        if let Some(root_id) = project.root_feature_id {
            if let Ok(feature) = self.get_feature(root_id.into()).await {
                if feature.details_summary.is_some() {
                    return feature.details_summary;
                }
            }
        }
        None
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

        // Get instructions summary from root feature (source of truth) or fallback to project.instructions
        let instructions = self
            .get_project_instructions_summary(&project_with_dirs.project)
            .await;

        Ok(ProjectContextResponse {
            project: ProjectInfo {
                id: project_with_dirs.project.id.into(),
                name: project_with_dirs.project.name,
                description: project_with_dirs.project.description,
                instructions,
            },
            directory: DirectoryInfo {
                id: matching_dir.id.into(),
                path: matching_dir.path.clone(),
                git_remote: matching_dir.git_remote.clone(),
                is_primary: matching_dir.is_primary,
                instructions: matching_dir.instructions.clone(),
            },
        })
    }

    // ============================================================
    // Verification
    // ============================================================

    /// Get assembled spec + diff context for verification (no LLM call).
    pub async fn get_verify_context(
        &self,
        id: Uuid,
        diff: Option<String>,
    ) -> Result<VerifyFeatureContextResponse, ClientError> {
        let body = serde_json::json!({ "diff": diff });
        let response = self
            .request(reqwest::Method::POST, &format!("/features/{}/verify", id))
            .json(&body)
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Store agent-generated verification comments on a feature.
    pub async fn record_verification(
        &self,
        id: Uuid,
        comments: Vec<serde_json::Value>,
    ) -> Result<Feature, ClientError> {
        let body = serde_json::json!({ "comments": comments });
        let response = self
            .request(
                reqwest::Method::PUT,
                &format!("/features/{}/verification", id),
            )
            .json(&body)
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Atomically claim a feature (agent_type, metadata).
    ///
    /// Returns `ClientError::Conflict` with structured JSON body if another
    /// agent already holds a claim (unless force=true).
    pub async fn set_feature_claim(
        &self,
        id: Uuid,
        agent_type: &str,
        metadata: Option<&str>,
        force: bool,
    ) -> Result<(), ClientError> {
        let body = serde_json::json!({
            "agent_type": agent_type,
            "metadata": metadata,
            "force": force,
        });
        let response = self
            .request(reqwest::Method::PUT, &format!("/features/{}/claim", id))
            .json(&body)
            .send()
            .await?;
        self.handle_response::<serde_json::Value>(response).await?;
        Ok(())
    }

    /// Complete a feature: create history, update state, clear claims.
    pub async fn complete_feature(
        &self,
        feature_id: Uuid,
        summary: &str,
        commits: &[CommitRef],
        backfill: bool,
    ) -> Result<CompleteFeatureResponse, ClientError> {
        let mut body = serde_json::json!({
            "summary": summary,
            "commits": commits,
        });
        if backfill {
            body["backfill"] = serde_json::Value::Bool(true);
        }
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/features/{}/complete", feature_id),
            )
            .json(&body)
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Create a proof for a feature.
    pub async fn create_proof(
        &self,
        feature_id: Uuid,
        input: &CreateProofInput,
    ) -> Result<Proof, ClientError> {
        let response = self
            .request(
                reqwest::Method::POST,
                &format!("/features/{}/proofs", feature_id),
            )
            .json(input)
            .send()
            .await?;
        self.handle_response(response).await
    }

    /// Get the latest proof for a feature (most recent first).
    pub async fn get_latest_proof_for_feature(
        &self,
        feature_id: Uuid,
    ) -> Result<Option<Proof>, ClientError> {
        let proofs = self.get_proofs_for_feature(feature_id).await?;
        Ok(proofs.into_iter().next())
    }

    /// Get all proofs for a feature, ordered by most recent first.
    pub async fn get_proofs_for_feature(
        &self,
        feature_id: Uuid,
    ) -> Result<Vec<Proof>, ClientError> {
        let response = self
            .request(
                reqwest::Method::GET,
                &format!("/features/{}/proofs", feature_id),
            )
            .send()
            .await?;
        self.handle_response(response).await
    }
}

/// Response from the complete_feature endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct CompleteFeatureResponse {
    pub feature: Feature,
    pub history: FeatureHistory,
    #[serde(default)]
    pub warnings: Vec<String>,
}
