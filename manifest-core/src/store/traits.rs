//! The `FeatureStore` async trait — the storage contract all backends implement.

use async_trait::async_trait;

use crate::models::{
    CreateFeatureInput, CreateHistoryInput, CreateProjectInput, CreateVersionInput, Feature,
    FeatureHistory, FeatureId, FeatureSummary, Project, ProjectId, UpdateProjectInput,
    UpdateVersionInput, Version, VersionId,
};

use super::{FeatureChangeset, FeatureQuery, StoreCapabilities, StoreResult};

/// Backend-agnostic storage trait for Manifest's feature data.
///
/// All shared data operations (projects, features, versions, history, blockers) go through
/// this trait. The application receives `Arc<dyn FeatureStore>` and never knows which
/// backend is active.
///
/// # Object Safety
///
/// This trait is object-safe (`dyn FeatureStore` works). No generic methods, no `Self` in
/// return types, no associated types with constraints that prevent dynamic dispatch.
///
/// # Error Handling
///
/// All methods return `StoreResult<T>` (alias for `Result<T, StoreError>`).
/// Handler code matches on `StoreError::FeatureNotFound`, never on backend-specific errors.
///
/// # Async
///
/// All methods are async. The SQLite backend wraps sync `rusqlite` calls with
/// `tokio::task::spawn_blocking`. The GitHub backend makes HTTP calls. The Turso backend
/// uses `libsql`'s async connection interface.
#[async_trait]
pub trait FeatureStore: Send + Sync {
    // ── Projects ────────────────────────────────────────────────────────

    async fn get_project(&self, project_id: &ProjectId) -> StoreResult<Project>;

    async fn list_projects(&self) -> StoreResult<Vec<Project>>;

    async fn create_project(&self, input: &CreateProjectInput) -> StoreResult<Project>;

    async fn update_project(
        &self,
        project_id: &ProjectId,
        input: &UpdateProjectInput,
    ) -> StoreResult<Project>;

    // ── Features ────────────────────────────────────────────────────────

    async fn get_feature(&self, feature_id: &FeatureId) -> StoreResult<Feature>;

    async fn get_feature_by_number(
        &self,
        project_id: &ProjectId,
        feature_number: i32,
    ) -> StoreResult<Feature>;

    async fn list_features(&self, query: &FeatureQuery) -> StoreResult<Vec<Feature>>;

    async fn create_feature(
        &self,
        project_id: &ProjectId,
        input: &CreateFeatureInput,
    ) -> StoreResult<Feature>;

    /// Batch create features. Default implementation loops over `create_feature`.
    /// Backends can override with efficient implementations (e.g., SQLite wraps in a transaction).
    async fn create_features_batch(
        &self,
        project_id: &ProjectId,
        inputs: &[CreateFeatureInput],
    ) -> StoreResult<Vec<Feature>> {
        let mut features = Vec::with_capacity(inputs.len());
        for input in inputs {
            features.push(self.create_feature(project_id, input).await?);
        }
        Ok(features)
    }

    async fn update_feature(
        &self,
        feature_id: &FeatureId,
        changeset: &FeatureChangeset,
    ) -> StoreResult<Feature>;

    async fn delete_feature(&self, feature_id: &FeatureId) -> StoreResult<()>;

    async fn move_feature(
        &self,
        feature_id: &FeatureId,
        new_parent_id: Option<&FeatureId>,
        position: Option<i32>,
    ) -> StoreResult<()>;

    async fn get_feature_children(&self, feature_id: &FeatureId) -> StoreResult<Vec<Feature>>;

    async fn get_feature_ancestors(&self, feature_id: &FeatureId) -> StoreResult<Vec<Feature>>;

    // ── Versions ────────────────────────────────────────────────────────

    async fn get_version(&self, version_id: &VersionId) -> StoreResult<Version>;

    async fn list_versions(&self, project_id: &ProjectId) -> StoreResult<Vec<Version>>;

    async fn create_version(
        &self,
        project_id: &ProjectId,
        input: &CreateVersionInput,
    ) -> StoreResult<Version>;

    async fn update_version(
        &self,
        version_id: &VersionId,
        input: &UpdateVersionInput,
    ) -> StoreResult<Version>;

    async fn delete_version(&self, version_id: &VersionId) -> StoreResult<()>;

    // ── Feature History ─────────────────────────────────────────────────

    async fn list_history(&self, feature_id: &FeatureId) -> StoreResult<Vec<FeatureHistory>>;

    async fn add_history(&self, input: &CreateHistoryInput) -> StoreResult<FeatureHistory>;

    // ── Blockers ────────────────────────────────────────────────────────

    /// Get features that block this feature (its dependencies).
    async fn get_blockers(&self, feature_id: &FeatureId) -> StoreResult<Vec<FeatureSummary>>;

    /// Get features that this feature blocks (its dependents).
    async fn get_blocked_by(&self, feature_id: &FeatureId) -> StoreResult<Vec<FeatureSummary>>;

    /// Set the blocker list for a feature, replacing any existing blockers.
    async fn set_blockers(
        &self,
        feature_id: &FeatureId,
        blocked_by_ids: &[FeatureId],
    ) -> StoreResult<()>;

    // ── Sync Metadata ───────────────────────────────────────────────────

    /// Returns the last time this store was synced with a remote (if applicable).
    async fn last_synced_at(&self) -> StoreResult<Option<chrono::DateTime<chrono::Utc>>>;

    /// Records the sync timestamp.
    async fn set_last_synced_at(&self, at: chrono::DateTime<chrono::Utc>) -> StoreResult<()>;

    // ── Capabilities ────────────────────────────────────────────────────

    /// Declares what this backend can and cannot do.
    fn capabilities(&self) -> StoreCapabilities;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time proof that `FeatureStore` is object-safe.
    /// If this compiles, `dyn FeatureStore` works.
    fn _assert_object_safe(_store: &dyn FeatureStore) {}

    /// Compile-time proof that `dyn FeatureStore` can be wrapped in Arc.
    fn _assert_arc_compatible(_store: std::sync::Arc<dyn FeatureStore>) {}
}
