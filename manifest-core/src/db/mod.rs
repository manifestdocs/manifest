use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::broadcast;

use crate::models::ProjectId;

mod features;
mod helpers;
mod history;
mod portfolio;

pub use features::CompletionResult;
mod projects;
mod proofs;
mod remotes;
mod templates;
mod versions;

// ============================================================
// Security Utilities
// ============================================================

/// Escape special characters in LIKE patterns to prevent SQL injection.
///
/// SQLite LIKE uses `%` and `_` as wildcards. This function escapes them
/// using `\` as the escape character.
pub fn escape_like_pattern(query: &str) -> String {
    query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

// ============================================================
// Feature Events (for SSE)
// ============================================================

/// Events emitted when features change, used for SSE notifications.
#[derive(Debug, Clone)]
pub enum FeatureEvent {
    Created {
        project_id: ProjectId,
    },
    Updated {
        project_id: ProjectId,
    },
    Deleted {
        project_id: ProjectId,
    },
    Completed {
        project_id: ProjectId,
        feature_id: crate::models::FeatureId,
        feature_title: String,
        project_name: String,
        agent_type: Option<String>,
    },
}

impl FeatureEvent {
    /// Extract the project ID from any event variant.
    #[must_use]
    pub fn project_id(&self) -> ProjectId {
        match self {
            FeatureEvent::Created { project_id } => *project_id,
            FeatureEvent::Updated { project_id } => *project_id,
            FeatureEvent::Deleted { project_id } => *project_id,
            FeatureEvent::Completed { project_id, .. } => *project_id,
        }
    }
}

/// Structured claim conflict details returned when an agent tries to claim
/// a feature that is already claimed by another agent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ClaimConflictInfo {
    /// The agent type that currently owns the claim (e.g. "claude", "gemini").
    pub agent_type: String,
    /// The feature ID that is already claimed.
    pub feature_id: String,
    /// When the existing claim was established (RFC3339).
    pub claimed_at: String,
    /// Optional metadata from the existing claim.
    pub claim_metadata: Option<String>,
}

/// Domain errors that can be meaningfully handled by callers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// The requested entity does not exist.
    #[error("{0}")]
    NotFound(String),
    /// The input failed validation constraints.
    #[error("{0}")]
    Validation(String),
    /// The operation is invalid for the entity's current state.
    #[error("{0}")]
    InvalidState(String),
    /// Another agent already holds a claim on this feature.
    #[error("Feature already claimed by '{}'", .0.agent_type)]
    ClaimConflict(ClaimConflictInfo),
}

impl ManifestError {
    /// Create a not-found error for the given entity name.
    #[must_use]
    pub fn not_found(entity: &str) -> Self {
        ManifestError::NotFound(format!("{} not found", entity))
    }

    /// Create a validation error with a custom message.
    #[must_use]
    pub fn validation(msg: impl Into<String>) -> Self {
        ManifestError::Validation(msg.into())
    }

    /// Create an invalid-state error with a custom message.
    #[must_use]
    pub fn invalid_state(msg: impl Into<String>) -> Self {
        ManifestError::InvalidState(msg.into())
    }
}

const EVENT_CHANNEL_CAPACITY: usize = 16;

/// Migration report for root feature migration.
#[derive(Debug, Clone, Default)]
pub struct RootFeatureMigrationReport {
    /// Number of projects that received a new root feature.
    pub projects_migrated: usize,
    /// Total number of features reparented under new root features.
    pub features_reparented: usize,
    /// Number of projects that already had a root feature.
    pub projects_skipped: usize,
}

/// Core database handle wrapping a libSQL database, connection, and event broadcaster.
///
/// Uses libSQL (a SQLite fork by Turso) as the sole storage engine. When configured
/// with a Turso remote URL, the database operates as an embedded replica with
/// automatic sync. Without a remote, it behaves identically to standard SQLite.
pub struct Database {
    db: Arc<libsql::Database>,
    conn: libsql::Connection,
    events: broadcast::Sender<FeatureEvent>,
}

impl Database {
    /// Open a local SQLite database at the specified path via libSQL.
    pub async fn open(path: PathBuf) -> Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Database path has no parent directory"))?;
        std::fs::create_dir_all(parent)?;

        let db = libsql::Builder::new_local(path)
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open database: {}", e))?;

        let conn = db
            .connect()
            .map_err(|e| anyhow::anyhow!("Failed to get connection: {}", e))?;

        // Set SQLite PRAGMAs
        conn.execute("PRAGMA journal_mode = WAL", ())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to set journal_mode: {}", e))?;
        conn.execute("PRAGMA foreign_keys = ON", ())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to enable foreign keys: {}", e))?;
        conn.execute("PRAGMA busy_timeout = 5000", ())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to set busy_timeout: {}", e))?;

        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        Ok(Self {
            db: Arc::new(db),
            conn,
            events,
        })
    }

    /// Open the default database location.
    pub async fn open_default() -> Result<Self> {
        let db_path = if let Ok(data_dir) = std::env::var("MANIFEST_DATA_DIR") {
            let path = PathBuf::from(data_dir).join("manifest.db");
            tracing::info!("Using database from MANIFEST_DATA_DIR: {}", path.display());
            path
        } else {
            let dirs = directories::ProjectDirs::from("", "", "manifest")
                .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?;
            let path = dirs.data_dir().join("manifest.db");
            tracing::info!("Using default database: {}", path.display());
            path
        };
        Self::open(db_path).await
    }

    /// Open a database with an explicit path override, falling back through
    /// config file → env vars → platform default.
    ///
    /// Precedence:
    /// 1. `db_path_override` (from --db flag or MANIFEST_DB env)
    /// 2. `config.json` database_path
    /// 3. MANIFEST_DATA_DIR env
    /// 4. Platform default
    pub async fn open_with_override(db_path_override: Option<PathBuf>) -> Result<Self> {
        if let Some(path) = db_path_override {
            tracing::info!("Using database from --db flag: {}", path.display());
            return Self::open(path).await;
        }

        // Check config file
        if let Ok(config) = crate::config::ServerConfig::load() {
            if let Some(ref db_path) = config.database_path {
                let path = PathBuf::from(db_path);
                tracing::info!("Using database from config file: {}", path.display());
                return Self::open(path).await;
            }
        }

        // Fall through to default resolution
        Self::open_default().await
    }

    /// Open an in-memory database for testing.
    pub async fn open_memory() -> Result<Self> {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open in-memory database: {}", e))?;

        let conn = db
            .connect()
            .map_err(|e| anyhow::anyhow!("Failed to get connection: {}", e))?;

        // Set SQLite PRAGMAs
        conn.execute("PRAGMA foreign_keys = ON", ()).await
            .map_err(|e| anyhow::anyhow!("Failed to enable foreign keys: {}", e))?;

        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        Ok(Self {
            db: Arc::new(db),
            conn,
            events,
        })
    }

    /// Open an embedded replica connected to a Turso remote.
    ///
    /// Reads are served from the local replica file (microsecond latency).
    /// Writes route through the Turso cloud primary.
    pub async fn open_replica(
        path: PathBuf,
        url: String,
        auth_token: String,
    ) -> Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Database path has no parent directory"))?;
        std::fs::create_dir_all(parent)?;

        let db = libsql::Builder::new_remote_replica(path, url, auth_token)
            .sync_interval(std::time::Duration::from_secs(5))
            .read_your_writes(true)
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open replica: {}", e))?;

        // Initial sync to pull remote state
        db.sync()
            .await
            .map_err(|e| anyhow::anyhow!("Initial sync failed: {}", e))?;

        let conn = db
            .connect()
            .map_err(|e| anyhow::anyhow!("Failed to get connection: {}", e))?;

        // Set SQLite PRAGMAs
        conn.execute("PRAGMA foreign_keys = ON", ()).await
            .map_err(|e| anyhow::anyhow!("Failed to enable foreign keys: {}", e))?;

        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        Ok(Self {
            db: Arc::new(db),
            conn,
            events,
        })
    }

    /// Get the libsql connection for executing queries.
    pub fn conn(&self) -> &libsql::Connection {
        &self.conn
    }

    /// Get the underlying libsql Database (for sync operations).
    pub fn libsql_db(&self) -> &Arc<libsql::Database> {
        &self.db
    }

    /// Subscribe to feature change events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<FeatureEvent> {
        self.events.subscribe()
    }

    /// Run database migrations.
    pub async fn migrate(&self) -> Result<()> {
        // Check if this is an existing database with our custom schema_migrations table
        let migration_count = self.query_scalar_i64(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'"
        ).await.unwrap_or(0);

        if migration_count > 0 {
            tracing::info!(
                "Detected existing database with schema_migrations, running incremental migrations"
            );
            self.run_incremental_migrations().await?;
            return Ok(());
        }

        // Check if core tables exist
        let features_count = self.query_scalar_i64(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='features'"
        ).await.unwrap_or(0);

        if features_count > 0 {
            tracing::info!("Detected existing database schema, running incremental migrations");
            self.run_incremental_migrations().await?;
            return Ok(());
        }

        // Run embedded schema directly
        self.run_schema().await?;
        // FTS5 triggers contain semicolons that break the schema splitter,
        // so they're created separately via the migration function.
        self.migrate_add_features_fts().await?;
        Ok(())
    }

    /// Run incremental migrations on existing databases.
    async fn run_incremental_migrations(&self) -> Result<()> {
        self.migrate_deprecated_to_archived().await?;
        self.migrate_add_project_slug().await?;
        self.migrate_add_default_feature_destination().await?;
        self.migrate_feature_destination_now_to_next().await?;
        self.migrate_add_details_summary().await?;
        self.migrate_add_project_focus().await?;
        self.migrate_add_copilot_agent_type().await?;
        self.migrate_add_feature_numbers().await?;
        self.migrate_add_blocked_state().await?;
        self.migrate_add_verification_columns().await?;
        self.migrate_add_claim_columns().await?;
        self.migrate_add_testing_policy().await?;
        self.migrate_add_proofs_table().await?;
        self.migrate_add_test_adapter().await?;
        self.migrate_add_spec_templates().await?;
        self.migrate_add_features_fts().await?;
        self.migrate_add_context_budget().await?;
        self.migrate_add_remotes().await?;
        self.migrate_add_field_timestamps().await?;
        self.migrate_add_offline_queue().await?;
        Ok(())
    }

    /// Add slug column to projects table if it doesn't exist.
    async fn migrate_add_project_slug(&self) -> Result<()> {
        if self.has_column("projects", "slug").await? {
            tracing::debug!("Project slug migration already applied");
            return Ok(());
        }

        tracing::info!("Adding slug column to projects table");

        self.conn.execute("PRAGMA foreign_keys = OFF", ()).await?;

        self.conn.execute("ALTER TABLE projects ADD COLUMN slug TEXT", ()).await?;

        self.conn.execute(
            "UPDATE projects SET slug = LOWER(REPLACE(REPLACE(REPLACE(REPLACE(name, ' ', '-'), '_', '-'), '.', '-'), '--', '-'))",
            (),
        ).await?;

        self.conn.execute(
            "UPDATE projects SET slug = slug || '-' || rowid
             WHERE rowid NOT IN (
                 SELECT MIN(rowid) FROM projects GROUP BY slug
             )",
            (),
        ).await?;

        let statements = [
            "DROP TABLE IF EXISTS projects_new",
            "CREATE TABLE projects_new (
                id TEXT PRIMARY KEY,
                slug TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                description TEXT,
                instructions TEXT,
                current_version_id TEXT,
                root_feature_id TEXT,
                owner_id TEXT,
                visibility TEXT NOT NULL DEFAULT 'private',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                CONSTRAINT chk_projects_visibility CHECK (visibility IN ('private', 'public'))
            )",
            "INSERT INTO projects_new SELECT id, slug, name, description, instructions, current_version_id, root_feature_id, owner_id, visibility, created_at, updated_at FROM projects",
            "DROP TABLE projects",
            "ALTER TABLE projects_new RENAME TO projects",
            "CREATE INDEX idx_projects_root_feature ON projects(root_feature_id)",
            "CREATE INDEX idx_projects_slug ON projects(slug)",
        ];

        for sql in statements {
            self.conn.execute(sql, ()).await?;
        }

        self.conn.execute("PRAGMA foreign_keys = ON", ()).await?;

        tracing::info!("Project slug migration complete");
        Ok(())
    }

    /// Migrate 'deprecated' feature state to 'archived'.
    async fn migrate_deprecated_to_archived(&self) -> Result<()> {
        let schema = self.query_scalar_optional_string(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='features'"
        ).await?;

        let has_deprecated = schema
            .as_ref()
            .map(|s| s.contains("'deprecated'"))
            .unwrap_or(false);

        if !has_deprecated {
            tracing::debug!("Feature state migration already applied");
            return Ok(());
        }

        tracing::info!("Migrating feature state: deprecated -> archived");

        self.conn.execute("PRAGMA foreign_keys = OFF", ()).await?;

        let statements = [
            "DROP TABLE IF EXISTS features_new",
            "CREATE TABLE features_new (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                parent_id TEXT REFERENCES features_new(id) ON DELETE CASCADE,
                title TEXT NOT NULL,
                details TEXT,
                desired_details TEXT,
                state TEXT NOT NULL DEFAULT 'proposed' CHECK (state IN ('proposed', 'in_progress', 'implemented', 'archived')),
                priority INTEGER NOT NULL DEFAULT 0,
                target_version_id TEXT REFERENCES versions(id) ON DELETE SET NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            "INSERT INTO features_new (id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at)
            SELECT id, project_id, parent_id, title, details, desired_details,
                CASE WHEN state = 'deprecated' THEN 'archived' ELSE state END,
                priority, target_version_id, created_at, updated_at
            FROM features",
            "DROP TABLE features",
            "ALTER TABLE features_new RENAME TO features",
            "CREATE INDEX idx_features_project ON features(project_id)",
            "CREATE INDEX idx_features_parent ON features(parent_id)",
            "CREATE INDEX idx_features_target_version ON features(target_version_id) WHERE target_version_id IS NOT NULL",
        ];

        for sql in statements {
            self.conn.execute(sql, ()).await?;
        }

        self.conn.execute("PRAGMA foreign_keys = ON", ()).await?;

        tracing::info!("Feature state migration complete");
        Ok(())
    }

    /// Add default_feature_destination column to projects table if it doesn't exist.
    async fn migrate_add_default_feature_destination(&self) -> Result<()> {
        if self.has_column("projects", "default_feature_destination").await? {
            return Ok(());
        }
        tracing::info!("Adding default_feature_destination column to projects table");
        self.conn.execute(
            "ALTER TABLE projects ADD COLUMN default_feature_destination TEXT NOT NULL DEFAULT 'backlog'",
            (),
        ).await?;
        Ok(())
    }

    /// Migrate default_feature_destination from "now" to "next".
    async fn migrate_feature_destination_now_to_next(&self) -> Result<()> {
        let count = self.query_scalar_i64(
            "SELECT COUNT(*) FROM projects WHERE default_feature_destination = 'now'"
        ).await?;
        if count == 0 {
            return Ok(());
        }
        tracing::info!("Migrating default_feature_destination from 'now' to 'next'");
        self.conn.execute(
            "UPDATE projects SET default_feature_destination = 'next' WHERE default_feature_destination = 'now'",
            (),
        ).await?;
        Ok(())
    }

    /// Add details_summary column to features table if it doesn't exist.
    async fn migrate_add_details_summary(&self) -> Result<()> {
        if self.has_column("features", "details_summary").await? {
            return Ok(());
        }
        tracing::info!("Adding details_summary column to features table");
        self.conn.execute("ALTER TABLE features ADD COLUMN details_summary TEXT", ()).await?;
        Ok(())
    }

    /// Add key_prefix column to projects and feature_number column to features.
    async fn migrate_add_feature_numbers(&self) -> Result<()> {
        if !self.has_column("projects", "key_prefix").await? {
            tracing::info!("Adding key_prefix column to projects table");
            self.conn.execute(
                "ALTER TABLE projects ADD COLUMN key_prefix TEXT NOT NULL DEFAULT ''", ()
            ).await?;

            // Backfill key_prefix from slug
            use helpers::derive_key_prefix;
            let mut rows = self.conn.query("SELECT id, slug FROM projects", ()).await?;
            let mut updates = Vec::new();
            while let Some(row) = rows.next().await
                .map_err(|e| anyhow::anyhow!("Failed to fetch row: {}", e))?
            {
                let id: String = row.get(0).map_err(|e| anyhow::anyhow!("{}", e))?;
                let slug: String = row.get(1).map_err(|e| anyhow::anyhow!("{}", e))?;
                updates.push((derive_key_prefix(&slug), id));
            }
            for (prefix, id) in &updates {
                self.conn.execute(
                    "UPDATE projects SET key_prefix = ?1 WHERE id = ?2",
                    libsql::params![prefix.as_str(), id.as_str()],
                ).await?;
            }
            tracing::info!("Backfilled key_prefix for {} projects", updates.len());
        }

        if !self.has_column("features", "feature_number").await? {
            tracing::info!("Adding feature_number column to features table");
            self.conn.execute("ALTER TABLE features ADD COLUMN feature_number INTEGER", ()).await?;

            self.conn.execute(
                "UPDATE features SET feature_number = (
                    SELECT rn FROM (
                        SELECT id, ROW_NUMBER() OVER (PARTITION BY project_id ORDER BY created_at) as rn
                        FROM features
                    ) numbered WHERE numbered.id = features.id
                )",
                (),
            ).await?;

            self.conn.execute(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_features_number ON features(project_id, feature_number)",
                (),
            ).await?;

            tracing::info!("Backfilled feature_number and created unique index");
        }

        Ok(())
    }

    /// Add 'blocked' to features state CHECK constraint and create feature_blockers table.
    async fn migrate_add_blocked_state(&self) -> Result<()> {
        if self.table_exists("feature_blockers").await? {
            tracing::debug!("Blocked state migration already applied");
            return Ok(());
        }

        tracing::info!("Adding blocked state and feature_blockers table");

        self.conn.execute("PRAGMA foreign_keys = OFF", ()).await?;

        let statements = [
            "DROP TABLE IF EXISTS features_new",
            "CREATE TABLE features_new (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                parent_id TEXT,
                title TEXT NOT NULL,
                details TEXT,
                desired_details TEXT,
                details_summary TEXT,
                state TEXT NOT NULL DEFAULT 'proposed',
                priority INTEGER NOT NULL DEFAULT 0,
                feature_number INTEGER,
                target_version_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                CONSTRAINT fk_features_project FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
                CONSTRAINT fk_features_parent FOREIGN KEY (parent_id) REFERENCES features_new(id) ON DELETE CASCADE,
                CONSTRAINT fk_features_version FOREIGN KEY (target_version_id) REFERENCES versions(id) ON DELETE SET NULL,
                CONSTRAINT chk_features_state CHECK (state IN ('proposed', 'blocked', 'in_progress', 'implemented', 'archived'))
            )",
            "INSERT INTO features_new SELECT id, project_id, parent_id, title, details, desired_details, details_summary, state, priority, feature_number, target_version_id, created_at, updated_at FROM features",
            "DROP TABLE features",
            "ALTER TABLE features_new RENAME TO features",
            "CREATE INDEX idx_features_project ON features(project_id)",
            "CREATE INDEX idx_features_parent ON features(parent_id)",
            "CREATE UNIQUE INDEX idx_features_number ON features(project_id, feature_number)",
        ];

        for sql in statements {
            self.conn.execute(sql, ()).await?;
        }

        self.conn.execute(
            "CREATE TABLE feature_blockers (
                feature_id TEXT NOT NULL,
                blocker_feature_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (feature_id, blocker_feature_id),
                FOREIGN KEY (feature_id) REFERENCES features(id) ON DELETE CASCADE,
                FOREIGN KEY (blocker_feature_id) REFERENCES features(id) ON DELETE CASCADE
            )",
            (),
        ).await?;

        self.conn.execute("PRAGMA foreign_keys = ON", ()).await?;

        tracing::info!("Blocked state migration complete");
        Ok(())
    }

    /// Execute the initial schema SQL.
    async fn run_schema(&self) -> Result<()> {
        let schema = include_str!("../../migrations/20240101000000_initial.sql");

        for statement in schema.split(';') {
            let sql: String = statement
                .lines()
                .filter(|line| {
                    let trimmed = line.trim();
                    !trimmed.is_empty() && !trimmed.starts_with("--")
                })
                .collect::<Vec<_>>()
                .join("\n");

            let sql = sql.trim();
            if !sql.is_empty() {
                self.conn.execute(sql, ()).await.map_err(|e| {
                    anyhow::anyhow!(
                        "Migration failed: {} - SQL: {}",
                        e,
                        &sql[..sql.len().min(100)]
                    )
                })?;
            }
        }

        Ok(())
    }

    /// Add project_focus table if it doesn't exist.
    async fn migrate_add_project_focus(&self) -> Result<()> {
        if self.table_exists("project_focus").await? {
            return Ok(());
        }
        tracing::info!("Creating project_focus table");
        self.conn.execute(
            "CREATE TABLE project_focus (
                project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
                feature_id TEXT NOT NULL REFERENCES features(id) ON DELETE CASCADE,
                updated_at TEXT NOT NULL
            )",
            (),
        ).await?;
        Ok(())
    }

    /// Add 'copilot' to tasks.agent_type CHECK constraint.
    async fn migrate_add_copilot_agent_type(&self) -> Result<()> {
        if !self.table_exists("tasks").await? {
            tracing::debug!("copilot migration: no tasks table, skipping");
            return Ok(());
        }

        let sql = include_str!("../db/migrations/019_add_copilot_agent_type.sql");
        for statement in sql.split(';') {
            let trimmed: String = statement
                .lines()
                .filter(|line| {
                    let t = line.trim();
                    !t.is_empty() && !t.starts_with("--")
                })
                .collect::<Vec<_>>()
                .join("\n");
            let trimmed = trimmed.trim();
            if !trimmed.is_empty() {
                if let Err(e) = self.conn.execute(trimmed, ()).await {
                    tracing::debug!("copilot migration statement skipped: {}", e);
                    return Ok(());
                }
            }
        }

        tracing::info!("Added 'copilot' to tasks.agent_type CHECK constraint");
        Ok(())
    }

    /// Add verification_result and verified_at columns to features table.
    async fn migrate_add_verification_columns(&self) -> Result<()> {
        if self.has_column("features", "verification_result").await? {
            return Ok(());
        }
        tracing::info!("Adding verification_result and verified_at columns to features table");
        self.conn.execute("ALTER TABLE features ADD COLUMN verification_result TEXT", ()).await?;
        self.conn.execute("ALTER TABLE features ADD COLUMN verified_at TEXT", ()).await?;
        Ok(())
    }

    /// Add claimed_by, claimed_at, and claim_metadata columns to features table.
    async fn migrate_add_claim_columns(&self) -> Result<()> {
        if self.has_column("features", "claimed_by").await? {
            return Ok(());
        }
        tracing::info!("Adding claimed_by, claimed_at, claim_metadata columns to features table");
        self.conn.execute("ALTER TABLE features ADD COLUMN claimed_by TEXT", ()).await?;
        self.conn.execute("ALTER TABLE features ADD COLUMN claimed_at TEXT", ()).await?;
        self.conn.execute("ALTER TABLE features ADD COLUMN claim_metadata TEXT", ()).await?;
        Ok(())
    }

    /// Add proofs table for test evidence.
    async fn migrate_add_proofs_table(&self) -> Result<()> {
        if self.table_exists("proofs").await? {
            return Ok(());
        }
        tracing::info!("Creating proofs table");
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS proofs (
                id TEXT PRIMARY KEY,
                feature_id TEXT NOT NULL,
                history_id TEXT,
                command TEXT NOT NULL,
                exit_code INTEGER NOT NULL,
                output TEXT,
                tests TEXT,
                evidence TEXT,
                commit_sha TEXT,
                agent_type TEXT,
                created_at TEXT NOT NULL,
                CONSTRAINT fk_proofs_feature FOREIGN KEY (feature_id) REFERENCES features(id) ON DELETE CASCADE,
                CONSTRAINT fk_proofs_history FOREIGN KEY (history_id) REFERENCES feature_history(id) ON DELETE SET NULL
            )",
            (),
        ).await?;
        self.conn.execute("CREATE INDEX IF NOT EXISTS idx_proofs_feature ON proofs(feature_id)", ()).await?;
        self.conn.execute("CREATE INDEX IF NOT EXISTS idx_proofs_history ON proofs(history_id)", ()).await?;
        Ok(())
    }

    /// Add testing_policy column to projects table.
    async fn migrate_add_testing_policy(&self) -> Result<()> {
        if self.has_column("projects", "testing_policy").await? {
            return Ok(());
        }
        tracing::info!("Adding testing_policy column to projects table");
        self.conn.execute("ALTER TABLE projects ADD COLUMN testing_policy TEXT NOT NULL DEFAULT 'tdd'", ()).await?;
        Ok(())
    }

    /// Add test_adapter column to projects table.
    async fn migrate_add_test_adapter(&self) -> Result<()> {
        if self.has_column("projects", "test_adapter").await? {
            return Ok(());
        }
        tracing::info!("Adding test_adapter column to projects table");
        self.conn.execute("ALTER TABLE projects ADD COLUMN test_adapter TEXT", ()).await?;
        Ok(())
    }

    /// Add spec_templates table if it doesn't exist.
    async fn migrate_add_spec_templates(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS spec_templates (
                id TEXT PRIMARY KEY,
                project_id TEXT NOT NULL,
                name TEXT NOT NULL,
                description TEXT,
                content TEXT NOT NULL,
                is_default INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                CONSTRAINT fk_spec_templates_project FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
                UNIQUE(project_id, name)
            )",
            (),
        ).await?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_spec_templates_project ON spec_templates(project_id)",
            (),
        ).await?;

        // Insert default template for any project that doesn't have one yet
        let now = chrono::Utc::now().to_rfc3339();
        let mut rows = self.conn.query(
            "SELECT p.id FROM projects p
             WHERE NOT EXISTS (SELECT 1 FROM spec_templates st WHERE st.project_id = p.id)",
            (),
        ).await?;

        let mut project_ids = Vec::new();
        while let Some(row) = rows.next().await
            .map_err(|e| anyhow::anyhow!("Failed to fetch row: {}", e))?
        {
            let id: String = row.get(0).map_err(|e| anyhow::anyhow!("{}", e))?;
            project_ids.push(id);
        }

        if project_ids.is_empty() {
            return Ok(());
        }

        for project_id in &project_ids {
            let template_id = uuid::Uuid::new_v4().to_string();
            self.conn.execute(
                "INSERT INTO spec_templates (id, project_id, name, description, content, is_default, created_at, updated_at)
                 VALUES (?1, ?2, 'Default', 'General-purpose feature specification template', ?3, 1, ?4, ?5)",
                libsql::params![
                    template_id.as_str(),
                    project_id.as_str(),
                    crate::models::DEFAULT_TEMPLATE_CONTENT,
                    now.as_str(),
                    now.as_str()
                ],
            ).await?;
        }

        tracing::info!("Created default spec templates for {} projects", project_ids.len());
        Ok(())
    }

    /// Add FTS5 full-text search index on features table.
    async fn migrate_add_features_fts(&self) -> Result<()> {
        let has_fts = self.query_scalar_i64(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='features_fts'"
        ).await.unwrap_or(0);

        if has_fts > 0 {
            return Ok(());
        }

        self.conn.execute(
            "CREATE VIRTUAL TABLE features_fts USING fts5(
                title, details,
                content=features,
                content_rowid=rowid
            )",
            (),
        ).await?;

        self.conn.execute(
            "CREATE TRIGGER IF NOT EXISTS features_fts_insert AFTER INSERT ON features BEGIN
                INSERT INTO features_fts(rowid, title, details) VALUES (new.rowid, new.title, COALESCE(new.details, ''));
            END",
            (),
        ).await?;

        self.conn.execute(
            "CREATE TRIGGER IF NOT EXISTS features_fts_update AFTER UPDATE OF title, details ON features BEGIN
                INSERT INTO features_fts(features_fts, rowid, title, details) VALUES ('delete', old.rowid, old.title, COALESCE(old.details, ''));
                INSERT INTO features_fts(rowid, title, details) VALUES (new.rowid, new.title, COALESCE(new.details, ''));
            END",
            (),
        ).await?;

        self.conn.execute(
            "CREATE TRIGGER IF NOT EXISTS features_fts_delete AFTER DELETE ON features BEGIN
                INSERT INTO features_fts(features_fts, rowid, title, details) VALUES ('delete', old.rowid, old.title, COALESCE(old.details, ''));
            END",
            (),
        ).await?;

        self.conn.execute(
            "INSERT INTO features_fts(rowid, title, details)
             SELECT rowid, title, COALESCE(details, '') FROM features",
            (),
        ).await?;

        tracing::info!("Created features_fts full-text search index");
        Ok(())
    }

    /// Add context_budget column to projects table.
    async fn migrate_add_context_budget(&self) -> Result<()> {
        if self.has_column("projects", "context_budget").await? {
            return Ok(());
        }
        self.conn.execute("ALTER TABLE projects ADD COLUMN context_budget INTEGER", ()).await?;
        tracing::info!("Added context_budget column to projects");
        Ok(())
    }

    /// Add remotes and project_remotes tables for Turso sync.
    async fn migrate_add_remotes(&self) -> Result<()> {
        if self.table_exists("remotes").await? {
            return Ok(());
        }

        self.conn.execute(
            "CREATE TABLE remotes (
                id TEXT PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                provider TEXT NOT NULL DEFAULT 'turso',
                url TEXT NOT NULL,
                auth_token TEXT NOT NULL,
                sync_mode TEXT NOT NULL DEFAULT 'full',
                sync_enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            (),
        ).await?;

        self.conn.execute(
            "CREATE TABLE project_remotes (
                project_id TEXT NOT NULL REFERENCES projects(id),
                remote_id TEXT NOT NULL REFERENCES remotes(id) ON DELETE CASCADE,
                sync_state TEXT NOT NULL DEFAULT 'active',
                last_synced_at TEXT,
                PRIMARY KEY (project_id, remote_id)
            )",
            (),
        ).await?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_project_remotes_remote ON project_remotes(remote_id)",
            (),
        ).await?;

        tracing::info!("Added remotes and project_remotes tables");
        Ok(())
    }

    /// Add field-level timestamp columns to features table for conflict resolution.
    async fn migrate_add_field_timestamps(&self) -> Result<()> {
        if self.has_column("features", "state_updated_at").await? {
            return Ok(());
        }

        for col in ["state_updated_at", "details_updated_at", "parent_id_updated_at"] {
            self.conn.execute(&format!("ALTER TABLE features ADD COLUMN {col} TEXT"), ()).await?;
        }

        tracing::info!("Added field-level timestamp columns to features table");
        Ok(())
    }

    /// Add offline_queue table for queuing writes when remotes are unreachable.
    async fn migrate_add_offline_queue(&self) -> Result<()> {
        if self.table_exists("offline_queue").await? {
            return Ok(());
        }

        self.conn.execute(
            "CREATE TABLE offline_queue (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT NOT NULL,
                remote_id TEXT NOT NULL,
                table_name TEXT NOT NULL,
                row_id TEXT NOT NULL,
                operation TEXT NOT NULL DEFAULT 'upsert',
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            (),
        ).await?;

        self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_offline_queue_remote ON offline_queue(remote_id)",
            (),
        ).await?;

        tracing::info!("Added offline_queue table");
        Ok(())
    }

    // ============================================================
    // Query helpers
    // ============================================================

    /// Check if a table exists in the database.
    async fn table_exists(&self, table_name: &str) -> Result<bool> {
        let sql = format!(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='{}'",
            table_name
        );
        let count = self.query_scalar_i64(&sql).await.unwrap_or(0);
        Ok(count > 0)
    }

    /// Check if a column exists on a table.
    async fn has_column(&self, table_name: &str, column_name: &str) -> Result<bool> {
        let schema = self.query_scalar_optional_string(
            &format!("SELECT sql FROM sqlite_master WHERE type='table' AND name='{}'", table_name)
        ).await?;
        Ok(schema
            .as_ref()
            .map(|s| s.to_lowercase().contains(column_name))
            .unwrap_or(false))
    }

    /// Execute a query that returns a single i64 scalar.
    pub(crate) async fn query_scalar_i64(&self, sql: &str) -> Result<i64> {
        let mut rows = self.conn.query(sql, ()).await
            .map_err(|e| anyhow::anyhow!("Query failed: {}", e))?;
        if let Some(row) = rows.next().await
            .map_err(|e| anyhow::anyhow!("Failed to fetch row: {}", e))?
        {
            row.get::<i64>(0).map_err(|e| anyhow::anyhow!("Failed to get value: {}", e))
        } else {
            Ok(0)
        }
    }

    /// Execute a query that returns an optional string scalar.
    pub(crate) async fn query_scalar_optional_string(&self, sql: &str) -> Result<Option<String>> {
        let mut rows = self.conn.query(sql, ()).await
            .map_err(|e| anyhow::anyhow!("Query failed: {}", e))?;
        if let Some(row) = rows.next().await
            .map_err(|e| anyhow::anyhow!("Failed to fetch row: {}", e))?
        {
            let val: Option<String> = row.get(0).map_err(|e| anyhow::anyhow!("Failed to get value: {}", e))?;
            Ok(val)
        } else {
            Ok(None)
        }
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        // libsql::Database is behind Arc, Connection can be re-obtained
        let conn = self.db.connect().expect("Failed to get connection on clone");
        Self {
            db: self.db.clone(),
            conn,
            events: self.events.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_like_pattern_leaves_plain_text() {
        assert_eq!(escape_like_pattern("hello"), "hello");
    }

    #[test]
    fn escape_like_pattern_escapes_wildcards() {
        assert_eq!(escape_like_pattern("hello%world"), "hello\\%world");
        assert_eq!(escape_like_pattern("hello_world"), "hello\\_world");
        assert_eq!(escape_like_pattern("50% off"), "50\\% off");
    }

    #[test]
    fn escape_like_pattern_escapes_backslash() {
        assert_eq!(escape_like_pattern("a\\b"), "a\\\\b");
    }

    #[tokio::test]
    async fn migrate_is_idempotent() {
        let db = Database::open_memory().await.expect("open in-memory db");

        db.migrate().await.expect("first migration");
        db.migrate().await.expect("second migration (idempotent)");

        let count = db.query_scalar_i64("SELECT COUNT(*) FROM features")
            .await
            .expect("query features table");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn migrate_then_incremental_is_idempotent() {
        let db = Database::open_memory().await.expect("open in-memory db");

        db.migrate().await.expect("initial migration");
        db.run_incremental_migrations()
            .await
            .expect("incremental migrations on fresh schema");
    }
}
