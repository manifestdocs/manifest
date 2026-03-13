//! Selective sync engine for Turso remotes.
//!
//! The sync engine coordinates data flow between the local SQLite database
//! and one or more Turso embedded replicas. Each remote represents an
//! organizational context (e.g., "work", "personal") with its own Turso DB.
//!
//! Architecture:
//! - Local DB (`manifest.db`) is the unified read source
//! - Each remote has an embedded replica (local SQLite + cloud sync)
//! - Write routing: writes to linked projects go through the appropriate remote
//! - Merge coordinator: incoming changes from remotes merge into local DB
//! - Offline queue: writes queue locally when remote is unreachable

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use crate::models::*;
use crate::turso::TursoConnection;

/// Convert an `Option<String>` to a `libsql::Value`, using `Null` for `None`.
fn opt_to_value(opt: &Option<String>) -> libsql::Value {
    match opt {
        Some(s) => libsql::Value::Text(s.clone()),
        None => libsql::Value::Null,
    }
}

/// A row from the remote `features` table, used for merge operations.
#[derive(Debug, Clone)]
pub struct RemoteFeature {
    pub id: String,
    pub project_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub details: Option<String>,
    pub desired_details: Option<String>,
    pub details_summary: Option<String>,
    pub state: String,
    pub priority: i64,
    pub feature_number: Option<i64>,
    pub target_version_id: Option<String>,
    pub claimed_by: Option<String>,
    pub claimed_at: Option<String>,
    pub claim_metadata: Option<String>,
    pub verification_result: Option<String>,
    pub verified_at: Option<String>,
    pub state_updated_at: Option<String>,
    pub details_updated_at: Option<String>,
    pub parent_id_updated_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A row from the remote `projects` table.
#[derive(Debug, Clone)]
pub struct RemoteProject {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub instructions: Option<String>,
    pub current_version_id: Option<String>,
    pub root_feature_id: Option<String>,
    pub default_feature_destination: String,
    pub test_adapter: Option<String>,
    pub context_budget: Option<i64>,
    pub key_prefix: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A row from the remote `versions` table.
#[derive(Debug, Clone)]
pub struct RemoteVersion {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub description: Option<String>,
    pub released_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Sync direction for a single operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    /// Push local changes to remote.
    Push,
    /// Pull remote changes to local.
    Pull,
}

/// Result of a single sync operation.
#[derive(Debug, Clone)]
pub struct SyncResult {
    pub remote_name: String,
    pub project_id: String,
    pub direction: SyncDirection,
    pub features_pushed: usize,
    pub features_pulled: usize,
    pub versions_pushed: usize,
    pub versions_pulled: usize,
    pub conflicts_resolved: usize,
}

/// Status of sync for a project-remote pair.
#[derive(Debug, Clone)]
pub struct ProjectSyncStatus {
    pub project_id: ProjectId,
    pub project_name: String,
    pub remote_id: RemoteId,
    pub remote_name: String,
    pub sync_state: SyncState,
    pub last_synced_at: Option<DateTime<Utc>>,
    pub pending_push: usize,
    pub pending_pull: usize,
}

/// An entry in the offline write queue.
#[derive(Debug, Clone)]
pub struct QueuedWrite {
    pub id: i64,
    pub project_id: String,
    pub remote_id: String,
    pub table_name: String,
    pub row_id: String,
    pub operation: WriteOperation,
    pub payload: String, // JSON-serialized row data
    pub created_at: DateTime<Utc>,
}

/// Type of queued write operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOperation {
    Upsert,
    Delete,
}

impl WriteOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            WriteOperation::Upsert => "upsert",
            WriteOperation::Delete => "delete",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "upsert" => Some(WriteOperation::Upsert),
            "delete" => Some(WriteOperation::Delete),
            _ => None,
        }
    }
}

/// Field-level conflict resolution: compare timestamps and keep the newest.
///
/// Returns the merged value and which side won.
fn resolve_field<T: Clone>(
    local_val: &T,
    local_ts: Option<&DateTime<Utc>>,
    remote_val: &T,
    remote_ts: Option<&DateTime<Utc>>,
) -> (T, SyncDirection) {
    match (local_ts, remote_ts) {
        (Some(l), Some(r)) => {
            if l >= r {
                (local_val.clone(), SyncDirection::Push)
            } else {
                (remote_val.clone(), SyncDirection::Pull)
            }
        }
        (Some(_), None) => (local_val.clone(), SyncDirection::Push),
        (None, Some(_)) => (remote_val.clone(), SyncDirection::Pull),
        // No timestamps on either side: remote wins (pull bias for new data)
        (None, None) => (remote_val.clone(), SyncDirection::Pull),
    }
}

/// Parse an RFC3339 timestamp string, returning None if missing or invalid.
fn parse_ts(s: Option<&str>) -> Option<DateTime<Utc>> {
    s.and_then(|v| DateTime::parse_from_rfc3339(v).ok())
        .map(|dt| dt.with_timezone(&Utc))
}

/// The merge coordinator manages sync between local DB and Turso remotes.
///
/// It holds references to active remote connections and orchestrates
/// push/pull operations for linked projects.
pub struct MergeCoordinator {
    /// Active Turso connections keyed by remote ID.
    connections: Arc<RwLock<HashMap<String, TursoConnection>>>,
}

impl MergeCoordinator {
    /// Create a new merge coordinator with no active connections.
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a remote connection.
    pub async fn add_connection(&self, remote_id: &str, conn: TursoConnection) {
        let mut conns = self.connections.write().await;
        conns.insert(remote_id.to_string(), conn);
    }

    /// Remove a remote connection.
    pub async fn remove_connection(&self, remote_id: &str) {
        let mut conns = self.connections.write().await;
        conns.remove(remote_id);
    }

    /// Check if a remote connection is active.
    pub async fn has_connection(&self, remote_id: &str) -> bool {
        let conns = self.connections.read().await;
        conns.contains_key(remote_id)
    }

    /// Get the number of active connections.
    pub async fn connection_count(&self) -> usize {
        let conns = self.connections.read().await;
        conns.len()
    }

    /// Push a project's features from local to a remote.
    ///
    /// Reads features from the local DB (via the provided query function)
    /// and upserts them into the remote Turso connection.
    pub async fn push_features(
        &self,
        remote_id: &str,
        features: &[RemoteFeature],
    ) -> Result<usize> {
        let conns = self.connections.read().await;
        let conn = conns
            .get(remote_id)
            .ok_or_else(|| anyhow::anyhow!("Remote '{}' not connected", remote_id))?;

        let c = conn.connect()?;
        let mut pushed = 0;

        for f in features {
            let params = vec![
                libsql::Value::Text(f.id.clone()),
                libsql::Value::Text(f.project_id.clone()),
                opt_to_value(&f.parent_id),
                libsql::Value::Text(f.title.clone()),
                opt_to_value(&f.details),
                opt_to_value(&f.desired_details),
                opt_to_value(&f.details_summary),
                libsql::Value::Text(f.state.clone()),
                libsql::Value::Integer(f.priority),
                f.feature_number
                    .map_or(libsql::Value::Null, libsql::Value::Integer),
                opt_to_value(&f.target_version_id),
                opt_to_value(&f.claimed_by),
                opt_to_value(&f.claimed_at),
                opt_to_value(&f.claim_metadata),
                opt_to_value(&f.verification_result),
                opt_to_value(&f.verified_at),
                opt_to_value(&f.state_updated_at),
                opt_to_value(&f.details_updated_at),
                opt_to_value(&f.parent_id_updated_at),
                libsql::Value::Text(f.created_at.clone()),
                libsql::Value::Text(f.updated_at.clone()),
            ];
            c.execute(
                "INSERT OR REPLACE INTO features (id, project_id, parent_id, title, details, desired_details, details_summary, state, priority, feature_number, target_version_id, claimed_by, claimed_at, claim_metadata, verification_result, verified_at, state_updated_at, details_updated_at, parent_id_updated_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
                params,
            )
            .await
            .with_context(|| format!("pushing feature {}", f.id))?;
            pushed += 1;
        }

        // Sync to push writes to the cloud primary
        conn.sync().await?;
        Ok(pushed)
    }

    /// Pull features from a remote for a specific project.
    ///
    /// Returns the remote features so the caller can merge them into the local DB.
    pub async fn pull_features(
        &self,
        remote_id: &str,
        project_id: &str,
    ) -> Result<Vec<RemoteFeature>> {
        let conns = self.connections.read().await;
        let conn = conns
            .get(remote_id)
            .ok_or_else(|| anyhow::anyhow!("Remote '{}' not connected", remote_id))?;

        // Sync first to get latest from cloud
        conn.sync().await?;

        let c = conn.connect()?;
        let mut rows = c
            .query(
                "SELECT id, project_id, parent_id, title, details, desired_details, details_summary, state, priority, feature_number, target_version_id, claimed_by, claimed_at, claim_metadata, verification_result, verified_at, state_updated_at, details_updated_at, parent_id_updated_at, created_at, updated_at FROM features WHERE project_id = ?1",
                libsql::params![project_id],
            )
            .await
            .context("querying remote features")?;

        let mut features = Vec::new();
        while let Some(row) = rows.next().await? {
            let get_str = |idx: i32| -> String { row.get::<String>(idx).unwrap_or_default() };
            let get_opt = |idx: i32| -> Option<String> {
                match row.get_value(idx) {
                    Ok(libsql::Value::Text(s)) if !s.is_empty() => Some(s),
                    _ => None,
                }
            };
            let get_opt_i64 = |idx: i32| -> Option<i64> {
                match row.get_value(idx) {
                    Ok(libsql::Value::Integer(v)) if v != 0 => Some(v),
                    _ => None,
                }
            };

            features.push(RemoteFeature {
                id: get_str(0),
                project_id: get_str(1),
                parent_id: get_opt(2),
                title: get_str(3),
                details: get_opt(4),
                desired_details: get_opt(5),
                details_summary: get_opt(6),
                state: get_str(7),
                priority: row.get::<i64>(8).unwrap_or(0),
                feature_number: get_opt_i64(9),
                target_version_id: get_opt(10),
                claimed_by: get_opt(11),
                claimed_at: get_opt(12),
                claim_metadata: get_opt(13),
                verification_result: get_opt(14),
                verified_at: get_opt(15),
                state_updated_at: get_opt(16),
                details_updated_at: get_opt(17),
                parent_id_updated_at: get_opt(18),
                created_at: get_str(19),
                updated_at: get_str(20),
            });
        }

        Ok(features)
    }

    /// Push versions from local to a remote.
    pub async fn push_versions(
        &self,
        remote_id: &str,
        versions: &[RemoteVersion],
    ) -> Result<usize> {
        let conns = self.connections.read().await;
        let conn = conns
            .get(remote_id)
            .ok_or_else(|| anyhow::anyhow!("Remote '{}' not connected", remote_id))?;

        let c = conn.connect()?;
        let mut pushed = 0;

        for v in versions {
            let params = vec![
                libsql::Value::Text(v.id.clone()),
                libsql::Value::Text(v.project_id.clone()),
                libsql::Value::Text(v.name.clone()),
                opt_to_value(&v.description),
                opt_to_value(&v.released_at),
                libsql::Value::Text(v.created_at.clone()),
                libsql::Value::Text(v.updated_at.clone()),
            ];
            c.execute(
                "INSERT OR REPLACE INTO versions (id, project_id, name, description, released_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params,
            )
            .await
            .with_context(|| format!("pushing version {}", v.id))?;
            pushed += 1;
        }

        conn.sync().await?;
        Ok(pushed)
    }

    /// Push a project from local to a remote.
    pub async fn push_project(&self, remote_id: &str, project: &RemoteProject) -> Result<()> {
        let conns = self.connections.read().await;
        let conn = conns
            .get(remote_id)
            .ok_or_else(|| anyhow::anyhow!("Remote '{}' not connected", remote_id))?;

        let c = conn.connect()?;
        let params = vec![
            libsql::Value::Text(project.id.clone()),
            libsql::Value::Text(project.slug.clone()),
            libsql::Value::Text(project.name.clone()),
            opt_to_value(&project.description),
            opt_to_value(&project.instructions),
            opt_to_value(&project.current_version_id),
            opt_to_value(&project.root_feature_id),
            libsql::Value::Text(project.default_feature_destination.clone()),
            opt_to_value(&project.test_adapter),
            project
                .context_budget
                .map_or(libsql::Value::Null, libsql::Value::Integer),
            libsql::Value::Text(project.key_prefix.clone()),
            libsql::Value::Text(project.created_at.clone()),
            libsql::Value::Text(project.updated_at.clone()),
        ];
        c.execute(
            "INSERT OR REPLACE INTO projects (id, slug, name, description, instructions, current_version_id, root_feature_id, default_feature_destination, test_adapter, context_budget, key_prefix, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params,
        )
        .await
        .context("pushing project")?;

        conn.sync().await?;
        Ok(())
    }

    /// Merge a set of remote features into local features using field-level timestamps.
    ///
    /// Returns (merged_features, conflicts_resolved) where merged_features
    /// contains the winning values for each field.
    pub fn merge_features(
        local: &[RemoteFeature],
        remote: &[RemoteFeature],
    ) -> (Vec<RemoteFeature>, usize) {
        let mut local_map: HashMap<&str, &RemoteFeature> = HashMap::new();
        for f in local {
            local_map.insert(&f.id, f);
        }

        let mut merged = Vec::new();
        let mut conflicts = 0;

        // Process all remote features
        for rf in remote {
            if let Some(lf) = local_map.remove(rf.id.as_str()) {
                // Feature exists in both — field-level merge
                let local_state_ts = parse_ts(lf.state_updated_at.as_deref());
                let remote_state_ts = parse_ts(rf.state_updated_at.as_deref());
                let local_details_ts = parse_ts(lf.details_updated_at.as_deref());
                let remote_details_ts = parse_ts(rf.details_updated_at.as_deref());
                let local_parent_ts = parse_ts(lf.parent_id_updated_at.as_deref());
                let remote_parent_ts = parse_ts(rf.parent_id_updated_at.as_deref());

                let (state, state_dir) = resolve_field(
                    &lf.state,
                    local_state_ts.as_ref(),
                    &rf.state,
                    remote_state_ts.as_ref(),
                );
                let (details, details_dir) = resolve_field(
                    &lf.details,
                    local_details_ts.as_ref(),
                    &rf.details,
                    remote_details_ts.as_ref(),
                );
                let (parent_id, parent_dir) = resolve_field(
                    &lf.parent_id,
                    local_parent_ts.as_ref(),
                    &rf.parent_id,
                    remote_parent_ts.as_ref(),
                );

                // Count conflicts (different values resolved)
                if lf.state != rf.state {
                    conflicts += 1;
                }
                if lf.details != rf.details {
                    conflicts += 1;
                }
                if lf.parent_id != rf.parent_id {
                    conflicts += 1;
                }

                // For non-timestamp-tracked fields, use the most recent updated_at
                let local_updated = parse_ts(Some(&lf.updated_at));
                let remote_updated = parse_ts(Some(&rf.updated_at));
                let use_remote_for_rest = match (local_updated, remote_updated) {
                    (Some(l), Some(r)) => r > l,
                    _ => true,
                };

                let base = if use_remote_for_rest { rf } else { lf };

                merged.push(RemoteFeature {
                    id: lf.id.clone(),
                    project_id: lf.project_id.clone(),
                    parent_id,
                    title: base.title.clone(),
                    details,
                    desired_details: base.desired_details.clone(),
                    details_summary: base.details_summary.clone(),
                    state,
                    priority: base.priority,
                    feature_number: base.feature_number,
                    target_version_id: base.target_version_id.clone(),
                    claimed_by: base.claimed_by.clone(),
                    claimed_at: base.claimed_at.clone(),
                    claim_metadata: base.claim_metadata.clone(),
                    verification_result: base.verification_result.clone(),
                    verified_at: base.verified_at.clone(),
                    state_updated_at: match state_dir {
                        SyncDirection::Push => lf.state_updated_at.clone(),
                        SyncDirection::Pull => rf.state_updated_at.clone(),
                    },
                    details_updated_at: match details_dir {
                        SyncDirection::Push => lf.details_updated_at.clone(),
                        SyncDirection::Pull => rf.details_updated_at.clone(),
                    },
                    parent_id_updated_at: match parent_dir {
                        SyncDirection::Push => lf.parent_id_updated_at.clone(),
                        SyncDirection::Pull => rf.parent_id_updated_at.clone(),
                    },
                    created_at: lf.created_at.clone(),
                    updated_at: base.updated_at.clone(),
                });
            } else {
                // Feature only on remote — pull it
                merged.push(rf.clone());
            }
        }

        // Features only on local — keep them
        for (_, lf) in local_map {
            merged.push(lf.clone());
        }

        (merged, conflicts)
    }
}

impl Default for MergeCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Manages the offline write queue.
///
/// When a remote is unreachable, writes are queued in a local table
/// and flushed when the connection is restored.
pub struct OfflineQueue {
    writes: Arc<RwLock<Vec<QueuedWrite>>>,
    next_id: Arc<RwLock<i64>>,
}

impl OfflineQueue {
    pub fn new() -> Self {
        Self {
            writes: Arc::new(RwLock::new(Vec::new())),
            next_id: Arc::new(RwLock::new(1)),
        }
    }

    /// Queue a write for later delivery.
    pub async fn enqueue(
        &self,
        project_id: &str,
        remote_id: &str,
        table_name: &str,
        row_id: &str,
        operation: WriteOperation,
        payload: &str,
    ) -> i64 {
        let mut id_guard = self.next_id.write().await;
        let id = *id_guard;
        *id_guard += 1;

        let write = QueuedWrite {
            id,
            project_id: project_id.to_string(),
            remote_id: remote_id.to_string(),
            table_name: table_name.to_string(),
            row_id: row_id.to_string(),
            operation,
            payload: payload.to_string(),
            created_at: Utc::now(),
        };

        let mut writes = self.writes.write().await;
        writes.push(write);
        id
    }

    /// Get all queued writes for a specific remote.
    pub async fn pending_for_remote(&self, remote_id: &str) -> Vec<QueuedWrite> {
        let writes = self.writes.read().await;
        writes
            .iter()
            .filter(|w| w.remote_id == remote_id)
            .cloned()
            .collect()
    }

    /// Get all queued writes.
    pub async fn pending_count(&self) -> usize {
        let writes = self.writes.read().await;
        writes.len()
    }

    /// Remove a write from the queue after successful delivery.
    pub async fn dequeue(&self, id: i64) -> bool {
        let mut writes = self.writes.write().await;
        let before = writes.len();
        writes.retain(|w| w.id != id);
        writes.len() < before
    }

    /// Flush all pending writes for a remote through the given coordinator.
    ///
    /// Returns the number of writes successfully flushed.
    pub async fn flush_remote(
        &self,
        remote_id: &str,
        coordinator: &MergeCoordinator,
    ) -> Result<usize> {
        let pending = self.pending_for_remote(remote_id).await;
        let mut flushed = 0;

        for write in &pending {
            match write.table_name.as_str() {
                "features" => {
                    if let Ok(feature) = serde_json::from_str::<RemoteFeature>(&write.payload) {
                        match write.operation {
                            WriteOperation::Upsert => {
                                coordinator.push_features(remote_id, &[feature]).await?;
                            }
                            WriteOperation::Delete => {
                                // Delete from remote
                                let conns = coordinator.connections.read().await;
                                if let Some(conn) = conns.get(remote_id) {
                                    let c = conn.connect()?;
                                    c.execute(
                                        "DELETE FROM features WHERE id = ?1",
                                        libsql::params![write.row_id.as_str()],
                                    )
                                    .await?;
                                    conn.sync().await?;
                                }
                            }
                        }
                        self.dequeue(write.id).await;
                        flushed += 1;
                    }
                }
                _ => {
                    tracing::warn!("Unknown table in offline queue: {}", write.table_name);
                }
            }
        }

        Ok(flushed)
    }
}

impl Default for OfflineQueue {
    fn default() -> Self {
        Self::new()
    }
}

// Implement serde for RemoteFeature so it can be serialized in the offline queue
impl serde::Serialize for RemoteFeature {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("RemoteFeature", 21)?;
        s.serialize_field("id", &self.id)?;
        s.serialize_field("project_id", &self.project_id)?;
        s.serialize_field("parent_id", &self.parent_id)?;
        s.serialize_field("title", &self.title)?;
        s.serialize_field("details", &self.details)?;
        s.serialize_field("desired_details", &self.desired_details)?;
        s.serialize_field("details_summary", &self.details_summary)?;
        s.serialize_field("state", &self.state)?;
        s.serialize_field("priority", &self.priority)?;
        s.serialize_field("feature_number", &self.feature_number)?;
        s.serialize_field("target_version_id", &self.target_version_id)?;
        s.serialize_field("claimed_by", &self.claimed_by)?;
        s.serialize_field("claimed_at", &self.claimed_at)?;
        s.serialize_field("claim_metadata", &self.claim_metadata)?;
        s.serialize_field("verification_result", &self.verification_result)?;
        s.serialize_field("verified_at", &self.verified_at)?;
        s.serialize_field("state_updated_at", &self.state_updated_at)?;
        s.serialize_field("details_updated_at", &self.details_updated_at)?;
        s.serialize_field("parent_id_updated_at", &self.parent_id_updated_at)?;
        s.serialize_field("created_at", &self.created_at)?;
        s.serialize_field("updated_at", &self.updated_at)?;
        s.end()
    }
}

impl<'de> serde::Deserialize<'de> for RemoteFeature {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Helper {
            id: String,
            project_id: String,
            parent_id: Option<String>,
            title: String,
            details: Option<String>,
            desired_details: Option<String>,
            details_summary: Option<String>,
            state: String,
            priority: i64,
            feature_number: Option<i64>,
            target_version_id: Option<String>,
            claimed_by: Option<String>,
            claimed_at: Option<String>,
            claim_metadata: Option<String>,
            verification_result: Option<String>,
            verified_at: Option<String>,
            state_updated_at: Option<String>,
            details_updated_at: Option<String>,
            parent_id_updated_at: Option<String>,
            created_at: String,
            updated_at: String,
        }
        let h = Helper::deserialize(deserializer)?;
        Ok(RemoteFeature {
            id: h.id,
            project_id: h.project_id,
            parent_id: h.parent_id,
            title: h.title,
            details: h.details,
            desired_details: h.desired_details,
            details_summary: h.details_summary,
            state: h.state,
            priority: h.priority,
            feature_number: h.feature_number,
            target_version_id: h.target_version_id,
            claimed_by: h.claimed_by,
            claimed_at: h.claimed_at,
            claim_metadata: h.claim_metadata,
            verification_result: h.verification_result,
            verified_at: h.verified_at,
            state_updated_at: h.state_updated_at,
            details_updated_at: h.details_updated_at,
            parent_id_updated_at: h.parent_id_updated_at,
            created_at: h.created_at,
            updated_at: h.updated_at,
        })
    }
}
