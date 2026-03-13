//! SQLite implementation of [`FeatureStore`], wrapping the existing [`Database`].
//!
//! This is an adapter that delegates to the existing `Database` methods, translating
//! between the store-layer types (`FeatureChangeset`, `FeatureQuery`, `StoreError`)
//! and the existing DB-layer types (`UpdateFeatureInput`, `ManifestError`).
//!
//! All existing SQL lives in `db/features.rs`, `db/projects.rs`, etc. This module
//! does not contain SQL — it translates and delegates.

use std::sync::Arc;

use async_trait::async_trait;

use crate::db::{Database, ManifestError};
use crate::models::*;

use super::{
    BackendType, FeatureChangeset, FeatureQuery, ParentFilter, StoreCapabilities, StoreError,
    StoreResult,
};

/// SQLite-backed implementation of [`FeatureStore`].
///
/// Wraps the existing [`Database`] and delegates all shared data operations.
/// Local-only operations (sessions, tasks, proofs, directories) are accessed
/// directly via [`database()`](SqliteStore::database).
pub struct SqliteStore {
    db: Arc<Database>,
}

impl SqliteStore {
    /// Create a new `SqliteStore` wrapping an existing `Database`.
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Access the underlying `Database` for local-only operations
    /// (sessions, tasks, proofs, directories, migrations, events).
    pub fn database(&self) -> &Database {
        &self.db
    }

    /// Access the underlying `Database` as an Arc for sharing.
    pub fn database_arc(&self) -> Arc<Database> {
        Arc::clone(&self.db)
    }
}

/// Convert an `anyhow::Error` to a `StoreError`, checking for known `ManifestError` variants.
fn map_error(err: anyhow::Error) -> StoreError {
    // Check if the underlying error is a ManifestError
    if let Some(me) = err.downcast_ref::<ManifestError>() {
        match me {
            ManifestError::NotFound(msg) => {
                // Try to determine entity type from message
                if msg.contains("feature") || msg.contains("Feature") {
                    return StoreError::FeatureNotFound(msg.clone());
                }
                if msg.contains("version") || msg.contains("Version") {
                    return StoreError::VersionNotFound(msg.clone());
                }
                if msg.contains("project") || msg.contains("Project") {
                    return StoreError::ProjectNotFound(msg.clone());
                }
                // Generic not-found — default to feature
                return StoreError::FeatureNotFound(msg.clone());
            }
            ManifestError::Validation(msg) => return StoreError::Validation(msg.clone()),
            ManifestError::InvalidState(msg) => return StoreError::Validation(msg.clone()),
            ManifestError::ClaimConflict(_) => {
                return StoreError::Validation(me.to_string());
            }
        }
    }

    // Fall through to internal error
    StoreError::Internal(err)
}

/// Convert `Result<Option<T>>` (DB convention) to `StoreResult<T>` (store convention).
fn require<T>(result: anyhow::Result<Option<T>>, entity: &str, id: &str) -> StoreResult<T> {
    match result {
        Ok(Some(val)) => Ok(val),
        Ok(None) => {
            let err_msg = format!("{entity} not found: {id}");
            match entity {
                "Feature" => Err(StoreError::FeatureNotFound(err_msg)),
                "Version" => Err(StoreError::VersionNotFound(err_msg)),
                "Project" => Err(StoreError::ProjectNotFound(err_msg)),
                _ => Err(StoreError::FeatureNotFound(err_msg)),
            }
        }
        Err(e) => Err(map_error(e)),
    }
}

/// Convert a `FeatureChangeset` to an `UpdateFeatureInput` for the existing DB layer.
fn changeset_to_input(changeset: &FeatureChangeset) -> UpdateFeatureInput {
    UpdateFeatureInput {
        title: changeset.title.clone(),
        details: changeset.details.clone(),
        desired_details: changeset.desired_details.clone(),
        details_summary: changeset.details_summary.clone(),
        state: changeset.state,
        priority: changeset.priority,
        parent_id: changeset.parent_id.map(|opt| opt.unwrap_or_default()),
        target_version_id: changeset.target_version_id,
        blocked_by: None,
    }
}

#[async_trait]
impl super::FeatureStore for SqliteStore {
    // ── Projects ────────────────────────────────────────────────────────

    async fn get_project(&self, project_id: &ProjectId) -> StoreResult<Project> {
        require(
            self.db.get_project(*project_id).await,
            "Project",
            &project_id.to_string(),
        )
    }

    async fn list_projects(&self) -> StoreResult<Vec<Project>> {
        self.db.get_all_projects().await.map_err(map_error)
    }

    async fn create_project(&self, input: &CreateProjectInput) -> StoreResult<Project> {
        self.db
            .create_project(input.clone())
            .await
            .map_err(map_error)
    }

    async fn update_project(
        &self,
        project_id: &ProjectId,
        input: &UpdateProjectInput,
    ) -> StoreResult<Project> {
        require(
            self.db.update_project(*project_id, input.clone()).await,
            "Project",
            &project_id.to_string(),
        )
    }

    // ── Features ────────────────────────────────────────────────────────

    async fn get_feature(&self, feature_id: &FeatureId) -> StoreResult<Feature> {
        require(
            self.db.get_feature(*feature_id).await,
            "Feature",
            &feature_id.to_string(),
        )
    }

    async fn get_feature_by_number(
        &self,
        project_id: &ProjectId,
        feature_number: i32,
    ) -> StoreResult<Feature> {
        let features = self
            .db
            .get_features_by_project(*project_id)
            .await
            .map_err(map_error)?;
        features
            .into_iter()
            .find(|f| f.feature_number == Some(feature_number))
            .ok_or_else(|| {
                StoreError::FeatureNotFound(format!(
                    "Feature #{feature_number} not found in project {project_id}"
                ))
            })
    }

    async fn list_features(&self, query: &FeatureQuery) -> StoreResult<Vec<Feature>> {
        // If search is specified, use search
        if let Some(ref search) = query.search {
            // search_features returns FeatureSummary; fetch full features by ID
            let summaries = self
                .db
                .search_features(search, query.project_id, query.limit.map(|l| l as u32))
                .await
                .map_err(map_error)?;
            let mut features = Vec::with_capacity(summaries.len());
            for s in summaries {
                if let Some(f) = self.db.get_feature(s.id).await.map_err(map_error)? {
                    features.push(f);
                }
            }
            return Ok(features);
        }

        // If parent filter is specified
        if let Some(ref parent) = query.parent_id {
            match parent {
                ParentFilter::Root => {
                    let project_id = query.project_id.ok_or_else(|| {
                        StoreError::Validation(
                            "project_id required for root feature queries".to_string(),
                        )
                    })?;
                    return self
                        .db
                        .get_root_features(project_id)
                        .await
                        .map_err(map_error);
                }
                ParentFilter::Exact(parent_id) => {
                    return self.db.get_children(*parent_id).await.map_err(map_error);
                }
                ParentFilter::Any => {} // Fall through to general query
            }
        }

        // General query: project-scoped with optional version/pagination
        if let Some(project_id) = query.project_id {
            let version_id = query.target_version_id;
            return self
                .db
                .get_features_by_project_paginated(
                    project_id,
                    version_id,
                    query.limit.map(|l| l as u32),
                    query.offset.map(|o| o as u32),
                )
                .await
                .map_err(map_error);
        }

        // No project filter: all features
        self.db
            .get_all_features_paginated(
                query.target_version_id,
                query.limit.map(|l| l as u32),
                query.offset.map(|o| o as u32),
            )
            .await
            .map_err(map_error)
    }

    async fn create_feature(
        &self,
        project_id: &ProjectId,
        input: &CreateFeatureInput,
    ) -> StoreResult<Feature> {
        self.db
            .create_feature(*project_id, input.clone())
            .await
            .map_err(map_error)
    }

    async fn create_features_batch(
        &self,
        project_id: &ProjectId,
        inputs: &[CreateFeatureInput],
    ) -> StoreResult<Vec<Feature>> {
        self.db
            .create_features_bulk(*project_id, inputs.to_vec())
            .await
            .map_err(map_error)
    }

    async fn update_feature(
        &self,
        feature_id: &FeatureId,
        changeset: &FeatureChangeset,
    ) -> StoreResult<Feature> {
        let input = changeset_to_input(changeset);
        require(
            self.db.update_feature(*feature_id, input).await,
            "Feature",
            &feature_id.to_string(),
        )
    }

    async fn delete_feature(&self, feature_id: &FeatureId) -> StoreResult<()> {
        let deleted = self
            .db
            .delete_feature(*feature_id)
            .await
            .map_err(map_error)?;
        if deleted {
            Ok(())
        } else {
            Err(StoreError::FeatureNotFound(format!(
                "Feature not found: {feature_id}"
            )))
        }
    }

    async fn move_feature(
        &self,
        feature_id: &FeatureId,
        new_parent_id: Option<&FeatureId>,
        position: Option<i32>,
    ) -> StoreResult<()> {
        let mut changeset = FeatureChangeset::default();
        changeset.parent_id = Some(new_parent_id.copied());
        if let Some(pos) = position {
            changeset.priority = Some(pos);
        }
        let input = changeset_to_input(&changeset);
        require(
            self.db.update_feature(*feature_id, input).await,
            "Feature",
            &feature_id.to_string(),
        )
        .map(|_| ())
    }

    async fn get_feature_children(&self, feature_id: &FeatureId) -> StoreResult<Vec<Feature>> {
        self.db.get_children(*feature_id).await.map_err(map_error)
    }

    async fn get_feature_ancestors(&self, feature_id: &FeatureId) -> StoreResult<Vec<Feature>> {
        // Build ancestor chain by walking up parent_id
        let mut ancestors = Vec::new();
        let mut current_id = *feature_id;
        loop {
            let feature = match self.db.get_feature(current_id).await.map_err(map_error)? {
                Some(f) => f,
                None => break,
            };
            match feature.parent_id {
                Some(pid) => {
                    current_id = pid;
                    if let Some(parent) = self.db.get_feature(pid).await.map_err(map_error)? {
                        ancestors.push(parent);
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
        require(
            self.db.get_version(*version_id).await,
            "Version",
            &version_id.to_string(),
        )
    }

    async fn list_versions(&self, project_id: &ProjectId) -> StoreResult<Vec<Version>> {
        self.db
            .get_versions_by_project(*project_id)
            .await
            .map_err(map_error)
    }

    async fn create_version(
        &self,
        project_id: &ProjectId,
        input: &CreateVersionInput,
    ) -> StoreResult<Version> {
        self.db
            .create_version(*project_id, input.clone())
            .await
            .map_err(map_error)
    }

    async fn update_version(
        &self,
        version_id: &VersionId,
        input: &UpdateVersionInput,
    ) -> StoreResult<Version> {
        require(
            self.db.update_version(*version_id, input.clone()).await,
            "Version",
            &version_id.to_string(),
        )
    }

    async fn delete_version(&self, version_id: &VersionId) -> StoreResult<()> {
        let deleted = self
            .db
            .delete_version(*version_id)
            .await
            .map_err(map_error)?;
        if deleted {
            Ok(())
        } else {
            Err(StoreError::VersionNotFound(format!(
                "Version not found: {version_id}"
            )))
        }
    }

    // ── Feature History ─────────────────────────────────────────────────

    async fn list_history(&self, feature_id: &FeatureId) -> StoreResult<Vec<FeatureHistory>> {
        self.db
            .get_feature_history(*feature_id)
            .await
            .map_err(map_error)
    }

    async fn add_history(&self, input: &CreateHistoryInput) -> StoreResult<FeatureHistory> {
        self.db
            .create_history_entry(input.clone())
            .await
            .map_err(map_error)
    }

    // ── Blockers ────────────────────────────────────────────────────────

    async fn get_blockers(&self, feature_id: &FeatureId) -> StoreResult<Vec<FeatureSummary>> {
        self.db
            .get_feature_blockers(*feature_id)
            .await
            .map_err(map_error)
    }

    async fn get_blocked_by(&self, feature_id: &FeatureId) -> StoreResult<Vec<FeatureSummary>> {
        self.db
            .get_feature_dependents(*feature_id)
            .await
            .map_err(map_error)
    }

    async fn set_blockers(
        &self,
        feature_id: &FeatureId,
        blocked_by_ids: &[FeatureId],
    ) -> StoreResult<()> {
        self.db
            .set_feature_blockers(*feature_id, blocked_by_ids)
            .await
            .map_err(map_error)
    }

    // ── Sync Metadata ───────────────────────────────────────────────────

    async fn last_synced_at(&self) -> StoreResult<Option<chrono::DateTime<chrono::Utc>>> {
        // SQLite is local-only, no sync concept
        Ok(None)
    }

    async fn set_last_synced_at(&self, _at: chrono::DateTime<chrono::Utc>) -> StoreResult<()> {
        // SQLite is local-only, no-op
        Ok(())
    }

    // ── Capabilities ────────────────────────────────────────────────────

    fn capabilities(&self) -> StoreCapabilities {
        StoreCapabilities {
            offline_writes: true,
            realtime_sync: false,
            fulltext_search: true,
            transactions: true,
            external_ui: false,
            backend_type: BackendType::Sqlite,
            max_detail_length: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::FeatureStore;

    async fn setup() -> SqliteStore {
        let db = Database::open_memory().await.expect("open memory db");
        db.migrate().await.expect("migrate");
        SqliteStore::new(Arc::new(db))
    }

    #[tokio::test]
    async fn create_and_get_project() {
        let store = setup().await;
        let input = CreateProjectInput {
            id: None,
            name: "Test Project".to_string(),
            slug: Some("test".to_string()),
            description: None,
            instructions: None,
            key_prefix: None,
            skip_default_versions: true,
        };
        let project = store.create_project(&input).await.unwrap();
        assert_eq!(project.name, "Test Project");

        let fetched = store.get_project(&project.id).await.unwrap();
        assert_eq!(fetched.id, project.id);
    }

    #[tokio::test]
    async fn list_projects_empty() {
        let store = setup().await;
        let projects = store.list_projects().await.unwrap();
        assert!(projects.is_empty());
    }

    #[tokio::test]
    async fn create_and_get_feature() {
        let store = setup().await;

        let project = store
            .create_project(&CreateProjectInput {
                id: None,
                name: "P".to_string(),
                slug: Some("p".to_string()),
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: true,
            })
            .await
            .unwrap();

        let feature = store
            .create_feature(
                &project.id,
                &CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Auth".to_string(),
                    details: Some("Login flow".to_string()),
                    state: None,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(feature.title, "Auth");
        assert_eq!(feature.details.as_deref(), Some("Login flow"));

        let fetched = store.get_feature(&feature.id).await.unwrap();
        assert_eq!(fetched.id, feature.id);
    }

    #[tokio::test]
    async fn update_feature_via_changeset() {
        let store = setup().await;

        let project = store
            .create_project(&CreateProjectInput {
                id: None,
                name: "P".to_string(),
                slug: Some("p2".to_string()),
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: true,
            })
            .await
            .unwrap();

        let feature = store
            .create_feature(
                &project.id,
                &CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Original".to_string(),
                    details: None,
                    state: None,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        let changeset = FeatureChangeset::default()
            .title("Updated")
            .details("New details")
            .state(FeatureState::InProgress);

        let updated = store.update_feature(&feature.id, &changeset).await.unwrap();
        assert_eq!(updated.title, "Updated");
        assert_eq!(updated.details.as_deref(), Some("New details"));
        assert_eq!(updated.state, FeatureState::InProgress);
    }

    #[tokio::test]
    async fn list_features_by_project() {
        let store = setup().await;

        let project = store
            .create_project(&CreateProjectInput {
                id: None,
                name: "P".to_string(),
                slug: Some("p3".to_string()),
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: true,
            })
            .await
            .unwrap();

        store
            .create_feature(
                &project.id,
                &CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "F1".to_string(),
                    details: None,
                    state: None,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        store
            .create_feature(
                &project.id,
                &CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "F2".to_string(),
                    details: None,
                    state: None,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        let query = FeatureQuery::for_project(project.id);
        let features = store.list_features(&query).await.unwrap();
        // Project creates a root feature + our 2 = 3
        assert!(features.len() >= 2);
    }

    #[tokio::test]
    async fn delete_feature_returns_error_for_missing() {
        let store = setup().await;
        let id = FeatureId::new();
        let result = store.delete_feature(&id).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            StoreError::FeatureNotFound(_)
        ));
    }

    #[tokio::test]
    async fn version_lifecycle() {
        let store = setup().await;

        let project = store
            .create_project(&CreateProjectInput {
                id: None,
                name: "P".to_string(),
                slug: Some("p4".to_string()),
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: true,
            })
            .await
            .unwrap();

        let version = store
            .create_version(
                &project.id,
                &CreateVersionInput {
                    id: None,
                    name: "1.0.0".to_string(),
                    description: Some("First release".to_string()),
                },
            )
            .await
            .unwrap();

        assert_eq!(version.name, "1.0.0");

        let versions = store.list_versions(&project.id).await.unwrap();
        assert_eq!(versions.len(), 1);

        let fetched = store.get_version(&version.id).await.unwrap();
        assert_eq!(fetched.id, version.id);

        store.delete_version(&version.id).await.unwrap();
        let versions = store.list_versions(&project.id).await.unwrap();
        assert!(versions.is_empty());
    }

    #[tokio::test]
    async fn history_add_and_list() {
        let store = setup().await;

        let project = store
            .create_project(&CreateProjectInput {
                id: None,
                name: "P".to_string(),
                slug: Some("p5".to_string()),
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: true,
            })
            .await
            .unwrap();

        let feature = store
            .create_feature(
                &project.id,
                &CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "F".to_string(),
                    details: None,
                    state: None,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        let entry = store
            .add_history(&CreateHistoryInput {
                feature_id: feature.id,
                version_id: None,
                details: HistoryDetails {
                    summary: "Initial work".to_string(),
                    commits: vec![],
                    backfilled: false,
                },
            })
            .await
            .unwrap();

        assert_eq!(entry.details.summary, "Initial work");

        let history = store.list_history(&feature.id).await.unwrap();
        assert_eq!(history.len(), 1);
    }

    #[tokio::test]
    async fn blockers_set_and_get() {
        let store = setup().await;

        let project = store
            .create_project(&CreateProjectInput {
                id: None,
                name: "P".to_string(),
                slug: Some("p6".to_string()),
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: true,
            })
            .await
            .unwrap();

        let f1 = store
            .create_feature(
                &project.id,
                &CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Blocker".to_string(),
                    details: None,
                    state: None,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        let f2 = store
            .create_feature(
                &project.id,
                &CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Blocked".to_string(),
                    details: None,
                    state: None,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        store.set_blockers(&f2.id, &[f1.id]).await.unwrap();

        let blockers = store.get_blockers(&f2.id).await.unwrap();
        assert_eq!(blockers.len(), 1);
        assert_eq!(blockers[0].id, f1.id);

        let dependents = store.get_blocked_by(&f1.id).await.unwrap();
        assert_eq!(dependents.len(), 1);
        assert_eq!(dependents[0].id, f2.id);
    }

    #[tokio::test]
    async fn capabilities_are_sqlite() {
        let store = setup().await;
        let caps = store.capabilities();
        assert_eq!(caps.backend_type, BackendType::Sqlite);
        assert!(caps.transactions);
        assert!(caps.fulltext_search);
        assert!(!caps.realtime_sync);
    }

    #[tokio::test]
    async fn sync_metadata_noop_for_sqlite() {
        let store = setup().await;
        assert!(store.last_synced_at().await.unwrap().is_none());
        store.set_last_synced_at(chrono::Utc::now()).await.unwrap();
        // Still None — SQLite doesn't track sync
        assert!(store.last_synced_at().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn get_feature_children_works() {
        let store = setup().await;

        let project = store
            .create_project(&CreateProjectInput {
                id: None,
                name: "P".to_string(),
                slug: Some("p7".to_string()),
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: true,
            })
            .await
            .unwrap();

        let parent = store
            .create_feature(
                &project.id,
                &CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Parent".to_string(),
                    details: None,
                    state: None,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        store
            .create_feature(
                &project.id,
                &CreateFeatureInput {
                    id: None,
                    parent_id: Some(parent.id),
                    title: "Child".to_string(),
                    details: None,
                    state: None,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        let children = store.get_feature_children(&parent.id).await.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].title, "Child");
    }

    #[tokio::test]
    async fn batch_create_features() {
        let store = setup().await;

        let project = store
            .create_project(&CreateProjectInput {
                id: None,
                name: "P".to_string(),
                slug: Some("p8".to_string()),
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: true,
            })
            .await
            .unwrap();

        let inputs = vec![
            CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "A".to_string(),
                details: None,
                state: None,
                priority: None,
                target_version_id: None,
            },
            CreateFeatureInput {
                id: None,
                parent_id: None,
                title: "B".to_string(),
                details: None,
                state: None,
                priority: None,
                target_version_id: None,
            },
        ];

        let features = store
            .create_features_batch(&project.id, &inputs)
            .await
            .unwrap();
        assert_eq!(features.len(), 2);
    }
}
