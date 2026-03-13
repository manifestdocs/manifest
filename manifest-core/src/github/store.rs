//! [`FeatureStore`] implementation backed by GitHub Issues.
//!
//! This is a **read-only** store that fetches feature data from the GitHub API.
//! Write operations return [`StoreError::Unsupported`]. The store is designed
//! to be wrapped by [`CachedStore`](crate::store::CachedStore) which provides
//! local caching and (eventually) write-through support.
//!
//! # Data Model Mapping
//!
//! | Manifest      | GitHub                    |
//! |---------------|---------------------------|
//! | Feature       | Issue                     |
//! | Feature state | Labels (`manifest:*`)     |
//! | Feature tree  | Sub-Issues                |
//! | Version       | Milestone                 |
//! | History       | Issue comments            |

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use crate::models::*;

use super::client::{GitHubClient, GitHubIssue, GitHubMilestone, SyncData};
use super::labels::state_from_labels;
use super::parser::parse_issue_body;
use crate::store::{
    BackendType, FeatureChangeset, FeatureQuery, FeatureStore, ParentFilter, StoreCapabilities,
    StoreError, StoreResult,
};

/// In-memory snapshot of the GitHub repository's feature data.
///
/// Populated by a full fetch from the GitHub API, then queried by the store.
/// The `CachedStore` wrapper manages refresh intervals.
#[derive(Debug, Default)]
struct Snapshot {
    project: Option<Project>,
    features: Vec<Feature>,
    versions: Vec<Version>,
    /// Map from GitHub issue number to feature ID for parent resolution.
    number_to_id: HashMap<i64, FeatureId>,
    /// Map from milestone title to version ID.
    milestone_to_version: HashMap<String, VersionId>,
    last_synced_at: Option<DateTime<Utc>>,
}

/// GitHub-backed feature store.
///
/// Holds a snapshot of the repo's feature data in memory, refreshed via
/// [`sync`](#method.sync). All read methods query the snapshot.
/// Write methods return `Unsupported`.
pub struct GitHubStore {
    client: Arc<GitHubClient>,
    snapshot: RwLock<Snapshot>,
    /// The project ID used for all features (one GitHub repo = one project).
    project_id: ProjectId,
}

impl GitHubStore {
    /// Create a new `GitHubStore`.
    ///
    /// Does NOT fetch data — call [`sync`](#method.sync) to populate.
    pub fn new(client: GitHubClient, project_id: ProjectId) -> Self {
        Self {
            client: Arc::new(client),
            snapshot: RwLock::new(Snapshot::default()),
            project_id,
        }
    }

    /// Fetch all data from GitHub and populate the in-memory snapshot.
    pub async fn sync(&self) -> anyhow::Result<()> {
        let data = self.client.fetch_all().await?;
        let snapshot = self.build_snapshot(data)?;
        *self.snapshot.write().await = snapshot;
        Ok(())
    }

    /// Build an in-memory snapshot from raw GitHub API data.
    fn build_snapshot(&self, data: SyncData) -> anyhow::Result<Snapshot> {
        let mut snapshot = Snapshot::default();
        let now = Utc::now();

        // Build versions from milestones
        for milestone in &data.milestones {
            let version = self.milestone_to_version(milestone)?;
            snapshot
                .milestone_to_version
                .insert(milestone.title.clone(), version.id);
            snapshot.versions.push(version);
        }

        // First pass: assign IDs and build the number-to-ID map
        let mut issue_features: Vec<(GitHubIssue, FeatureId)> = Vec::new();
        for issue in &data.issues {
            let meta = parse_issue_body(issue.body.as_deref().unwrap_or(""));
            let feature_id = meta.manifest_id.unwrap_or_else(FeatureId::new);
            snapshot.number_to_id.insert(issue.number, feature_id);
            issue_features.push((issue.clone(), feature_id));
        }

        // Second pass: build features with resolved parent IDs
        for (issue, feature_id) in &issue_features {
            let feature = self.issue_to_feature(issue, *feature_id, &snapshot)?;
            snapshot.features.push(feature);
        }

        // Build project
        snapshot.project = Some(Project {
            id: self.project_id,
            slug: self.client.repo_full_name().replace('/', "-"),
            name: self.client.repo_full_name(),
            description: None,
            instructions: None,
            current_version_id: None,
            root_feature_id: None,
            default_feature_destination: "backlog".to_string(),
            test_adapter: None,
            context_budget: None,
            key_prefix: "GH".to_string(),
            created_at: now,
            updated_at: now,
        });

        snapshot.last_synced_at = Some(now);

        Ok(snapshot)
    }

    /// Convert a GitHub issue to a Manifest Feature.
    fn issue_to_feature(
        &self,
        issue: &GitHubIssue,
        feature_id: FeatureId,
        snapshot: &Snapshot,
    ) -> anyhow::Result<Feature> {
        let meta = parse_issue_body(issue.body.as_deref().unwrap_or(""));

        let state = state_from_labels(&issue.labels).unwrap_or(FeatureState::Proposed);

        let parent_id = issue
            .parent_number
            .and_then(|n| snapshot.number_to_id.get(&n))
            .copied();

        let target_version_id = issue
            .milestone_title
            .as_ref()
            .and_then(|t| snapshot.milestone_to_version.get(t))
            .copied();

        let created_at = parse_datetime(&issue.created_at)?;
        let updated_at = parse_datetime(&issue.updated_at)?;

        Ok(Feature {
            id: feature_id,
            project_id: self.project_id,
            parent_id,
            title: issue.title.clone(),
            details: meta.details,
            desired_details: meta.desired_details,
            details_summary: meta.details_summary,
            state,
            priority: 0, // GitHub doesn't have a native priority field
            feature_number: Some(issue.number as i32),
            target_version_id,
            verification_result: None,
            verified_at: None,
            claimed_by: None,
            claimed_at: None,
            claim_metadata: None,
            created_at,
            updated_at,
        })
    }

    /// Convert a GitHub milestone to a Manifest Version.
    fn milestone_to_version(&self, milestone: &GitHubMilestone) -> anyhow::Result<Version> {
        // Check for manifest:id in milestone description
        let version_id = milestone
            .description
            .as_ref()
            .and_then(|d| {
                let meta = parse_issue_body(d);
                meta.manifest_id.map(|fid| VersionId::from(fid.inner()))
            })
            .unwrap_or_else(VersionId::new);

        let released_at = if milestone.state == "CLOSED" {
            milestone
                .due_on
                .as_ref()
                .and_then(|d| parse_datetime(d).ok())
                .or(Some(Utc::now()))
        } else {
            None
        };

        let created_at = parse_datetime(&milestone.created_at)?;
        let updated_at = parse_datetime(&milestone.updated_at)?;

        // Strip manifest metadata from description for display
        let description = milestone.description.as_ref().map(|d| {
            d.lines()
                .filter(|l| !l.trim().starts_with("<!-- manifest:"))
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string()
        });

        Ok(Version {
            id: version_id,
            project_id: self.project_id,
            name: milestone.title.clone(),
            description,
            released_at,
            created_at,
            updated_at,
        })
    }
}

/// Parse an ISO 8601 datetime string.
fn parse_datetime(s: &str) -> anyhow::Result<DateTime<Utc>> {
    s.parse::<DateTime<Utc>>()
        .map_err(|e| anyhow::anyhow!("Failed to parse datetime '{s}': {e}"))
}

fn unsupported(op: &str) -> StoreError {
    StoreError::Unsupported(format!("GitHub backend is read-only: {op} not supported"))
}

#[async_trait]
impl FeatureStore for GitHubStore {
    // ── Projects ────────────────────────────────────────────────────────

    async fn get_project(&self, project_id: &ProjectId) -> StoreResult<Project> {
        let snap = self.snapshot.read().await;
        snap.project
            .as_ref()
            .filter(|p| p.id == *project_id)
            .cloned()
            .ok_or_else(|| StoreError::ProjectNotFound(project_id.to_string()))
    }

    async fn list_projects(&self) -> StoreResult<Vec<Project>> {
        let snap = self.snapshot.read().await;
        Ok(snap.project.iter().cloned().collect())
    }

    async fn create_project(&self, _input: &CreateProjectInput) -> StoreResult<Project> {
        Err(unsupported("create_project"))
    }

    async fn update_project(
        &self,
        _project_id: &ProjectId,
        _input: &UpdateProjectInput,
    ) -> StoreResult<Project> {
        Err(unsupported("update_project"))
    }

    // ── Features ────────────────────────────────────────────────────────

    async fn get_feature(&self, feature_id: &FeatureId) -> StoreResult<Feature> {
        let snap = self.snapshot.read().await;
        snap.features
            .iter()
            .find(|f| f.id == *feature_id)
            .cloned()
            .ok_or_else(|| StoreError::FeatureNotFound(feature_id.to_string()))
    }

    async fn get_feature_by_number(
        &self,
        project_id: &ProjectId,
        feature_number: i32,
    ) -> StoreResult<Feature> {
        let snap = self.snapshot.read().await;
        snap.features
            .iter()
            .find(|f| f.project_id == *project_id && f.feature_number == Some(feature_number))
            .cloned()
            .ok_or_else(|| {
                StoreError::FeatureNotFound(format!(
                    "Feature #{feature_number} not found in project {project_id}"
                ))
            })
    }

    async fn list_features(&self, query: &FeatureQuery) -> StoreResult<Vec<Feature>> {
        let snap = self.snapshot.read().await;
        let mut results: Vec<&Feature> = snap.features.iter().collect();

        // Filter by project
        if let Some(pid) = query.project_id {
            results.retain(|f| f.project_id == pid);
        }

        // Filter by parent
        if let Some(ref parent) = query.parent_id {
            match parent {
                ParentFilter::Root => results.retain(|f| f.parent_id.is_none()),
                ParentFilter::Exact(pid) => results.retain(|f| f.parent_id == Some(*pid)),
                ParentFilter::Any => {}
            }
        }

        // Filter by state
        if let Some(ref states) = query.state {
            results.retain(|f| states.contains(&f.state));
        }

        // Filter by version
        if let Some(vid) = query.target_version_id {
            results.retain(|f| f.target_version_id == Some(vid));
        }

        // Search
        if let Some(ref search) = query.search {
            let lower = search.to_lowercase();
            results.retain(|f| {
                f.title.to_lowercase().contains(&lower)
                    || f.details
                        .as_ref()
                        .is_some_and(|d| d.to_lowercase().contains(&lower))
            });
        }

        // Sort by priority
        let mut owned: Vec<Feature> = results.into_iter().cloned().collect();
        owned.sort_by_key(|f| f.priority);

        // Pagination
        if let Some(offset) = query.offset {
            let offset = offset as usize;
            if offset < owned.len() {
                owned = owned[offset..].to_vec();
            } else {
                owned.clear();
            }
        }
        if let Some(limit) = query.limit {
            owned.truncate(limit as usize);
        }

        Ok(owned)
    }

    async fn create_feature(
        &self,
        _project_id: &ProjectId,
        _input: &CreateFeatureInput,
    ) -> StoreResult<Feature> {
        Err(unsupported("create_feature"))
    }

    async fn update_feature(
        &self,
        _feature_id: &FeatureId,
        _changeset: &FeatureChangeset,
    ) -> StoreResult<Feature> {
        Err(unsupported("update_feature"))
    }

    async fn delete_feature(&self, _feature_id: &FeatureId) -> StoreResult<()> {
        Err(unsupported("delete_feature"))
    }

    async fn move_feature(
        &self,
        _feature_id: &FeatureId,
        _new_parent_id: Option<&FeatureId>,
        _position: Option<i32>,
    ) -> StoreResult<()> {
        Err(unsupported("move_feature"))
    }

    async fn get_feature_children(&self, feature_id: &FeatureId) -> StoreResult<Vec<Feature>> {
        let snap = self.snapshot.read().await;
        Ok(snap
            .features
            .iter()
            .filter(|f| f.parent_id == Some(*feature_id))
            .cloned()
            .collect())
    }

    async fn get_feature_ancestors(&self, feature_id: &FeatureId) -> StoreResult<Vec<Feature>> {
        let snap = self.snapshot.read().await;
        let mut ancestors = Vec::new();
        let mut current_id = *feature_id;

        loop {
            let feature = match snap.features.iter().find(|f| f.id == current_id) {
                Some(f) => f,
                None => break,
            };
            match feature.parent_id {
                Some(pid) => {
                    if let Some(parent) = snap.features.iter().find(|f| f.id == pid) {
                        ancestors.push(parent.clone());
                        current_id = pid;
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }

        ancestors.reverse(); // Root first
        Ok(ancestors)
    }

    // ── Versions ────────────────────────────────────────────────────────

    async fn get_version(&self, version_id: &VersionId) -> StoreResult<Version> {
        let snap = self.snapshot.read().await;
        snap.versions
            .iter()
            .find(|v| v.id == *version_id)
            .cloned()
            .ok_or_else(|| StoreError::VersionNotFound(version_id.to_string()))
    }

    async fn list_versions(&self, project_id: &ProjectId) -> StoreResult<Vec<Version>> {
        let snap = self.snapshot.read().await;
        Ok(snap
            .versions
            .iter()
            .filter(|v| v.project_id == *project_id)
            .cloned()
            .collect())
    }

    async fn create_version(
        &self,
        _project_id: &ProjectId,
        _input: &CreateVersionInput,
    ) -> StoreResult<Version> {
        Err(unsupported("create_version"))
    }

    async fn update_version(
        &self,
        _version_id: &VersionId,
        _input: &UpdateVersionInput,
    ) -> StoreResult<Version> {
        Err(unsupported("update_version"))
    }

    async fn delete_version(&self, _version_id: &VersionId) -> StoreResult<()> {
        Err(unsupported("delete_version"))
    }

    // ── Feature History ─────────────────────────────────────────────────

    async fn list_history(&self, _feature_id: &FeatureId) -> StoreResult<Vec<FeatureHistory>> {
        // History is stored as issue comments — not yet implemented for read-only phase
        Ok(Vec::new())
    }

    async fn add_history(&self, _input: &CreateHistoryInput) -> StoreResult<FeatureHistory> {
        Err(unsupported("add_history"))
    }

    // ── Blockers ────────────────────────────────────────────────────────

    async fn get_blockers(&self, _feature_id: &FeatureId) -> StoreResult<Vec<FeatureSummary>> {
        // Blockers require parsing issue body cross-references — not yet implemented
        Ok(Vec::new())
    }

    async fn get_blocked_by(&self, _feature_id: &FeatureId) -> StoreResult<Vec<FeatureSummary>> {
        Ok(Vec::new())
    }

    async fn set_blockers(
        &self,
        _feature_id: &FeatureId,
        _blocked_by_ids: &[FeatureId],
    ) -> StoreResult<()> {
        Err(unsupported("set_blockers"))
    }

    // ── Sync Metadata ───────────────────────────────────────────────────

    async fn last_synced_at(&self) -> StoreResult<Option<DateTime<Utc>>> {
        Ok(self.snapshot.read().await.last_synced_at)
    }

    async fn set_last_synced_at(&self, at: DateTime<Utc>) -> StoreResult<()> {
        self.snapshot.write().await.last_synced_at = Some(at);
        Ok(())
    }

    // ── Capabilities ────────────────────────────────────────────────────

    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            offline_writes: false,
            realtime_sync: false,
            fulltext_search: false,
            transactions: false,
            external_ui: true,
            backend_type: BackendType::GitHub,
            max_detail_length: Some(65536), // GitHub issue body limit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sync_data() -> SyncData {
        SyncData {
            issues: vec![
                GitHubIssue {
                    id: "I_1".to_string(),
                    number: 1,
                    title: "Auth".to_string(),
                    body: Some("<!-- manifest:feature -->\n<!-- manifest:id:550e8400-e29b-41d4-a716-446655440000 -->\n\nAuth system details".to_string()),
                    state: "OPEN".to_string(),
                    labels: vec!["manifest:in_progress".to_string(), "manifest:feature_set".to_string()],
                    milestone_title: Some("0.1.0".to_string()),
                    milestone_id: Some("MI_1".to_string()),
                    parent_number: None,
                    sub_issue_numbers: vec![2],
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                    updated_at: "2026-01-02T00:00:00Z".to_string(),
                },
                GitHubIssue {
                    id: "I_2".to_string(),
                    number: 2,
                    title: "OAuth Login".to_string(),
                    body: Some("<!-- manifest:feature -->\n<!-- manifest:id:660e8400-e29b-41d4-a716-446655440000 -->\n\nOAuth details".to_string()),
                    state: "OPEN".to_string(),
                    labels: vec!["manifest:proposed".to_string()],
                    milestone_title: None,
                    milestone_id: None,
                    parent_number: Some(1),
                    sub_issue_numbers: vec![],
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                    updated_at: "2026-01-01T00:00:00Z".to_string(),
                },
            ],
            milestones: vec![GitHubMilestone {
                id: "MI_1".to_string(),
                number: 1,
                title: "0.1.0".to_string(),
                description: Some("First release".to_string()),
                state: "OPEN".to_string(),
                due_on: None,
                created_at: "2026-01-01T00:00:00Z".to_string(),
                updated_at: "2026-01-01T00:00:00Z".to_string(),
            }],
        }
    }

    fn make_store() -> GitHubStore {
        let client = GitHubClient::new("test/repo", "fake-token".to_string()).unwrap();
        let project_id = ProjectId::new();
        GitHubStore::new(client, project_id)
    }

    #[test]
    fn build_snapshot_creates_features() {
        let store = make_store();
        let data = make_sync_data();
        let snapshot = store.build_snapshot(data).unwrap();

        assert_eq!(snapshot.features.len(), 2);
        assert_eq!(snapshot.versions.len(), 1);
        assert!(snapshot.project.is_some());
    }

    #[test]
    fn build_snapshot_resolves_parent() {
        let store = make_store();
        let data = make_sync_data();
        let snapshot = store.build_snapshot(data).unwrap();

        let child = snapshot
            .features
            .iter()
            .find(|f| f.title == "OAuth Login")
            .unwrap();
        let parent = snapshot
            .features
            .iter()
            .find(|f| f.title == "Auth")
            .unwrap();

        assert_eq!(child.parent_id, Some(parent.id));
    }

    #[test]
    fn build_snapshot_extracts_state() {
        let store = make_store();
        let data = make_sync_data();
        let snapshot = store.build_snapshot(data).unwrap();

        let auth = snapshot
            .features
            .iter()
            .find(|f| f.title == "Auth")
            .unwrap();
        assert_eq!(auth.state, FeatureState::InProgress);

        let oauth = snapshot
            .features
            .iter()
            .find(|f| f.title == "OAuth Login")
            .unwrap();
        assert_eq!(oauth.state, FeatureState::Proposed);
    }

    #[test]
    fn build_snapshot_resolves_version() {
        let store = make_store();
        let data = make_sync_data();
        let snapshot = store.build_snapshot(data).unwrap();

        let auth = snapshot
            .features
            .iter()
            .find(|f| f.title == "Auth")
            .unwrap();
        assert!(auth.target_version_id.is_some());

        let version = &snapshot.versions[0];
        assert_eq!(auth.target_version_id.unwrap(), version.id);
    }

    #[test]
    fn build_snapshot_parses_manifest_id() {
        let store = make_store();
        let data = make_sync_data();
        let snapshot = store.build_snapshot(data).unwrap();

        let auth = snapshot
            .features
            .iter()
            .find(|f| f.title == "Auth")
            .unwrap();
        assert_eq!(auth.id.to_string(), "550e8400-e29b-41d4-a716-446655440000");
    }

    #[test]
    fn capabilities_are_github() {
        let store = make_store();
        let caps = store.capabilities();
        assert_eq!(caps.backend_type, BackendType::GitHub);
        assert!(caps.external_ui);
        assert!(!caps.offline_writes);
        assert!(!caps.transactions);
    }

    #[tokio::test]
    async fn read_methods_work_after_snapshot() {
        let store = make_store();
        let data = make_sync_data();
        let snapshot = store.build_snapshot(data).unwrap();
        let project_id = snapshot.project.as_ref().unwrap().id;
        *store.snapshot.write().await = snapshot;

        // list_projects
        let projects = store.list_projects().await.unwrap();
        assert_eq!(projects.len(), 1);

        // get_project
        let project = store.get_project(&project_id).await.unwrap();
        assert_eq!(project.id, project_id);

        // list_features
        let features = store
            .list_features(&FeatureQuery::for_project(project_id))
            .await
            .unwrap();
        assert_eq!(features.len(), 2);

        // get_feature
        let auth = features.iter().find(|f| f.title == "Auth").unwrap();
        let fetched = store.get_feature(&auth.id).await.unwrap();
        assert_eq!(fetched.title, "Auth");

        // get_feature_children
        let children = store.get_feature_children(&auth.id).await.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].title, "OAuth Login");

        // list_versions
        let versions = store.list_versions(&project_id).await.unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].name, "0.1.0");
    }

    #[tokio::test]
    async fn write_methods_return_unsupported() {
        let store = make_store();

        let err = store
            .create_project(&CreateProjectInput {
                id: None,
                name: "P".to_string(),
                slug: None,
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: true,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, StoreError::Unsupported(_)));

        let err = store
            .create_feature(
                &ProjectId::new(),
                &CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "X".to_string(),
                    details: None,
                    state: None,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, StoreError::Unsupported(_)));

        let err = store.delete_feature(&FeatureId::new()).await.unwrap_err();
        assert!(matches!(err, StoreError::Unsupported(_)));
    }

    #[tokio::test]
    async fn list_features_with_filters() {
        let store = make_store();
        let data = make_sync_data();
        let snapshot = store.build_snapshot(data).unwrap();
        let project_id = snapshot.project.as_ref().unwrap().id;
        *store.snapshot.write().await = snapshot;

        // Filter by state
        let proposed = store
            .list_features(
                &FeatureQuery::for_project(project_id).with_state(FeatureState::Proposed),
            )
            .await
            .unwrap();
        assert_eq!(proposed.len(), 1);
        assert_eq!(proposed[0].title, "OAuth Login");

        // Filter root only
        let roots = store
            .list_features(&FeatureQuery::for_project(project_id).with_parent(ParentFilter::Root))
            .await
            .unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].title, "Auth");

        // Pagination
        let page = store
            .list_features(&FeatureQuery::for_project(project_id).with_limit(1))
            .await
            .unwrap();
        assert_eq!(page.len(), 1);
    }

    #[tokio::test]
    async fn get_feature_ancestors_builds_chain() {
        let store = make_store();
        let data = make_sync_data();
        let snapshot = store.build_snapshot(data).unwrap();
        let child_id = snapshot
            .features
            .iter()
            .find(|f| f.title == "OAuth Login")
            .unwrap()
            .id;
        *store.snapshot.write().await = snapshot;

        let ancestors = store.get_feature_ancestors(&child_id).await.unwrap();
        assert_eq!(ancestors.len(), 1);
        assert_eq!(ancestors[0].title, "Auth");
    }

    #[tokio::test]
    async fn not_found_returns_correct_errors() {
        let store = make_store();

        let err = store.get_feature(&FeatureId::new()).await.unwrap_err();
        assert!(matches!(err, StoreError::FeatureNotFound(_)));

        let err = store.get_project(&ProjectId::new()).await.unwrap_err();
        assert!(matches!(err, StoreError::ProjectNotFound(_)));

        let err = store.get_version(&VersionId::new()).await.unwrap_err();
        assert!(matches!(err, StoreError::VersionNotFound(_)));
    }

    #[tokio::test]
    async fn sync_metadata_works() {
        let store = make_store();
        assert!(store.last_synced_at().await.unwrap().is_none());

        let now = Utc::now();
        store.set_last_synced_at(now).await.unwrap();
        let stored = store.last_synced_at().await.unwrap().unwrap();
        assert_eq!(stored, now);
    }
}
