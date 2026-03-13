//! Caching wrapper for remote [`FeatureStore`] backends.
//!
//! `CachedStore` wraps a remote `FeatureStore` with a local `SqliteStore` cache.
//! Reads always come from the local cache for speed. Writes follow the configured
//! [`WriteStrategy`]: write-through (remote first, then cache) for GitHub-style
//! backends, or write-local (cache first, remote syncs separately) for Turso-style
//! backends. A periodic sync keeps the cache fresh from remote.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tokio::time::Instant;

use crate::models::*;

use super::{
    FeatureChangeset, FeatureQuery, FeatureStore, SqliteStore, StoreCapabilities, StoreError,
    StoreResult,
};

/// Controls how writes are routed between remote and cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteStrategy {
    /// Write remote first, then update cache on success (GitHub model).
    /// If remote is unavailable, write fails with [`StoreError::Unavailable`].
    WriteThrough,
    /// Write cache only; remote syncs via its own mechanism (Turso embedded replica model).
    WriteLocal,
}

/// Caching wrapper around a remote [`FeatureStore`].
///
/// Reads always serve from the local SQLite cache. Writes follow the configured
/// [`WriteStrategy`]. A background sync mechanism keeps the cache fresh.
///
/// # Type Parameter
///
/// `R` is the remote backend. Use concrete types for efficiency or
/// `Box<dyn FeatureStore>` for dynamic dispatch.
pub struct CachedStore<R: FeatureStore + 'static> {
    remote: Arc<R>,
    cache: Arc<SqliteStore>,
    sync_interval: Duration,
    write_strategy: WriteStrategy,
    last_sync: Arc<RwLock<Option<Instant>>>,
    online: Arc<AtomicBool>,
    syncing: Arc<AtomicBool>,
}

impl<R: FeatureStore + 'static> CachedStore<R> {
    /// Create a new `CachedStore` wrapping a remote backend with a local cache.
    pub fn new(
        remote: Arc<R>,
        cache: Arc<SqliteStore>,
        sync_interval: Duration,
        write_strategy: WriteStrategy,
    ) -> Self {
        Self {
            remote,
            cache,
            sync_interval,
            write_strategy,
            last_sync: Arc::new(RwLock::new(None)),
            online: Arc::new(AtomicBool::new(true)),
            syncing: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Whether the remote backend is currently reachable.
    pub fn is_online(&self) -> bool {
        self.online.load(Ordering::Relaxed)
    }

    /// Access the local cache store.
    pub fn cache(&self) -> &SqliteStore {
        &self.cache
    }

    /// Access the remote store.
    pub fn remote(&self) -> &R {
        &self.remote
    }

    /// Trigger a sync if the sync interval has elapsed.
    ///
    /// Non-blocking: spawns the sync as a background task. Reads are never
    /// delayed by sync. If a sync is already in progress, this is a no-op.
    pub async fn maybe_sync(&self) {
        let should_sync = {
            let last = self.last_sync.read().await;
            match *last {
                None => true,
                Some(t) => t.elapsed() >= self.sync_interval,
            }
        };

        if should_sync && !self.syncing.swap(true, Ordering::AcqRel) {
            let remote = Arc::clone(&self.remote);
            let cache = Arc::clone(&self.cache);
            let last_sync = Arc::clone(&self.last_sync);
            let online = Arc::clone(&self.online);
            let syncing = Arc::clone(&self.syncing);

            tokio::spawn(async move {
                match sync_remote_to_cache(remote.as_ref(), &cache).await {
                    Ok(()) => {
                        online.store(true, Ordering::Relaxed);
                        *last_sync.write().await = Some(Instant::now());
                    }
                    Err(StoreError::Unavailable(_)) => {
                        online.store(false, Ordering::Relaxed);
                    }
                    Err(_) => {
                        // Other errors don't change online status
                    }
                }
                syncing.store(false, Ordering::Release);
            });
        }
    }

    /// Run a one-shot sync from remote to cache. Blocks until complete.
    pub async fn sync_now(&self) -> StoreResult<()> {
        let result = sync_remote_to_cache(self.remote.as_ref(), &self.cache).await;
        match &result {
            Ok(()) => {
                self.online.store(true, Ordering::Relaxed);
                *self.last_sync.write().await = Some(Instant::now());
            }
            Err(StoreError::Unavailable(_)) => {
                self.online.store(false, Ordering::Relaxed);
            }
            Err(_) => {}
        }
        result
    }

    /// Spawn a background task that syncs on the configured interval.
    /// Returns a `JoinHandle` the caller can abort to stop the loop.
    pub fn spawn_sync_loop(&self) -> tokio::task::JoinHandle<()> {
        let remote = Arc::clone(&self.remote);
        let cache = Arc::clone(&self.cache);
        let last_sync = Arc::clone(&self.last_sync);
        let online = Arc::clone(&self.online);
        let interval = self.sync_interval;

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await; // First tick is immediate, skip it
            loop {
                ticker.tick().await;
                match sync_remote_to_cache(remote.as_ref(), &cache).await {
                    Ok(()) => {
                        online.store(true, Ordering::Relaxed);
                        *last_sync.write().await = Some(Instant::now());
                    }
                    Err(StoreError::Unavailable(_)) => {
                        online.store(false, Ordering::Relaxed);
                    }
                    Err(_) => {}
                }
            }
        })
    }

    /// Write to remote, then update cache on success. Returns `Unavailable` if offline.
    async fn write_through<T, Fremote, Fcache, Fut1, Fut2>(
        &self,
        remote_op: Fremote,
        cache_op: Fcache,
    ) -> StoreResult<T>
    where
        Fremote: FnOnce(Arc<R>) -> Fut1,
        Fcache: FnOnce(Arc<SqliteStore>, T) -> Fut2,
        Fut1: std::future::Future<Output = StoreResult<T>>,
        Fut2: std::future::Future<Output = StoreResult<T>>,
        T: Clone,
    {
        let result = remote_op(Arc::clone(&self.remote)).await;
        match result {
            Ok(val) => {
                self.online.store(true, Ordering::Relaxed);
                // Best-effort cache update; don't fail the operation if cache update fails
                let _ = cache_op(Arc::clone(&self.cache), val.clone()).await;
                Ok(val)
            }
            Err(StoreError::Unavailable(msg)) => {
                self.online.store(false, Ordering::Relaxed);
                Err(StoreError::Unavailable(msg))
            }
            Err(e) => Err(e),
        }
    }
}

/// Sync data from remote store into local cache.
///
/// Pulls projects, features, and versions from remote. For each entity:
/// - If it exists in cache with an older `updated_at`, update the cache copy.
/// - If it doesn't exist in cache, create it (preserving the remote ID).
///
/// Feature creates preserve IDs via `CreateFeatureInput.id`. Projects and versions
/// also support ID preservation via their respective input types.
async fn sync_remote_to_cache(
    remote: &(dyn FeatureStore + '_),
    cache: &SqliteStore,
) -> StoreResult<()> {
    let remote_projects = remote.list_projects().await?;

    for rp in &remote_projects {
        // Sync the project itself
        match cache.get_project(&rp.id).await {
            Ok(cached) => {
                if rp.updated_at > cached.updated_at {
                    let input = UpdateProjectInput {
                        name: Some(rp.name.clone()),
                        slug: Some(rp.slug.clone()),
                        description: rp.description.clone(),
                        instructions: rp.instructions.clone(),
                        ..Default::default()
                    };
                    let _ = cache.update_project(&rp.id, &input).await;
                }
            }
            Err(StoreError::ProjectNotFound(_)) => {
                let input = CreateProjectInput {
                    id: Some(rp.id),
                    name: rp.name.clone(),
                    slug: Some(rp.slug.clone()),
                    description: rp.description.clone(),
                    instructions: rp.instructions.clone(),
                    key_prefix: Some(rp.key_prefix.clone()),
                    skip_default_versions: true,
                };
                let _ = cache.create_project(&input).await;
            }
            Err(_) => continue,
        }

        // Sync features for this project
        let query = FeatureQuery::for_project(rp.id);
        let remote_features = match remote.list_features(&query).await {
            Ok(f) => f,
            Err(_) => continue,
        };

        for rf in &remote_features {
            match cache.get_feature(&rf.id).await {
                Ok(cached) => {
                    if rf.updated_at > cached.updated_at {
                        let changeset = FeatureChangeset {
                            title: Some(rf.title.clone()),
                            details: rf.details.clone(),
                            state: Some(rf.state),
                            priority: Some(rf.priority),
                            parent_id: Some(rf.parent_id),
                            target_version_id: Some(rf.target_version_id),
                            ..Default::default()
                        };
                        let _ = cache.update_feature(&rf.id, &changeset).await;
                    }
                }
                Err(StoreError::FeatureNotFound(_)) => {
                    // Try with parent_id first; fall back to None if parent doesn't
                    // exist in cache (e.g., root feature ID mismatch).
                    let input = CreateFeatureInput {
                        id: Some(rf.id),
                        parent_id: rf.parent_id,
                        title: rf.title.clone(),
                        details: rf.details.clone(),
                        state: Some(rf.state),
                        priority: Some(rf.priority),
                        target_version_id: rf.target_version_id,
                    };
                    if cache.create_feature(&rp.id, &input).await.is_err() {
                        let fallback = CreateFeatureInput {
                            parent_id: None,
                            ..input
                        };
                        let _ = cache.create_feature(&rp.id, &fallback).await;
                    }
                }
                Err(_) => continue,
            }
        }

        // Sync versions
        let remote_versions = match remote.list_versions(&rp.id).await {
            Ok(v) => v,
            Err(_) => continue,
        };

        for rv in &remote_versions {
            match cache.get_version(&rv.id).await {
                Ok(cached) => {
                    if rv.updated_at > cached.updated_at {
                        let input = UpdateVersionInput {
                            name: Some(rv.name.clone()),
                            description: rv.description.clone(),
                            released_at: rv.released_at,
                        };
                        let _ = cache.update_version(&rv.id, &input).await;
                    }
                }
                Err(StoreError::VersionNotFound(_)) => {
                    let input = CreateVersionInput {
                        id: Some(rv.id),
                        name: rv.name.clone(),
                        description: rv.description.clone(),
                    };
                    let _ = cache.create_version(&rp.id, &input).await;
                }
                Err(_) => continue,
            }
        }
    }

    Ok(())
}

#[async_trait]
impl<R: FeatureStore + 'static> FeatureStore for CachedStore<R> {
    // ── Projects ────────────────────────────────────────────────────────

    async fn get_project(&self, project_id: &ProjectId) -> StoreResult<Project> {
        self.maybe_sync().await;
        // Try cache first, fall back to remote on miss
        match self.cache.get_project(project_id).await {
            Ok(p) => Ok(p),
            Err(StoreError::ProjectNotFound(_)) => {
                let p = self.remote.get_project(project_id).await?;
                // Cache for next time (best-effort)
                let input = CreateProjectInput {
                    id: Some(p.id),
                    name: p.name.clone(),
                    slug: Some(p.slug.clone()),
                    description: p.description.clone(),
                    instructions: p.instructions.clone(),
                    key_prefix: Some(p.key_prefix.clone()),
                    skip_default_versions: true,
                };
                let _ = self.cache.create_project(&input).await;
                Ok(p)
            }
            Err(e) => Err(e),
        }
    }

    async fn list_projects(&self) -> StoreResult<Vec<Project>> {
        self.maybe_sync().await;
        self.cache.list_projects().await
    }

    async fn create_project(&self, input: &CreateProjectInput) -> StoreResult<Project> {
        match self.write_strategy {
            WriteStrategy::WriteThrough => {
                let input_clone = input.clone();
                self.write_through(
                    |remote| async move { remote.create_project(&input_clone).await },
                    |cache, project: Project| async move {
                        let cache_input = CreateProjectInput {
                            id: Some(project.id),
                            name: project.name.clone(),
                            slug: Some(project.slug.clone()),
                            description: project.description.clone(),
                            instructions: project.instructions.clone(),
                            key_prefix: Some(project.key_prefix.clone()),
                            skip_default_versions: true,
                        };
                        cache.create_project(&cache_input).await
                    },
                )
                .await
            }
            WriteStrategy::WriteLocal => self.cache.create_project(input).await,
        }
    }

    async fn update_project(
        &self,
        project_id: &ProjectId,
        input: &UpdateProjectInput,
    ) -> StoreResult<Project> {
        match self.write_strategy {
            WriteStrategy::WriteThrough => {
                let pid = *project_id;
                let inp = input.clone();
                let inp2 = inp.clone();
                self.write_through(
                    |remote| async move { remote.update_project(&pid, &inp).await },
                    |cache, _project: Project| async move {
                        cache.update_project(&pid, &inp2).await
                    },
                )
                .await
            }
            WriteStrategy::WriteLocal => self.cache.update_project(project_id, input).await,
        }
    }

    // ── Features ────────────────────────────────────────────────────────

    async fn get_feature(&self, feature_id: &FeatureId) -> StoreResult<Feature> {
        self.maybe_sync().await;
        match self.cache.get_feature(feature_id).await {
            Ok(f) => Ok(f),
            Err(StoreError::FeatureNotFound(_)) => {
                let f = self.remote.get_feature(feature_id).await?;
                // Cache for next time
                let input = CreateFeatureInput {
                    id: Some(f.id),
                    parent_id: f.parent_id,
                    title: f.title.clone(),
                    details: f.details.clone(),
                    state: Some(f.state),
                    priority: Some(f.priority),
                    target_version_id: f.target_version_id,
                };
                // Need project_id for create — get it from the feature
                let _ = self.cache.create_feature(&f.project_id, &input).await;
                Ok(f)
            }
            Err(e) => Err(e),
        }
    }

    async fn get_feature_by_number(
        &self,
        project_id: &ProjectId,
        feature_number: i32,
    ) -> StoreResult<Feature> {
        self.maybe_sync().await;
        match self
            .cache
            .get_feature_by_number(project_id, feature_number)
            .await
        {
            Ok(f) => Ok(f),
            Err(StoreError::FeatureNotFound(_)) => {
                self.remote
                    .get_feature_by_number(project_id, feature_number)
                    .await
            }
            Err(e) => Err(e),
        }
    }

    async fn list_features(&self, query: &FeatureQuery) -> StoreResult<Vec<Feature>> {
        self.maybe_sync().await;
        self.cache.list_features(query).await
    }

    async fn create_feature(
        &self,
        project_id: &ProjectId,
        input: &CreateFeatureInput,
    ) -> StoreResult<Feature> {
        match self.write_strategy {
            WriteStrategy::WriteThrough => {
                let pid = *project_id;
                let inp = input.clone();
                self.write_through(
                    |remote| async move { remote.create_feature(&pid, &inp).await },
                    |cache, feature: Feature| async move {
                        // Use parent_id=None for cache create: the cache's project has its
                        // own root feature, so remote parent IDs may not exist locally.
                        // The cache auto-assigns the project's root feature as parent.
                        let cache_input = CreateFeatureInput {
                            id: Some(feature.id),
                            parent_id: None,
                            title: feature.title.clone(),
                            details: feature.details.clone(),
                            state: Some(feature.state),
                            priority: Some(feature.priority),
                            target_version_id: feature.target_version_id,
                        };
                        cache.create_feature(&pid, &cache_input).await
                    },
                )
                .await
            }
            WriteStrategy::WriteLocal => self.cache.create_feature(project_id, input).await,
        }
    }

    async fn create_features_batch(
        &self,
        project_id: &ProjectId,
        inputs: &[CreateFeatureInput],
    ) -> StoreResult<Vec<Feature>> {
        match self.write_strategy {
            WriteStrategy::WriteThrough => {
                let pid = *project_id;
                let inp = inputs.to_vec();
                let features = self.remote.create_features_batch(&pid, &inp).await?;
                self.online.store(true, Ordering::Relaxed);
                // Cache each created feature with its remote-assigned ID
                let cache_inputs: Vec<_> = features
                    .iter()
                    .map(|f| CreateFeatureInput {
                        id: Some(f.id),
                        parent_id: None, // Let cache auto-assign parent
                        title: f.title.clone(),
                        details: f.details.clone(),
                        state: Some(f.state),
                        priority: Some(f.priority),
                        target_version_id: f.target_version_id,
                    })
                    .collect();
                let _ = self.cache.create_features_batch(&pid, &cache_inputs).await;
                Ok(features)
            }
            WriteStrategy::WriteLocal => self.cache.create_features_batch(project_id, inputs).await,
        }
    }

    async fn update_feature(
        &self,
        feature_id: &FeatureId,
        changeset: &FeatureChangeset,
    ) -> StoreResult<Feature> {
        match self.write_strategy {
            WriteStrategy::WriteThrough => {
                let fid = *feature_id;
                let cs = changeset.clone();
                let cs2 = cs.clone();
                self.write_through(
                    |remote| async move { remote.update_feature(&fid, &cs).await },
                    |cache, _feature: Feature| async move {
                        cache.update_feature(&fid, &cs2).await
                    },
                )
                .await
            }
            WriteStrategy::WriteLocal => self.cache.update_feature(feature_id, changeset).await,
        }
    }

    async fn delete_feature(&self, feature_id: &FeatureId) -> StoreResult<()> {
        match self.write_strategy {
            WriteStrategy::WriteThrough => {
                self.remote.delete_feature(feature_id).await?;
                self.online.store(true, Ordering::Relaxed);
                let _ = self.cache.delete_feature(feature_id).await;
                Ok(())
            }
            WriteStrategy::WriteLocal => self.cache.delete_feature(feature_id).await,
        }
    }

    async fn move_feature(
        &self,
        feature_id: &FeatureId,
        new_parent_id: Option<&FeatureId>,
        position: Option<i32>,
    ) -> StoreResult<()> {
        match self.write_strategy {
            WriteStrategy::WriteThrough => {
                self.remote
                    .move_feature(feature_id, new_parent_id, position)
                    .await?;
                self.online.store(true, Ordering::Relaxed);
                let _ = self
                    .cache
                    .move_feature(feature_id, new_parent_id, position)
                    .await;
                Ok(())
            }
            WriteStrategy::WriteLocal => {
                self.cache
                    .move_feature(feature_id, new_parent_id, position)
                    .await
            }
        }
    }

    async fn get_feature_children(&self, feature_id: &FeatureId) -> StoreResult<Vec<Feature>> {
        self.maybe_sync().await;
        self.cache.get_feature_children(feature_id).await
    }

    async fn get_feature_ancestors(&self, feature_id: &FeatureId) -> StoreResult<Vec<Feature>> {
        self.maybe_sync().await;
        self.cache.get_feature_ancestors(feature_id).await
    }

    // ── Versions ────────────────────────────────────────────────────────

    async fn get_version(&self, version_id: &VersionId) -> StoreResult<Version> {
        self.maybe_sync().await;
        match self.cache.get_version(version_id).await {
            Ok(v) => Ok(v),
            Err(StoreError::VersionNotFound(_)) => self.remote.get_version(version_id).await,
            Err(e) => Err(e),
        }
    }

    async fn list_versions(&self, project_id: &ProjectId) -> StoreResult<Vec<Version>> {
        self.maybe_sync().await;
        self.cache.list_versions(project_id).await
    }

    async fn create_version(
        &self,
        project_id: &ProjectId,
        input: &CreateVersionInput,
    ) -> StoreResult<Version> {
        match self.write_strategy {
            WriteStrategy::WriteThrough => {
                let pid = *project_id;
                let inp = input.clone();
                self.write_through(
                    |remote| async move { remote.create_version(&pid, &inp).await },
                    |cache, version: Version| async move {
                        let cache_input = CreateVersionInput {
                            id: Some(version.id),
                            name: version.name.clone(),
                            description: version.description.clone(),
                        };
                        cache.create_version(&pid, &cache_input).await
                    },
                )
                .await
            }
            WriteStrategy::WriteLocal => self.cache.create_version(project_id, input).await,
        }
    }

    async fn update_version(
        &self,
        version_id: &VersionId,
        input: &UpdateVersionInput,
    ) -> StoreResult<Version> {
        match self.write_strategy {
            WriteStrategy::WriteThrough => {
                let vid = *version_id;
                let inp = input.clone();
                let inp2 = inp.clone();
                self.write_through(
                    |remote| async move { remote.update_version(&vid, &inp).await },
                    |cache, _version: Version| async move {
                        cache.update_version(&vid, &inp2).await
                    },
                )
                .await
            }
            WriteStrategy::WriteLocal => self.cache.update_version(version_id, input).await,
        }
    }

    async fn delete_version(&self, version_id: &VersionId) -> StoreResult<()> {
        match self.write_strategy {
            WriteStrategy::WriteThrough => {
                self.remote.delete_version(version_id).await?;
                self.online.store(true, Ordering::Relaxed);
                let _ = self.cache.delete_version(version_id).await;
                Ok(())
            }
            WriteStrategy::WriteLocal => self.cache.delete_version(version_id).await,
        }
    }

    // ── Feature History ─────────────────────────────────────────────────

    async fn list_history(&self, feature_id: &FeatureId) -> StoreResult<Vec<FeatureHistory>> {
        self.maybe_sync().await;
        self.cache.list_history(feature_id).await
    }

    async fn add_history(&self, input: &CreateHistoryInput) -> StoreResult<FeatureHistory> {
        match self.write_strategy {
            WriteStrategy::WriteThrough => {
                let inp = input.clone();
                let entry = self.remote.add_history(&inp).await?;
                self.online.store(true, Ordering::Relaxed);
                let _ = self.cache.add_history(&inp).await;
                Ok(entry)
            }
            WriteStrategy::WriteLocal => self.cache.add_history(input).await,
        }
    }

    // ── Blockers ────────────────────────────────────────────────────────

    async fn get_blockers(&self, feature_id: &FeatureId) -> StoreResult<Vec<FeatureSummary>> {
        self.maybe_sync().await;
        self.cache.get_blockers(feature_id).await
    }

    async fn get_blocked_by(&self, feature_id: &FeatureId) -> StoreResult<Vec<FeatureSummary>> {
        self.maybe_sync().await;
        self.cache.get_blocked_by(feature_id).await
    }

    async fn set_blockers(
        &self,
        feature_id: &FeatureId,
        blocked_by_ids: &[FeatureId],
    ) -> StoreResult<()> {
        match self.write_strategy {
            WriteStrategy::WriteThrough => {
                self.remote.set_blockers(feature_id, blocked_by_ids).await?;
                self.online.store(true, Ordering::Relaxed);
                let _ = self.cache.set_blockers(feature_id, blocked_by_ids).await;
                Ok(())
            }
            WriteStrategy::WriteLocal => self.cache.set_blockers(feature_id, blocked_by_ids).await,
        }
    }

    // ── Sync Metadata ───────────────────────────────────────────────────

    async fn last_synced_at(&self) -> StoreResult<Option<chrono::DateTime<chrono::Utc>>> {
        let last = self.last_sync.read().await;
        match *last {
            Some(instant) => {
                // Convert Instant to DateTime<Utc> approximately
                let elapsed = instant.elapsed();
                Ok(Some(chrono::Utc::now() - elapsed))
            }
            None => Ok(None),
        }
    }

    async fn set_last_synced_at(&self, _at: chrono::DateTime<chrono::Utc>) -> StoreResult<()> {
        *self.last_sync.write().await = Some(Instant::now());
        Ok(())
    }

    // ── Capabilities ────────────────────────────────────────────────────

    fn capabilities(&self) -> StoreCapabilities {
        let remote_caps = self.remote.capabilities();
        let cache_caps = self.cache.capabilities();
        // Merge: cache provides local capabilities, remote provides network capabilities
        StoreCapabilities {
            offline_writes: matches!(self.write_strategy, WriteStrategy::WriteLocal),
            realtime_sync: remote_caps.realtime_sync,
            fulltext_search: cache_caps.fulltext_search, // Cache enables local search
            transactions: cache_caps.transactions,       // Cache enables local transactions
            external_ui: remote_caps.external_ui,
            backend_type: remote_caps.backend_type,
            max_detail_length: remote_caps.max_detail_length,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::store::BackendType;

    /// Create a fresh in-memory SqliteStore for testing.
    async fn new_store() -> Arc<SqliteStore> {
        let db = Database::open_memory().await.expect("open memory db");
        db.migrate().await.expect("migrate");
        Arc::new(SqliteStore::new(Arc::new(db)))
    }

    /// Create a project in a store and return it.
    async fn create_test_project(store: &dyn FeatureStore, slug: &str) -> Project {
        store
            .create_project(&CreateProjectInput {
                id: None,
                name: format!("Project {slug}"),
                slug: Some(slug.to_string()),
                description: None,
                instructions: None,
                key_prefix: None,
                skip_default_versions: true,
            })
            .await
            .expect("create project")
    }

    /// Create a CachedStore with WriteThrough and short sync interval.
    async fn setup_write_through() -> (CachedStore<SqliteStore>, Arc<SqliteStore>, Arc<SqliteStore>)
    {
        let remote = new_store().await;
        let cache = new_store().await;
        let cached = CachedStore::new(
            Arc::clone(&remote),
            Arc::clone(&cache),
            Duration::from_secs(60),
            WriteStrategy::WriteThrough,
        );
        (cached, remote, cache)
    }

    /// Create a CachedStore with WriteLocal and short sync interval.
    async fn setup_write_local() -> (CachedStore<SqliteStore>, Arc<SqliteStore>, Arc<SqliteStore>) {
        let remote = new_store().await;
        let cache = new_store().await;
        let cached = CachedStore::new(
            Arc::clone(&remote),
            Arc::clone(&cache),
            Duration::from_secs(60),
            WriteStrategy::WriteLocal,
        );
        (cached, remote, cache)
    }

    #[tokio::test]
    async fn write_through_creates_in_both_stores() {
        let (cached, remote, cache) = setup_write_through().await;
        let project = create_test_project(&cached, "wt1").await;

        // Verify project exists in both remote and cache
        assert!(remote.get_project(&project.id).await.is_ok());
        assert!(cache.get_project(&project.id).await.is_ok());
    }

    #[tokio::test]
    async fn write_through_feature_in_both_stores() {
        let (cached, remote, cache) = setup_write_through().await;
        let project = create_test_project(&cached, "wt2").await;

        let feature = cached
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

        // Feature exists in both
        assert!(remote.get_feature(&feature.id).await.is_ok());
        assert!(cache.get_feature(&feature.id).await.is_ok());
    }

    #[tokio::test]
    async fn write_local_only_writes_to_cache() {
        let (cached, remote, cache) = setup_write_local().await;
        let project = create_test_project(&cached, "wl1").await;

        // In WriteLocal, project only goes to cache
        assert!(remote.get_project(&project.id).await.is_err());
        assert!(cache.get_project(&project.id).await.is_ok());

        let feature = cached
            .create_feature(
                &project.id,
                &CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Local Feature".to_string(),
                    details: None,
                    state: None,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        // Feature only in cache
        assert!(remote.get_feature(&feature.id).await.is_err());
        assert!(cache.get_feature(&feature.id).await.is_ok());
        assert_eq!(
            cache.get_feature(&feature.id).await.unwrap().title,
            "Local Feature"
        );
    }

    #[tokio::test]
    async fn reads_come_from_cache() {
        let (cached, _remote, cache) = setup_write_through().await;

        // Pre-populate cache directly
        let project = create_test_project(cache.as_ref(), "cache1").await;

        // Read through CachedStore — should find it in cache
        let fetched = cached.get_project(&project.id).await.unwrap();
        assert_eq!(fetched.name, project.name);
    }

    #[tokio::test]
    async fn cache_miss_falls_through_to_remote() {
        let (cached, remote, _cache) = setup_write_through().await;

        // Create project only in remote
        let project = create_test_project(remote.as_ref(), "remote1").await;

        // CachedStore should find it via remote fallback
        let fetched = cached.get_project(&project.id).await.unwrap();
        assert_eq!(fetched.id, project.id);
    }

    #[tokio::test]
    async fn sync_copies_remote_to_cache() {
        let remote = new_store().await;
        let cache = new_store().await;
        let cached = CachedStore::new(
            Arc::clone(&remote),
            Arc::clone(&cache),
            Duration::from_secs(60),
            WriteStrategy::WriteThrough,
        );

        // Create data in remote directly
        let project = create_test_project(remote.as_ref(), "sync1").await;
        remote
            .create_feature(
                &project.id,
                &CreateFeatureInput {
                    id: None,
                    parent_id: None,
                    title: "Remote Feature".to_string(),
                    details: Some("synced".to_string()),
                    state: None,
                    priority: None,
                    target_version_id: None,
                },
            )
            .await
            .unwrap();

        // Cache is empty
        assert!(cache.list_projects().await.unwrap().is_empty());

        // Sync
        cached.sync_now().await.unwrap();

        // Cache now has the data
        let cache_projects = cache.list_projects().await.unwrap();
        assert_eq!(cache_projects.len(), 1);
        assert_eq!(cache_projects[0].name, "Project sync1");

        let query = FeatureQuery::for_project(project.id);
        let cache_features = cache.list_features(&query).await.unwrap();
        // Root feature + our feature
        assert!(cache_features.len() >= 1);
    }

    #[tokio::test]
    async fn sync_updates_stale_cache_data() {
        let remote = new_store().await;
        let cache = new_store().await;
        let cached = CachedStore::new(
            Arc::clone(&remote),
            Arc::clone(&cache),
            Duration::from_secs(60),
            WriteStrategy::WriteThrough,
        );

        // Create same project in both via write-through
        let project = create_test_project(&cached, "stale1").await;

        // Create feature via write-through
        let feature = cached
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

        // Update feature directly on remote (simulating another user)
        let changeset = FeatureChangeset::default()
            .title("Updated by remote")
            .details("new details");
        remote
            .update_feature(&feature.id, &changeset)
            .await
            .unwrap();

        // Cache still has old data
        assert_eq!(
            cache.get_feature(&feature.id).await.unwrap().title,
            "Original"
        );

        // Sync
        cached.sync_now().await.unwrap();

        // Cache now has updated data
        let synced = cache.get_feature(&feature.id).await.unwrap();
        assert_eq!(synced.title, "Updated by remote");
        assert_eq!(synced.details.as_deref(), Some("new details"));
    }

    #[tokio::test]
    async fn maybe_sync_respects_interval() {
        let remote = new_store().await;
        let cache = new_store().await;
        let cached = CachedStore::new(
            Arc::clone(&remote),
            Arc::clone(&cache),
            Duration::from_secs(3600), // Very long interval
            WriteStrategy::WriteThrough,
        );

        // First sync
        cached.sync_now().await.unwrap();

        // Create data in remote after sync
        create_test_project(remote.as_ref(), "nosync").await;

        // maybe_sync should NOT trigger because interval hasn't elapsed
        cached.maybe_sync().await;
        // Give any spawned task a moment
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Cache should still be empty (sync was skipped)
        assert!(cache.list_projects().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn capabilities_merge_remote_and_cache() {
        let (cached, _remote, _cache) = setup_write_through().await;
        let caps = cached.capabilities();

        // WriteThrough: offline_writes = false
        assert!(!caps.offline_writes);
        // Cache provides search and transactions
        assert!(caps.fulltext_search);
        assert!(caps.transactions);
        // Remote backend type preserved
        assert_eq!(caps.backend_type, BackendType::Sqlite);
    }

    #[tokio::test]
    async fn write_local_capabilities_show_offline_writes() {
        let (cached, _remote, _cache) = setup_write_local().await;
        let caps = cached.capabilities();
        assert!(caps.offline_writes);
    }

    #[tokio::test]
    async fn online_status_starts_true() {
        let (cached, _remote, _cache) = setup_write_through().await;
        assert!(cached.is_online());
    }

    #[tokio::test]
    async fn version_write_through() {
        let (cached, remote, cache) = setup_write_through().await;
        let project = create_test_project(&cached, "ver1").await;

        let version = cached
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

        // Version in both stores
        assert!(remote.get_version(&version.id).await.is_ok());
        assert!(cache.get_version(&version.id).await.is_ok());
    }
}
