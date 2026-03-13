//! Changeset, capabilities, and configuration types for the storage layer.

use std::path::PathBuf;

use crate::models::{FeatureId, FeatureState, VersionId};

/// Field-level changeset for updating a feature.
///
/// Uses `Option` fields to represent "only the fields that changed". This is critical because:
/// - **SQLite backend**: generates a dynamic `UPDATE` with only changed columns
/// - **Turso backend**: updates field-level `_updated_at` timestamps for conflict resolution
/// - **GitHub backend**: translates state changes to label swaps, details changes to issue body
///   edits, version changes to milestone assignment -- each a different API call
///
/// If the trait only accepted whole `Feature` structs, every backend would need to diff against
/// the previous state. The changeset makes the intent explicit.
#[derive(Debug, Clone, Default)]
pub struct FeatureChangeset {
    pub title: Option<String>,
    pub details: Option<String>,
    /// `Some(None)` = clear desired_details, `Some(Some(v))` = set it, `None` = don't change.
    pub desired_details: Option<Option<String>>,
    /// `Some(None)` = clear summary, `Some(Some(v))` = set it, `None` = don't change.
    pub details_summary: Option<Option<String>>,
    pub state: Option<FeatureState>,
    pub priority: Option<i32>,
    /// `Some(None)` = unparent (make root), `Some(Some(id))` = reparent, `None` = don't change.
    pub parent_id: Option<Option<FeatureId>>,
    /// `Some(None)` = unassign version, `Some(Some(id))` = assign, `None` = don't change.
    pub target_version_id: Option<Option<VersionId>>,
    pub feature_number: Option<i32>,
}

impl FeatureChangeset {
    pub fn state(mut self, state: FeatureState) -> Self {
        self.state = Some(state);
        self
    }

    pub fn details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = Some(priority);
        self
    }

    pub fn parent_id(mut self, parent_id: Option<FeatureId>) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    pub fn target_version_id(mut self, version_id: Option<VersionId>) -> Self {
        self.target_version_id = Some(version_id);
        self
    }

    pub fn desired_details(mut self, desired: Option<String>) -> Self {
        self.desired_details = Some(desired);
        self
    }

    pub fn details_summary(mut self, summary: Option<String>) -> Self {
        self.details_summary = Some(summary);
        self
    }

    /// Returns true if no fields are set for update.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.details.is_none()
            && self.desired_details.is_none()
            && self.details_summary.is_none()
            && self.state.is_none()
            && self.priority.is_none()
            && self.parent_id.is_none()
            && self.target_version_id.is_none()
            && self.feature_number.is_none()
    }

    /// Returns the names of fields that are set for update.
    pub fn changed_fields(&self) -> Vec<&'static str> {
        let mut fields = Vec::new();
        if self.title.is_some() {
            fields.push("title");
        }
        if self.details.is_some() {
            fields.push("details");
        }
        if self.desired_details.is_some() {
            fields.push("desired_details");
        }
        if self.details_summary.is_some() {
            fields.push("details_summary");
        }
        if self.state.is_some() {
            fields.push("state");
        }
        if self.priority.is_some() {
            fields.push("priority");
        }
        if self.parent_id.is_some() {
            fields.push("parent_id");
        }
        if self.target_version_id.is_some() {
            fields.push("target_version_id");
        }
        if self.feature_number.is_some() {
            fields.push("feature_number");
        }
        fields
    }
}

/// Declares what a storage backend can and cannot do.
///
/// Application code uses capabilities to adapt behavior rather than checking backend type.
#[derive(Debug, Clone)]
pub struct StoreCapabilities {
    /// Backend supports offline writes (queued for later sync).
    pub offline_writes: bool,
    /// Backend supports real-time sync (changes from other users appear automatically).
    pub realtime_sync: bool,
    /// Backend supports full-text search in feature details.
    pub fulltext_search: bool,
    /// Backend supports transactions (atomic multi-operation changes).
    pub transactions: bool,
    /// Backend provides a web UI for non-CLI users (e.g., GitHub Issues).
    pub external_ui: bool,
    /// Backend type identifier.
    pub backend_type: BackendType,
    /// Maximum feature detail length (None = unlimited).
    pub max_detail_length: Option<usize>,
}

impl StoreCapabilities {
    /// Default capabilities for a local SQLite backend.
    pub fn sqlite() -> Self {
        Self {
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

/// Identifies the storage backend type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendType {
    Sqlite,
    Turso,
    GitHub,
    Custom(String),
}

impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite => write!(f, "sqlite"),
            Self::Turso => write!(f, "turso"),
            Self::GitHub => write!(f, "github"),
            Self::Custom(name) => write!(f, "custom:{name}"),
        }
    }
}

/// Configuration for selecting and connecting to a storage backend.
///
/// Typically loaded from a config file or environment. The `create_store` factory
/// reads this and returns a `Box<dyn FeatureStore>`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type")]
pub enum BackendConfig {
    #[serde(rename = "sqlite")]
    Sqlite { path: Option<PathBuf> },

    #[serde(rename = "turso")]
    Turso {
        url: String,
        /// Token resolved from keychain, env var, or this field.
        token: Option<String>,
        sync_interval_secs: Option<u64>,
    },

    #[serde(rename = "github")]
    GitHub {
        /// "owner/repo", inferred from git remote if absent.
        repo: Option<String>,
        /// Resolved from gh auth, env var, or this field.
        token: Option<String>,
        sync_interval_secs: Option<u64>,
        project_number: Option<i32>,
    },
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self::Sqlite { path: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changeset_default_is_empty() {
        let cs = FeatureChangeset::default();
        assert!(cs.is_empty());
        assert!(cs.changed_fields().is_empty());
    }

    #[test]
    fn changeset_builder_tracks_fields() {
        let cs = FeatureChangeset::default()
            .state(FeatureState::InProgress)
            .details("new details");

        assert!(!cs.is_empty());
        let fields = cs.changed_fields();
        assert_eq!(fields.len(), 2);
        assert!(fields.contains(&"state"));
        assert!(fields.contains(&"details"));
    }

    #[test]
    fn changeset_title_builder() {
        let cs = FeatureChangeset::default().title("New Title");
        assert_eq!(cs.title.as_deref(), Some("New Title"));
        assert_eq!(cs.changed_fields(), vec!["title"]);
    }

    #[test]
    fn changeset_priority_builder() {
        let cs = FeatureChangeset::default().priority(5);
        assert_eq!(cs.priority, Some(5));
        assert_eq!(cs.changed_fields(), vec!["priority"]);
    }

    #[test]
    fn changeset_parent_id_builder() {
        let id = FeatureId::new();
        let cs = FeatureChangeset::default().parent_id(Some(id));
        assert!(cs.parent_id.is_some());
        assert_eq!(cs.changed_fields(), vec!["parent_id"]);
    }

    #[test]
    fn changeset_clear_version() {
        let cs = FeatureChangeset::default().target_version_id(None);
        assert!(cs.target_version_id.is_some()); // Some(None) = clear
        assert!(cs.target_version_id.unwrap().is_none());
        assert_eq!(cs.changed_fields(), vec!["target_version_id"]);
    }

    #[test]
    fn changeset_set_version() {
        let vid = VersionId::new();
        let cs = FeatureChangeset::default().target_version_id(Some(vid));
        assert_eq!(cs.target_version_id.unwrap().unwrap(), vid);
    }

    #[test]
    fn changeset_desired_details() {
        let cs = FeatureChangeset::default().desired_details(Some("proposed change".to_string()));
        assert!(cs.desired_details.is_some());
        assert_eq!(
            cs.desired_details.unwrap().as_deref(),
            Some("proposed change")
        );
    }

    #[test]
    fn changeset_clear_desired_details() {
        let cs = FeatureChangeset::default().desired_details(None);
        assert!(cs.desired_details.is_some()); // Some(None) = clear
        assert!(cs.desired_details.unwrap().is_none());
    }

    #[test]
    fn changeset_all_fields_tracked() {
        let cs = FeatureChangeset {
            title: Some("t".into()),
            details: Some("d".into()),
            desired_details: Some(None),
            details_summary: Some(None),
            state: Some(FeatureState::Proposed),
            priority: Some(0),
            parent_id: Some(None),
            target_version_id: Some(None),
            feature_number: Some(1),
        };
        assert_eq!(cs.changed_fields().len(), 9);
    }

    #[test]
    fn store_capabilities_sqlite_defaults() {
        let caps = StoreCapabilities::sqlite();
        assert!(caps.offline_writes);
        assert!(!caps.realtime_sync);
        assert!(caps.fulltext_search);
        assert!(caps.transactions);
        assert!(!caps.external_ui);
        assert_eq!(caps.backend_type, BackendType::Sqlite);
        assert!(caps.max_detail_length.is_none());
    }

    #[test]
    fn backend_type_display() {
        assert_eq!(BackendType::Sqlite.to_string(), "sqlite");
        assert_eq!(BackendType::Turso.to_string(), "turso");
        assert_eq!(BackendType::GitHub.to_string(), "github");
        assert_eq!(
            BackendType::Custom("redis".to_string()).to_string(),
            "custom:redis"
        );
    }

    #[test]
    fn backend_config_default_is_sqlite() {
        let config = BackendConfig::default();
        assert!(matches!(config, BackendConfig::Sqlite { path: None }));
    }

    #[test]
    fn backend_config_deserialize_sqlite() {
        let json = r#"{"type": "sqlite", "path": "/tmp/test.db"}"#;
        let config: BackendConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, BackendConfig::Sqlite { path: Some(_) }));
    }

    #[test]
    fn backend_config_deserialize_turso() {
        let json = r#"{"type": "turso", "url": "libsql://test.turso.io", "sync_interval_secs": 5}"#;
        let config: BackendConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, BackendConfig::Turso { .. }));
    }

    #[test]
    fn backend_config_deserialize_github() {
        let json = r#"{"type": "github", "repo": "org/repo"}"#;
        let config: BackendConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config, BackendConfig::GitHub { .. }));
    }
}
