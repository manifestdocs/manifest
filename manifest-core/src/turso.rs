//! Turso embedded replica connection management.
//!
//! Provides [`TursoConnection`] for connecting to Turso databases via libSQL
//! embedded replicas. Reads are served from a local SQLite file (microsecond
//! latency), writes route through the cloud primary.
//!
//! # Usage
//!
//! ```ignore
//! use manifest_core::turso::TursoConnection;
//!
//! let conn = TursoConnection::open_replica(
//!     "/path/to/replica.db",
//!     "libsql://mydb.turso.io",
//!     "auth-token",
//! ).await?;
//!
//! conn.sync().await?;
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::sync::RwLock;

/// Schema SQL for shared tables that get provisioned on first connect.
/// This is a subset of Manifest's schema — only the tables that sync to Turso.
const SHARED_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    slug TEXT UNIQUE NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    instructions TEXT,
    current_version_id TEXT,
    root_feature_id TEXT,
    default_feature_destination TEXT NOT NULL DEFAULT 'backlog',
    test_adapter TEXT,
    context_budget INTEGER,
    key_prefix TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS features (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    parent_id TEXT REFERENCES features(id),
    title TEXT NOT NULL,
    details TEXT,
    desired_details TEXT,
    details_summary TEXT,
    state TEXT NOT NULL DEFAULT 'proposed',
    priority INTEGER NOT NULL DEFAULT 0,
    feature_number INTEGER,
    target_version_id TEXT,
    claimed_by TEXT,
    claimed_at TEXT,
    claim_metadata TEXT,
    verification_result TEXT,
    verified_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS versions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    name TEXT NOT NULL,
    description TEXT,
    released_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS feature_history (
    id TEXT PRIMARY KEY,
    feature_id TEXT NOT NULL REFERENCES features(id),
    version_id TEXT,
    details TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS feature_blockers (
    feature_id TEXT NOT NULL REFERENCES features(id),
    blocker_id TEXT NOT NULL REFERENCES features(id),
    PRIMARY KEY (feature_id, blocker_id)
);
"#;

/// Configuration for opening a Turso connection.
#[derive(Debug, Clone)]
pub struct TursoConfig {
    /// Path to the local embedded replica SQLite file.
    pub replica_path: PathBuf,
    /// Remote Turso URL (e.g., `libsql://mydb.turso.io`).
    pub url: String,
    /// Auth token for the Turso database.
    pub auth_token: String,
    /// Background sync interval. Default: 5 seconds.
    pub sync_interval: Duration,
    /// Whether to sync after writes (read-your-own-writes). Default: true.
    pub read_your_writes: bool,
}

impl TursoConfig {
    /// Create a config from remote management data.
    pub fn from_remote(remote_name: &str, url: &str, auth_token: &str) -> Self {
        let replica_path = replica_dir().join(format!("{}.db", remote_name));
        Self {
            replica_path,
            url: url.to_string(),
            auth_token: auth_token.to_string(),
            sync_interval: Duration::from_secs(5),
            read_your_writes: true,
        }
    }
}

/// A connection to a Turso database via embedded replica.
///
/// Wraps a `libsql::Database` and provides sync operations.
/// The underlying embedded replica serves reads from a local SQLite file
/// and routes writes through the Turso cloud primary.
pub struct TursoConnection {
    db: Arc<libsql::Database>,
    config: TursoConfig,
    last_sync: Arc<RwLock<Option<Instant>>>,
}

impl TursoConnection {
    /// Open an embedded replica connection to a Turso database.
    ///
    /// Creates the replica directory if needed, opens the embedded replica,
    /// and performs an initial sync.
    pub async fn open(config: TursoConfig) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = config.replica_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("creating replica directory")?;
        }

        let db = libsql::Builder::new_remote_replica(
            &config.replica_path,
            config.url.clone(),
            config.auth_token.clone(),
        )
        .sync_interval(config.sync_interval)
        .read_your_writes(config.read_your_writes)
        .build()
        .await
        .context("opening Turso embedded replica")?;

        let conn = Self {
            db: Arc::new(db),
            config,
            last_sync: Arc::new(RwLock::new(None)),
        };

        // Initial sync to pull remote state
        conn.sync().await.context("initial sync")?;

        Ok(conn)
    }

    /// Open a local-only libSQL database (for testing without a Turso server).
    pub async fn open_local(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .context("creating local db directory")?;
        }

        let db = libsql::Builder::new_local(path)
            .build()
            .await
            .context("opening local libsql database")?;

        Ok(Self {
            db: Arc::new(db),
            config: TursoConfig {
                replica_path: path.to_path_buf(),
                url: String::new(),
                auth_token: String::new(),
                sync_interval: Duration::from_secs(5),
                read_your_writes: true,
            },
            last_sync: Arc::new(RwLock::new(None)),
        })
    }

    /// Trigger a manual sync with the remote.
    ///
    /// For local-only databases this is a no-op.
    pub async fn sync(&self) -> Result<()> {
        if self.config.url.is_empty() {
            // Local-only mode — no remote to sync with
            return Ok(());
        }

        self.db.sync().await.context("syncing with remote")?;

        let mut last_sync = self.last_sync.write().await;
        *last_sync = Some(Instant::now());
        Ok(())
    }

    /// Get a connection for executing queries.
    pub fn connect(&self) -> Result<libsql::Connection> {
        self.db
            .connect()
            .context("getting connection from Turso database")
    }

    /// Provision the shared schema on the database if tables don't exist.
    ///
    /// This is safe to call multiple times (all statements use IF NOT EXISTS).
    pub async fn provision_schema(&self) -> Result<()> {
        let conn = self.connect()?;

        // Execute each statement separately (libsql doesn't support multi-statement)
        for statement in SHARED_SCHEMA.split(';') {
            let trimmed = statement.trim();
            if trimmed.is_empty() {
                continue;
            }
            conn.execute(trimmed, ()).await.with_context(|| {
                format!(
                    "provisioning schema statement: {}",
                    &trimmed[..trimmed.len().min(60)]
                )
            })?;
        }

        // Sync after provisioning so the schema is written to the remote
        self.sync().await?;

        Ok(())
    }

    /// Ping the remote and return round-trip latency.
    ///
    /// Returns `None` for local-only databases.
    pub async fn ping(&self) -> Result<Option<Duration>> {
        if self.config.url.is_empty() {
            return Ok(None);
        }

        let start = Instant::now();
        self.db.sync().await.context("ping sync")?;
        Ok(Some(start.elapsed()))
    }

    /// Get information about the database.
    pub async fn info(&self) -> Result<TursoInfo> {
        let conn = self.connect()?;

        // Count tables
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                (),
            )
            .await?;
        let table_count = if let Some(row) = rows.next().await? {
            row.get::<i64>(0)?
        } else {
            0
        };

        // Count projects
        let project_count = match conn.query("SELECT COUNT(*) FROM projects", ()).await {
            Ok(mut rows) => {
                if let Some(row) = rows.next().await? {
                    row.get::<i64>(0)?
                } else {
                    0
                }
            }
            Err(_) => 0, // Table may not exist yet
        };

        // Count features
        let feature_count = match conn.query("SELECT COUNT(*) FROM features", ()).await {
            Ok(mut rows) => {
                if let Some(row) = rows.next().await? {
                    row.get::<i64>(0)?
                } else {
                    0
                }
            }
            Err(_) => 0,
        };

        let last_sync = self.last_sync.read().await;

        Ok(TursoInfo {
            replica_path: self.config.replica_path.clone(),
            url: if self.config.url.is_empty() {
                None
            } else {
                Some(self.config.url.clone())
            },
            table_count,
            project_count,
            feature_count,
            last_sync: last_sync.map(|t| t.elapsed()),
            sync_interval: self.config.sync_interval,
        })
    }

    /// Get the config.
    pub fn config(&self) -> &TursoConfig {
        &self.config
    }

    /// Get the time since last sync.
    pub async fn time_since_sync(&self) -> Option<Duration> {
        let last = self.last_sync.read().await;
        last.map(|t| t.elapsed())
    }

    /// Spawn a background sync loop that syncs at the configured interval.
    ///
    /// Returns a `JoinHandle` that runs until dropped.
    pub fn spawn_sync_loop(&self) -> tokio::task::JoinHandle<()> {
        let db = self.db.clone();
        let interval = self.config.sync_interval;
        let last_sync = self.last_sync.clone();
        let url = self.config.url.clone();

        tokio::spawn(async move {
            if url.is_empty() {
                return; // Local-only, no sync needed
            }

            let mut ticker = tokio::time::interval(interval);
            ticker.tick().await; // First tick is immediate, skip it

            loop {
                ticker.tick().await;
                match db.sync().await {
                    Ok(_) => {
                        let mut ls = last_sync.write().await;
                        *ls = Some(Instant::now());
                        tracing::trace!("background sync completed");
                    }
                    Err(e) => {
                        tracing::warn!("background sync failed: {}", e);
                    }
                }
            }
        })
    }
}

/// Information about a Turso database connection.
#[derive(Debug)]
pub struct TursoInfo {
    pub replica_path: PathBuf,
    pub url: Option<String>,
    pub table_count: i64,
    pub project_count: i64,
    pub feature_count: i64,
    pub last_sync: Option<Duration>,
    pub sync_interval: Duration,
}

/// Get the default directory for storing embedded replica files.
pub fn replica_dir() -> PathBuf {
    if let Ok(data_dir) = std::env::var("MANIFEST_DATA_DIR") {
        PathBuf::from(data_dir).join("replicas")
    } else {
        directories::ProjectDirs::from("", "", "manifest")
            .map(|dirs| dirs.data_dir().join("replicas"))
            .unwrap_or_else(|| PathBuf::from(".manifest/replicas"))
    }
}
