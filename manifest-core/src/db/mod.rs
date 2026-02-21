use std::path::PathBuf;

use anyhow::Result;
use sqlx::any::AnyPoolOptions;
use sqlx::{AnyPool, Executor};
use tokio::sync::broadcast;

use crate::models::ProjectId;

mod features;
mod helpers;
mod history;
mod portfolio;
mod projects;
mod users;
mod versions;

// ============================================================
// Database Dialect
// ============================================================

/// Database dialect for SQL syntax differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbDialect {
    Sqlite,
    Postgres,
}

impl DbDialect {
    /// Detect dialect from a database URL.
    pub fn from_url(url: &str) -> Self {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            DbDialect::Postgres
        } else {
            DbDialect::Sqlite
        }
    }

    /// Returns SQL to check if a table exists.
    ///
    /// # Panics
    /// Panics if `table_name` contains non-alphanumeric/underscore characters,
    /// since the name is interpolated directly into SQL. All callers must pass
    /// hardcoded string literals.
    #[must_use]
    pub fn table_exists_sql(&self, table_name: &str) -> String {
        assert!(
            !table_name.is_empty()
                && table_name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "table_exists_sql: table name must be [a-zA-Z0-9_]+, got '{}'",
            table_name
        );
        match self {
            DbDialect::Sqlite => format!(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='{}'",
                table_name
            ),
            DbDialect::Postgres => format!(
                "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema='public' AND table_name='{}'",
                table_name
            ),
        }
    }

    /// Returns true if this dialect is SQLite.
    #[must_use]
    pub fn is_sqlite(&self) -> bool {
        matches!(self, DbDialect::Sqlite)
    }

    /// Returns true if this dialect is PostgreSQL.
    #[must_use]
    pub fn is_postgres(&self) -> bool {
        matches!(self, DbDialect::Postgres)
    }

    /// SQL fragment for unlimited results with an offset.
    /// SQLite uses `LIMIT -1 OFFSET n`, PostgreSQL uses `LIMIT ALL OFFSET n`.
    #[must_use]
    pub fn unlimited_offset_sql(&self) -> &'static str {
        match self {
            DbDialect::Sqlite => "LIMIT -1",
            DbDialect::Postgres => "LIMIT ALL",
        }
    }
}

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
    Created { project_id: ProjectId },
    Updated { project_id: ProjectId },
    Deleted { project_id: ProjectId },
}

impl FeatureEvent {
    /// Extract the project ID from any event variant.
    #[must_use]
    pub fn project_id(&self) -> ProjectId {
        match self {
            FeatureEvent::Created { project_id } => *project_id,
            FeatureEvent::Updated { project_id } => *project_id,
            FeatureEvent::Deleted { project_id } => *project_id,
        }
    }
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

/// Core database handle wrapping a connection pool, dialect info, and event broadcaster.
pub struct Database {
    pool: AnyPool,
    dialect: DbDialect,
    events: broadcast::Sender<FeatureEvent>,
}

impl Database {
    /// Connect to a database using a URL.
    /// URL format: `sqlite:path/to/db.db` or `postgres://user:pass@host/db`
    pub async fn connect(url: &str) -> Result<Self> {
        // Install the SQLx any driver for the URL scheme
        sqlx::any::install_default_drivers();

        let dialect = DbDialect::from_url(url);

        let mut pool_opts = AnyPoolOptions::new();

        // For SQLite, set PRAGMAs on every new connection in the pool
        if dialect == DbDialect::Sqlite {
            pool_opts = pool_opts.after_connect(|conn, _meta| {
                Box::pin(async move {
                    conn.execute("PRAGMA journal_mode = WAL").await?;
                    conn.execute("PRAGMA foreign_keys = ON").await?;
                    conn.execute("PRAGMA busy_timeout = 5000").await?;
                    Ok(())
                })
            });
        }

        let pool = pool_opts.connect(url).await?;

        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        Ok(Self {
            pool,
            dialect,
            events,
        })
    }

    /// Returns the database dialect (SQLite or PostgreSQL).
    #[must_use]
    pub fn dialect(&self) -> DbDialect {
        self.dialect
    }

    /// Open a SQLite database at the specified path.
    pub async fn open(path: PathBuf) -> Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Database path has no parent directory"))?;
        std::fs::create_dir_all(parent)?;

        let url = format!("sqlite:{}?mode=rwc", path.display());
        Self::connect(&url).await
    }

    /// Open the default SQLite database location.
    pub async fn open_default() -> Result<Self> {
        let db_path = if let Ok(data_dir) = std::env::var("MANIFEST_DATA_DIR") {
            let path = PathBuf::from(data_dir).join("manifest.db");
            tracing::info!("Using database from MANIFEST_DATA_DIR: {}", path.display());
            path
        } else if let Ok(url) = std::env::var("DATABASE_URL") {
            tracing::info!("Using database from DATABASE_URL");
            return Self::connect(&url).await;
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
    /// 4. DATABASE_URL env
    /// 5. Platform default
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

    /// Open an in-memory SQLite database for testing.
    /// Uses shared cache mode so all connections in the pool see the same database.
    pub async fn open_memory() -> Result<Self> {
        // Use a unique named in-memory database with shared cache
        // This ensures all connections in the pool share the same database
        let unique_id = uuid::Uuid::new_v4();
        let url = format!("sqlite:file:memdb_{}?mode=memory&cache=shared", unique_id);
        Self::connect(&url).await
    }

    /// Subscribe to feature change events.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<FeatureEvent> {
        self.events.subscribe()
    }

    /// Run database migrations.
    pub async fn migrate(&self) -> Result<()> {
        // Check if this is an existing database with our custom schema_migrations table
        let migration_sql = self.dialect.table_exists_sql("schema_migrations");
        let migration_count: i64 = sqlx::query_scalar(&migration_sql)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

        if migration_count > 0 {
            // Existing database with old migration system - run incremental migrations
            tracing::info!(
                "Detected existing database with schema_migrations, running incremental migrations"
            );
            self.run_incremental_migrations().await?;
            return Ok(());
        }

        // Check if core tables exist
        let features_sql = self.dialect.table_exists_sql("features");
        let features_count: i64 = sqlx::query_scalar(&features_sql)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

        if features_count > 0 {
            tracing::info!("Detected existing database schema, running incremental migrations");
            self.run_incremental_migrations().await?;
            return Ok(());
        }

        // Run embedded schema directly (sqlx::migrate! has issues with the any driver)
        self.run_schema().await?;
        Ok(())
    }

    /// Run incremental migrations on existing databases.
    async fn run_incremental_migrations(&self) -> Result<()> {
        // Migration: rename 'deprecated' state to 'archived'
        self.migrate_deprecated_to_archived().await?;
        // Migration: add slug column to projects
        self.migrate_add_project_slug().await?;
        // Migration: add default_feature_destination column to projects
        self.migrate_add_default_feature_destination().await?;
        // Migration: rename default_feature_destination value "now" → "next"
        self.migrate_feature_destination_now_to_next().await?;
        // Migration: add details_summary column to features
        self.migrate_add_details_summary().await?;
        // Migration: add guidance level columns to projects
        self.migrate_add_guidance_levels().await?;
        // Migration: rename spec_level → ac_level, add ac_format
        self.migrate_rename_spec_to_ac().await?;
        // Migration: add project_focus table
        self.migrate_add_project_focus().await?;
        // Migration: add 'copilot' to tasks.agent_type CHECK constraint
        self.migrate_add_copilot_agent_type().await?;
        // Migration: add key_prefix to projects, feature_number to features
        self.migrate_add_feature_numbers().await?;
        // Migration: add 'blocked' state and feature_blockers table
        self.migrate_add_blocked_state().await?;
        Ok(())
    }

    /// Add slug column to projects table if it doesn't exist.
    async fn migrate_add_project_slug(&self) -> Result<()> {
        // Check if slug column already exists
        let has_slug = if self.dialect.is_sqlite() {
            let schema: Option<String> = sqlx::query_scalar(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='projects'",
            )
            .fetch_optional(&self.pool)
            .await?;

            schema
                .as_ref()
                .map(|s| s.to_lowercase().contains("slug"))
                .unwrap_or(false)
        } else {
            // PostgreSQL: check information_schema
            let col_exists: Option<String> = sqlx::query_scalar(
                "SELECT column_name FROM information_schema.columns
                 WHERE table_name = 'projects' AND column_name = 'slug'",
            )
            .fetch_optional(&self.pool)
            .await?;

            col_exists.is_some()
        };

        if has_slug {
            tracing::debug!("Project slug migration already applied");
            return Ok(());
        }

        tracing::info!("Adding slug column to projects table");

        if self.dialect.is_sqlite() {
            // SQLite: Add column, populate, then recreate table with constraints
            let mut conn = self.pool.acquire().await?;

            // Disable foreign keys
            sqlx::query("PRAGMA foreign_keys = OFF")
                .execute(&mut *conn)
                .await?;

            // Add slug column (nullable initially)
            sqlx::query("ALTER TABLE projects ADD COLUMN slug TEXT")
                .execute(&mut *conn)
                .await?;

            // Generate slugs from names
            sqlx::query(
                "UPDATE projects SET slug = LOWER(REPLACE(REPLACE(REPLACE(REPLACE(name, ' ', '-'), '_', '-'), '.', '-'), '--', '-'))"
            )
            .execute(&mut *conn)
            .await?;

            // Handle duplicates by appending rowid
            sqlx::query(
                "UPDATE projects SET slug = slug || '-' || rowid
                 WHERE rowid NOT IN (
                     SELECT MIN(rowid) FROM projects GROUP BY slug
                 )",
            )
            .execute(&mut *conn)
            .await?;

            // Recreate table with NOT NULL and UNIQUE constraint
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
                sqlx::query(sql).execute(&mut *conn).await?;
            }

            // Re-enable foreign keys
            sqlx::query("PRAGMA foreign_keys = ON")
                .execute(&mut *conn)
                .await?;
        } else {
            // PostgreSQL: simpler migration
            sqlx::query("ALTER TABLE projects ADD COLUMN slug TEXT")
                .execute(&self.pool)
                .await?;

            sqlx::query(
                "UPDATE projects SET slug = LOWER(REGEXP_REPLACE(REGEXP_REPLACE(name, '[^a-zA-Z0-9]', '-', 'g'), '-+', '-', 'g'))"
            )
            .execute(&self.pool)
            .await?;

            // Handle duplicates
            sqlx::query(
                "UPDATE projects p1 SET slug = slug || '-' || (
                    SELECT COUNT(*) FROM projects p2
                    WHERE p2.slug = p1.slug AND p2.created_at < p1.created_at
                ) WHERE slug IN (SELECT slug FROM projects GROUP BY slug HAVING COUNT(*) > 1)",
            )
            .execute(&self.pool)
            .await?;

            sqlx::query("ALTER TABLE projects ALTER COLUMN slug SET NOT NULL")
                .execute(&self.pool)
                .await?;

            sqlx::query("ALTER TABLE projects ADD CONSTRAINT projects_slug_unique UNIQUE (slug)")
                .execute(&self.pool)
                .await?;

            sqlx::query("CREATE INDEX idx_projects_slug ON projects(slug)")
                .execute(&self.pool)
                .await?;
        }

        tracing::info!("Project slug migration complete");
        Ok(())
    }

    /// Migrate 'deprecated' feature state to 'archived'.
    async fn migrate_deprecated_to_archived(&self) -> Result<()> {
        // Check if migration is needed by looking for 'deprecated' in the constraint
        // We do this by checking if we can insert 'archived' - if not, migration needed
        let test_result = if self.dialect.is_sqlite() {
            // For SQLite, check the table schema
            let schema: Option<String> = sqlx::query_scalar(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='features'",
            )
            .fetch_optional(&self.pool)
            .await?;

            schema
                .as_ref()
                .map(|s| s.contains("'deprecated'"))
                .unwrap_or(false)
        } else {
            // For PostgreSQL, check constraint definition
            let constraint_def: Option<String> = sqlx::query_scalar(
                "SELECT pg_get_constraintdef(oid) FROM pg_constraint
                 WHERE conname = 'chk_features_state'",
            )
            .fetch_optional(&self.pool)
            .await?;

            constraint_def
                .as_ref()
                .map(|s| s.contains("deprecated"))
                .unwrap_or(false)
        };

        if !test_result {
            tracing::debug!("Feature state migration already applied");
            return Ok(());
        }

        tracing::info!("Migrating feature state: deprecated -> archived");

        if self.dialect.is_sqlite() {
            // SQLite: recreate table with new constraint
            // Must use a single connection for PRAGMA foreign_keys to work
            let mut conn = self.pool.acquire().await?;

            // Disable foreign keys on this connection
            sqlx::query("PRAGMA foreign_keys = OFF")
                .execute(&mut *conn)
                .await?;

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
                sqlx::query(sql).execute(&mut *conn).await?;
            }

            // Re-enable foreign keys
            sqlx::query("PRAGMA foreign_keys = ON")
                .execute(&mut *conn)
                .await?;
        } else {
            // PostgreSQL: update data and alter constraint
            sqlx::query("UPDATE features SET state = 'archived' WHERE state = 'deprecated'")
                .execute(&self.pool)
                .await?;

            sqlx::query("ALTER TABLE features DROP CONSTRAINT IF EXISTS chk_features_state")
                .execute(&self.pool)
                .await?;

            sqlx::query(
                "ALTER TABLE features ADD CONSTRAINT chk_features_state
                 CHECK (state IN ('proposed', 'in_progress', 'implemented', 'archived'))",
            )
            .execute(&self.pool)
            .await?;
        }

        tracing::info!("Feature state migration complete");
        Ok(())
    }

    /// Add default_feature_destination column to projects table if it doesn't exist.
    async fn migrate_add_default_feature_destination(&self) -> Result<()> {
        let has_column = if self.dialect.is_sqlite() {
            let schema: Option<String> = sqlx::query_scalar(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='projects'",
            )
            .fetch_optional(&self.pool)
            .await?;

            schema
                .as_ref()
                .map(|s| s.to_lowercase().contains("default_feature_destination"))
                .unwrap_or(false)
        } else {
            let col_exists: Option<String> = sqlx::query_scalar(
                "SELECT column_name FROM information_schema.columns
                 WHERE table_name = 'projects' AND column_name = 'default_feature_destination'",
            )
            .fetch_optional(&self.pool)
            .await?;

            col_exists.is_some()
        };

        if has_column {
            tracing::debug!("default_feature_destination migration already applied");
            return Ok(());
        }

        tracing::info!("Adding default_feature_destination column to projects table");
        sqlx::query(
            "ALTER TABLE projects ADD COLUMN default_feature_destination TEXT NOT NULL DEFAULT 'backlog'",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Migrate default_feature_destination from "now" to "next".
    async fn migrate_feature_destination_now_to_next(&self) -> Result<()> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM projects WHERE default_feature_destination = 'now'",
        )
        .fetch_one(&self.pool)
        .await?;

        if count == 0 {
            return Ok(());
        }

        tracing::info!("Migrating default_feature_destination from 'now' to 'next'");
        sqlx::query(
            "UPDATE projects SET default_feature_destination = 'next' WHERE default_feature_destination = 'now'",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Add details_summary column to features table if it doesn't exist.
    async fn migrate_add_details_summary(&self) -> Result<()> {
        let has_column = if self.dialect.is_sqlite() {
            let schema: Option<String> = sqlx::query_scalar(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='features'",
            )
            .fetch_optional(&self.pool)
            .await?;

            schema
                .as_ref()
                .map(|s| s.to_lowercase().contains("details_summary"))
                .unwrap_or(false)
        } else {
            let col_exists: Option<String> = sqlx::query_scalar(
                "SELECT column_name FROM information_schema.columns
                 WHERE table_name = 'features' AND column_name = 'details_summary'",
            )
            .fetch_optional(&self.pool)
            .await?;

            col_exists.is_some()
        };

        if has_column {
            tracing::debug!("details_summary migration already applied");
            return Ok(());
        }

        tracing::info!("Adding details_summary column to features table");
        sqlx::query("ALTER TABLE features ADD COLUMN details_summary TEXT")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Add detail_level and spec_level columns to projects table if they don't exist.
    async fn migrate_add_guidance_levels(&self) -> Result<()> {
        let has_column = if self.dialect.is_sqlite() {
            let schema: Option<String> = sqlx::query_scalar(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='projects'",
            )
            .fetch_optional(&self.pool)
            .await?;

            schema
                .as_ref()
                .map(|s| s.to_lowercase().contains("detail_level"))
                .unwrap_or(false)
        } else {
            let col_exists: Option<String> = sqlx::query_scalar(
                "SELECT column_name FROM information_schema.columns
                 WHERE table_name = 'projects' AND column_name = 'detail_level'",
            )
            .fetch_optional(&self.pool)
            .await?;

            col_exists.is_some()
        };

        if has_column {
            tracing::debug!("Guidance levels migration already applied");
            return Ok(());
        }

        tracing::info!("Adding detail_level and spec_level columns to projects table");
        sqlx::query(
            "ALTER TABLE projects ADD COLUMN detail_level TEXT NOT NULL DEFAULT 'standard'",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("ALTER TABLE projects ADD COLUMN spec_level TEXT NOT NULL DEFAULT 'standard'")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    /// Rename spec_level → ac_level and add ac_format column.
    /// Leaves spec_level as a dead column (Rust code stops reading it).
    async fn migrate_rename_spec_to_ac(&self) -> Result<()> {
        let has_ac_level = if self.dialect.is_sqlite() {
            let schema: Option<String> = sqlx::query_scalar(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='projects'",
            )
            .fetch_optional(&self.pool)
            .await?;

            schema
                .as_ref()
                .map(|s| s.to_lowercase().contains("ac_level"))
                .unwrap_or(false)
        } else {
            let col_exists: Option<String> = sqlx::query_scalar(
                "SELECT column_name FROM information_schema.columns
                 WHERE table_name = 'projects' AND column_name = 'ac_level'",
            )
            .fetch_optional(&self.pool)
            .await?;

            col_exists.is_some()
        };

        if has_ac_level {
            tracing::debug!("spec_level → ac_level migration already applied");
            return Ok(());
        }

        tracing::info!("Renaming spec_level → ac_level and adding ac_format column");

        // Add ac_level column
        sqlx::query("ALTER TABLE projects ADD COLUMN ac_level TEXT NOT NULL DEFAULT 'standard'")
            .execute(&self.pool)
            .await?;

        // Copy spec_level values into ac_level
        sqlx::query("UPDATE projects SET ac_level = spec_level")
            .execute(&self.pool)
            .await?;

        // Add ac_format column
        sqlx::query("ALTER TABLE projects ADD COLUMN ac_format TEXT NOT NULL DEFAULT 'checkbox'")
            .execute(&self.pool)
            .await?;

        tracing::info!("spec_level → ac_level migration complete");
        Ok(())
    }

    /// Add key_prefix column to projects and feature_number column to features.
    /// Backfills existing data: key_prefix from slug, feature_number in created_at order.
    async fn migrate_add_feature_numbers(&self) -> Result<()> {
        // Check if key_prefix column already exists on projects
        let has_key_prefix = if self.dialect.is_sqlite() {
            let schema: Option<String> = sqlx::query_scalar(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='projects'",
            )
            .fetch_optional(&self.pool)
            .await?;

            schema
                .as_ref()
                .map(|s| s.to_lowercase().contains("key_prefix"))
                .unwrap_or(false)
        } else {
            let col_exists: Option<String> = sqlx::query_scalar(
                "SELECT column_name FROM information_schema.columns
                 WHERE table_name = 'projects' AND column_name = 'key_prefix'",
            )
            .fetch_optional(&self.pool)
            .await?;

            col_exists.is_some()
        };

        if !has_key_prefix {
            tracing::info!("Adding key_prefix column to projects table");
            sqlx::query("ALTER TABLE projects ADD COLUMN key_prefix TEXT NOT NULL DEFAULT ''")
                .execute(&self.pool)
                .await?;

            // Backfill key_prefix from slug
            use helpers::derive_key_prefix;
            let rows = sqlx::query("SELECT id, slug FROM projects")
                .fetch_all(&self.pool)
                .await?;
            for row in &rows {
                let id: String = sqlx::Row::get(row, "id");
                let slug: String = sqlx::Row::get(row, "slug");
                let prefix = derive_key_prefix(&slug);
                sqlx::query("UPDATE projects SET key_prefix = $1 WHERE id = $2")
                    .bind(&prefix)
                    .bind(&id)
                    .execute(&self.pool)
                    .await?;
            }
            tracing::info!("Backfilled key_prefix for {} projects", rows.len());
        }

        // Check if feature_number column already exists on features
        let has_feature_number = if self.dialect.is_sqlite() {
            let schema: Option<String> = sqlx::query_scalar(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name='features'",
            )
            .fetch_optional(&self.pool)
            .await?;

            schema
                .as_ref()
                .map(|s| s.to_lowercase().contains("feature_number"))
                .unwrap_or(false)
        } else {
            let col_exists: Option<String> = sqlx::query_scalar(
                "SELECT column_name FROM information_schema.columns
                 WHERE table_name = 'features' AND column_name = 'feature_number'",
            )
            .fetch_optional(&self.pool)
            .await?;

            col_exists.is_some()
        };

        if !has_feature_number {
            tracing::info!("Adding feature_number column to features table");
            sqlx::query("ALTER TABLE features ADD COLUMN feature_number INTEGER")
                .execute(&self.pool)
                .await?;

            // Backfill feature_number per project using ROW_NUMBER
            if self.dialect.is_sqlite() {
                sqlx::query(
                    "UPDATE features SET feature_number = (
                        SELECT rn FROM (
                            SELECT id, ROW_NUMBER() OVER (PARTITION BY project_id ORDER BY created_at) as rn
                            FROM features
                        ) numbered WHERE numbered.id = features.id
                    )",
                )
                .execute(&self.pool)
                .await?;
            } else {
                sqlx::query(
                    "UPDATE features SET feature_number = sub.rn FROM (
                        SELECT id, ROW_NUMBER() OVER (PARTITION BY project_id ORDER BY created_at) as rn
                        FROM features
                    ) sub WHERE features.id = sub.id",
                )
                .execute(&self.pool)
                .await?;
            }

            // Add unique index
            sqlx::query(
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_features_number ON features(project_id, feature_number)",
            )
            .execute(&self.pool)
            .await?;

            tracing::info!("Backfilled feature_number and created unique index");
        }

        Ok(())
    }

    /// Add 'blocked' to features state CHECK constraint and create feature_blockers table.
    async fn migrate_add_blocked_state(&self) -> Result<()> {
        // Check if feature_blockers table already exists (indicates migration was applied)
        let has_blockers_table = {
            let count: i64 = sqlx::query_scalar(&self.dialect.table_exists_sql("feature_blockers"))
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0);
            count > 0
        };

        if has_blockers_table {
            tracing::debug!("Blocked state migration already applied");
            return Ok(());
        }

        tracing::info!("Adding blocked state and feature_blockers table");

        if self.dialect.is_sqlite() {
            let mut conn = self.pool.acquire().await?;

            sqlx::query("PRAGMA foreign_keys = OFF")
                .execute(&mut *conn)
                .await?;

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
                sqlx::query(sql).execute(&mut *conn).await?;
            }

            // Create feature_blockers table
            sqlx::query(
                "CREATE TABLE feature_blockers (
                    feature_id TEXT NOT NULL,
                    blocker_feature_id TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    PRIMARY KEY (feature_id, blocker_feature_id),
                    FOREIGN KEY (feature_id) REFERENCES features(id) ON DELETE CASCADE,
                    FOREIGN KEY (blocker_feature_id) REFERENCES features(id) ON DELETE CASCADE
                )",
            )
            .execute(&mut *conn)
            .await?;

            sqlx::query("PRAGMA foreign_keys = ON")
                .execute(&mut *conn)
                .await?;
        } else {
            // PostgreSQL
            sqlx::query("ALTER TABLE features DROP CONSTRAINT IF EXISTS chk_features_state")
                .execute(&self.pool)
                .await?;

            sqlx::query(
                "ALTER TABLE features ADD CONSTRAINT chk_features_state
                 CHECK (state IN ('proposed', 'blocked', 'in_progress', 'implemented', 'archived'))",
            )
            .execute(&self.pool)
            .await?;

            sqlx::query(
                "CREATE TABLE IF NOT EXISTS feature_blockers (
                    feature_id TEXT NOT NULL,
                    blocker_feature_id TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    PRIMARY KEY (feature_id, blocker_feature_id),
                    FOREIGN KEY (feature_id) REFERENCES features(id) ON DELETE CASCADE,
                    FOREIGN KEY (blocker_feature_id) REFERENCES features(id) ON DELETE CASCADE
                )",
            )
            .execute(&self.pool)
            .await?;
        }

        tracing::info!("Blocked state migration complete");
        Ok(())
    }

    /// Execute the initial schema SQL.
    async fn run_schema(&self) -> Result<()> {
        let schema = include_str!("../../migrations/20240101000000_initial.sql");

        // Split on semicolons and execute each statement
        for statement in schema.split(';') {
            // Filter out comment lines and empty lines, then rejoin
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
                sqlx::query(sql).execute(&self.pool).await.map_err(|e| {
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
        let has_table = {
            let count: i64 = sqlx::query_scalar(&self.dialect.table_exists_sql("project_focus"))
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0);
            count > 0
        };

        if has_table {
            tracing::debug!("project_focus migration already applied");
            return Ok(());
        }

        tracing::info!("Creating project_focus table");
        sqlx::query(
            "CREATE TABLE project_focus (
                project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
                feature_id TEXT NOT NULL REFERENCES features(id) ON DELETE CASCADE,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Add 'copilot' to tasks.agent_type CHECK constraint.
    /// The tasks table is ephemeral (rows deleted when sessions complete) and
    /// only exists in databases created with the old 001_initial.sql schema.
    async fn migrate_add_copilot_agent_type(&self) -> Result<()> {
        // Only relevant for SQLite databases with the old tasks table
        if !self.dialect.is_sqlite() {
            return Ok(());
        }

        let has_table = {
            let count: i64 = sqlx::query_scalar(&self.dialect.table_exists_sql("tasks"))
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0);
            count > 0
        };

        if !has_table {
            tracing::debug!("copilot migration: no tasks table, skipping");
            return Ok(());
        }

        // Check if 'copilot' is already in the CHECK constraint by trying an insert
        // This is simpler than parsing SQLite's table_info for CHECK constraints.
        // Since the table is always empty in practice, just recreate it.
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
                // Ignore errors from already-applied migration (e.g., tasks_new doesn't exist)
                if let Err(e) = sqlx::query(trimmed).execute(&self.pool).await {
                    tracing::debug!("copilot migration statement skipped: {}", e);
                    return Ok(());
                }
            }
        }

        tracing::info!("Added 'copilot' to tasks.agent_type CHECK constraint");
        Ok(())
    }

    /// Get the underlying connection pool.
    #[must_use]
    pub fn pool(&self) -> &AnyPool {
        &self.pool
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            dialect: self.dialect,
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

        // First migration: creates all tables from scratch
        db.migrate().await.expect("first migration");

        // Second migration: should succeed without errors (IF NOT EXISTS)
        db.migrate().await.expect("second migration (idempotent)");

        // Verify tables exist by running a simple query
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM features")
            .fetch_one(db.pool())
            .await
            .expect("query features table");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn migrate_then_incremental_is_idempotent() {
        let db = Database::open_memory().await.expect("open in-memory db");

        // Run initial schema
        db.migrate().await.expect("initial migration");

        // Incremental migrations should be safe to re-run on a fresh schema
        db.run_incremental_migrations()
            .await
            .expect("incremental migrations on fresh schema");
    }
}
