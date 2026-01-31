use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::any::AnyRow;
use sqlx::{AnyPool, Row};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::models::*;

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
    /// # Safety
    /// The `table_name` parameter is interpolated directly into SQL.
    /// This is safe because all callers pass hardcoded string literals.
    /// Do NOT call this with user-provided input.
    pub fn table_exists_sql(&self, table_name: &str) -> String {
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
    pub fn is_sqlite(&self) -> bool {
        matches!(self, DbDialect::Sqlite)
    }

    /// Returns true if this dialect is PostgreSQL.
    pub fn is_postgres(&self) -> bool {
        matches!(self, DbDialect::Postgres)
    }

    /// SQL fragment for unlimited results with an offset.
    /// SQLite uses `LIMIT -1 OFFSET n`, PostgreSQL uses `LIMIT ALL OFFSET n`.
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
fn escape_like_pattern(query: &str) -> String {
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
    Created { project_id: Uuid },
    Updated { project_id: Uuid },
    Deleted { project_id: Uuid },
}

impl FeatureEvent {
    pub fn project_id(&self) -> Uuid {
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
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    InvalidState(String),
}

impl ManifestError {
    pub fn not_found(entity: &str) -> Self {
        ManifestError::NotFound(format!("{} not found", entity))
    }

    pub fn validation(msg: impl Into<String>) -> Self {
        ManifestError::Validation(msg.into())
    }

    pub fn invalid_state(msg: impl Into<String>) -> Self {
        ManifestError::InvalidState(msg.into())
    }

    pub fn is_client_error(&self) -> bool {
        true
    }
}

const EVENT_CHANNEL_CAPACITY: usize = 16;

/// Migration report for root feature migration.
#[derive(Debug, Clone, Default)]
pub struct RootFeatureMigrationReport {
    pub projects_migrated: usize,
    pub features_reparented: usize,
    pub projects_skipped: usize,
}

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
        let pool = AnyPool::connect(url).await?;

        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);

        Ok(Self {
            pool,
            dialect,
            events,
        })
    }

    /// Returns the database dialect (SQLite or PostgreSQL).
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
        let db = Self::connect(&url).await?;

        // Enable WAL mode and foreign keys for SQLite
        if url.starts_with("sqlite:") {
            sqlx::query("PRAGMA journal_mode = WAL")
                .execute(&db.pool)
                .await
                .ok();
            sqlx::query("PRAGMA foreign_keys = ON")
                .execute(&db.pool)
                .await
                .ok();
        }

        Ok(db)
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
        let db = Self::connect(&url).await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&db.pool)
            .await
            .ok();
        Ok(db)
    }

    /// Subscribe to feature change events.
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

    /// Get the underlying connection pool.
    pub fn pool(&self) -> &AnyPool {
        &self.pool
    }

    // ============================================================
    // Project operations
    // ============================================================

    pub async fn get_all_projects(&self) -> Result<Vec<Project>> {
        let rows = sqlx::query(
            "SELECT id, slug, name, description, instructions, current_version_id, root_feature_id, default_feature_destination, created_at, updated_at
             FROM projects ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_project).collect()
    }

    pub async fn get_project(&self, id: Uuid) -> Result<Option<Project>> {
        let row = sqlx::query(
            "SELECT id, slug, name, description, instructions, current_version_id, root_feature_id, default_feature_destination, created_at, updated_at
             FROM projects WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_project).transpose()
    }

    pub async fn get_project_by_slug(&self, slug: &str) -> Result<Option<Project>> {
        let row = sqlx::query(
            "SELECT id, slug, name, description, instructions, current_version_id, root_feature_id, default_feature_destination, created_at, updated_at
             FROM projects WHERE slug = $1",
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_project).transpose()
    }

    pub async fn create_project(&self, input: CreateProjectInput) -> Result<Project> {
        let project_id = Uuid::new_v4();
        let root_feature_id = Uuid::new_v4();
        let now = Utc::now();

        // Generate slug from name if not provided
        let slug = input.slug.unwrap_or_else(|| slugify(&input.name));

        let mut tx = self.pool.begin().await?;

        // Create project with root_feature_id
        sqlx::query(
            "INSERT INTO projects (id, slug, name, description, instructions, root_feature_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(project_id.to_string())
        .bind(&slug)
        .bind(&input.name)
        .bind(&input.description)
        .bind(&input.instructions)
        .bind(root_feature_id.to_string())
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        // Create root feature
        sqlx::query(
            "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, created_at, updated_at)
             VALUES ($1, $2, NULL, $3, $4, 'implemented', 0, $5, $6)",
        )
        .bind(root_feature_id.to_string())
        .bind(project_id.to_string())
        .bind(&input.name)
        .bind(&input.instructions)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(Project {
            id: project_id,
            slug,
            name: input.name,
            description: input.description,
            instructions: input.instructions,
            current_version_id: None,
            root_feature_id: Some(root_feature_id),
            default_feature_destination: "backlog".to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn update_project(
        &self,
        id: Uuid,
        input: UpdateProjectInput,
    ) -> Result<Option<Project>> {
        let Some(existing) = self.get_project(id).await? else {
            return Ok(None);
        };

        let now = Utc::now();
        let name_changed = input.name.is_some() && input.name.as_ref() != Some(&existing.name);
        let slug = input.slug.unwrap_or(existing.slug);
        let name = input.name.unwrap_or(existing.name);
        let description = input.description.or(existing.description);
        let instructions = input.instructions.or(existing.instructions);
        let current_version_id = input.current_version_id.or(existing.current_version_id);
        let default_feature_destination = input
            .default_feature_destination
            .unwrap_or(existing.default_feature_destination);

        sqlx::query(
            "UPDATE projects SET slug = $1, name = $2, description = $3, instructions = $4, current_version_id = $5, default_feature_destination = $6, updated_at = $7 WHERE id = $8",
        )
        .bind(&slug)
        .bind(&name)
        .bind(&description)
        .bind(&instructions)
        .bind(current_version_id.map(|u| u.to_string()))
        .bind(&default_feature_destination)
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;

        // Sync name to root feature title if changed
        if name_changed {
            if let Some(root_id) = existing.root_feature_id {
                sqlx::query("UPDATE features SET title = $1, updated_at = $2 WHERE id = $3")
                    .bind(&name)
                    .bind(now.to_rfc3339())
                    .bind(root_id.to_string())
                    .execute(&self.pool)
                    .await?;
            }
        }

        Ok(Some(Project {
            id,
            slug,
            name,
            description,
            instructions,
            current_version_id,
            root_feature_id: existing.root_feature_id,
            default_feature_destination,
            created_at: existing.created_at,
            updated_at: now,
        }))
    }

    pub async fn delete_project(&self, id: Uuid) -> Result<bool> {
        let mut tx = self.pool.begin().await?;

        // Delete feature history
        sqlx::query(
            "DELETE FROM feature_history WHERE feature_id IN (SELECT id FROM features WHERE project_id = $1)",
        )
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;

        // Delete features
        sqlx::query("DELETE FROM features WHERE project_id = $1")
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;

        // Delete directories
        sqlx::query("DELETE FROM project_directories WHERE project_id = $1")
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;

        // Delete versions
        sqlx::query("DELETE FROM versions WHERE project_id = $1")
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;

        // Delete project
        let result = sqlx::query("DELETE FROM projects WHERE id = $1")
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        Ok(result.rows_affected() > 0)
    }

    // ============================================================
    // Project Directory operations
    // ============================================================

    pub async fn get_project_directories(&self, project_id: Uuid) -> Result<Vec<ProjectDirectory>> {
        let rows = sqlx::query(
            "SELECT id, project_id, path, git_remote, is_primary, instructions, created_at
             FROM project_directories WHERE project_id = $1 ORDER BY is_primary DESC, path",
        )
        .bind(project_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_project_directory).collect()
    }

    pub async fn add_project_directory(
        &self,
        project_id: Uuid,
        input: AddDirectoryInput,
    ) -> Result<ProjectDirectory> {
        self.get_project(project_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        let id = Uuid::new_v4();
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO project_directories (id, project_id, path, git_remote, is_primary, instructions, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(id.to_string())
        .bind(project_id.to_string())
        .bind(&input.path)
        .bind(&input.git_remote)
        .bind(if input.is_primary { 1i32 } else { 0i32 })
        .bind(&input.instructions)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(ProjectDirectory {
            id,
            project_id,
            path: input.path,
            git_remote: input.git_remote,
            is_primary: input.is_primary,
            instructions: input.instructions,
            created_at: now,
        })
    }

    pub async fn remove_project_directory(&self, id: Uuid) -> Result<bool> {
        let result = sqlx::query("DELETE FROM project_directories WHERE id = $1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn get_project_with_directories(
        &self,
        id: Uuid,
    ) -> Result<Option<ProjectWithDirectories>> {
        let project = match self.get_project(id).await? {
            Some(p) => p,
            None => return Ok(None),
        };
        let directories = self.get_project_directories(id).await?;
        Ok(Some(ProjectWithDirectories {
            project,
            directories,
        }))
    }

    pub async fn get_project_by_directory(
        &self,
        path: &str,
    ) -> Result<Option<ProjectWithDirectories>> {
        let rows = sqlx::query(
            "SELECT project_id, path FROM project_directories ORDER BY length(path) DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        for row in &rows {
            let dir_path: String = row.get("path");
            if path == dir_path || path.starts_with(&format!("{}/", dir_path)) {
                let project_id_str: String = row.get("project_id");
                return self
                    .get_project_with_directories(parse_uuid(project_id_str)?)
                    .await;
            }
        }

        Ok(None)
    }

    // ============================================================
    // Version operations
    // ============================================================

    pub async fn get_versions_by_project(&self, project_id: Uuid) -> Result<Vec<Version>> {
        let rows = sqlx::query(
            "SELECT id, project_id, name, description, released_at, created_at, updated_at
             FROM versions WHERE project_id = $1 ORDER BY created_at",
        )
        .bind(project_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_version).collect()
    }

    pub async fn get_version(&self, id: Uuid) -> Result<Option<Version>> {
        let row = sqlx::query(
            "SELECT id, project_id, name, description, released_at, created_at, updated_at
             FROM versions WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_version).transpose()
    }

    /// Get the "Next" version (first unreleased version) for a project.
    /// Returns None if no unreleased versions exist.
    pub async fn get_next_version(&self, project_id: Uuid) -> Result<Option<Version>> {
        let row = sqlx::query(
            "SELECT id, project_id, name, description, released_at, created_at, updated_at
             FROM versions WHERE project_id = $1 AND released_at IS NULL ORDER BY created_at LIMIT 1",
        )
        .bind(project_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_version).transpose()
    }

    /// Get the latest unreleased version for a project (for new feature assignment).
    /// Returns None if no unreleased versions exist.
    pub async fn get_latest_version(&self, project_id: Uuid) -> Result<Option<Version>> {
        let row = sqlx::query(
            "SELECT id, project_id, name, description, released_at, created_at, updated_at
             FROM versions WHERE project_id = $1 AND released_at IS NULL ORDER BY created_at DESC LIMIT 1",
        )
        .bind(project_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_version).transpose()
    }

    /// Ensure at least `min_count` unreleased versions exist for a project.
    /// Auto-creates versions with incremented minor version numbers as needed.
    pub async fn ensure_minimum_versions(
        &self,
        project_id: Uuid,
        min_count: usize,
    ) -> Result<Vec<Version>> {
        let mut all_versions = self.get_versions_by_project(project_id).await?;
        let unreleased_count = all_versions
            .iter()
            .filter(|v| v.released_at.is_none())
            .count();

        let mut created = Vec::new();
        if unreleased_count >= min_count {
            return Ok(created);
        }

        let needed = min_count - unreleased_count;
        for _ in 0..needed {
            let next_name = compute_next_version_name(&all_versions);
            let version = self
                .create_version(
                    project_id,
                    CreateVersionInput {
                        name: next_name,
                        description: None,
                    },
                )
                .await?;
            all_versions.push(version.clone());
            created.push(version);
        }

        Ok(created)
    }

    pub async fn create_version(
        &self,
        project_id: Uuid,
        input: CreateVersionInput,
    ) -> Result<Version> {
        // Guard rail: version names must be semantic versions
        if !is_valid_semver(&input.name) {
            return Err(ManifestError::validation(format!(
                "'{}' is not a valid semantic version. Use the format MAJOR.MINOR.PATCH (e.g., '0.1.0', '1.0.0') with an optional 'v' prefix (e.g., 'v0.1.0').",
                input.name
            ))
            .into());
        }

        self.get_project(project_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        // Guard rail: cap unreleased versions at 6
        let versions = self.get_versions_by_project(project_id).await?;
        let unreleased_count = versions.iter().filter(|v| v.released_at.is_none()).count();
        if unreleased_count >= 6 {
            return Err(ManifestError::validation(format!(
                "Project already has {} unreleased versions (max 6). Release or delete existing versions before creating new ones.",
                unreleased_count
            ))
            .into());
        }

        let id = Uuid::new_v4();
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO versions (id, project_id, name, description, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id.to_string())
        .bind(project_id.to_string())
        .bind(&input.name)
        .bind(&input.description)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(Version {
            id,
            project_id,
            name: input.name,
            description: input.description,
            released_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn update_version(
        &self,
        id: Uuid,
        input: UpdateVersionInput,
    ) -> Result<Option<Version>> {
        let Some(existing) = self.get_version(id).await? else {
            return Ok(None);
        };

        let now = Utc::now();
        let name = input.name.unwrap_or(existing.name);
        let description = input.description.or(existing.description);
        let released_at = input.released_at.or(existing.released_at);

        sqlx::query(
            "UPDATE versions SET name = $1, description = $2, released_at = $3, updated_at = $4 WHERE id = $5",
        )
        .bind(&name)
        .bind(&description)
        .bind(released_at.map(|d| d.to_rfc3339()))
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;

        Ok(Some(Version {
            id,
            project_id: existing.project_id,
            name,
            description,
            released_at,
            created_at: existing.created_at,
            updated_at: now,
        }))
    }

    pub async fn delete_version(&self, id: Uuid) -> Result<bool> {
        let result = sqlx::query("DELETE FROM versions WHERE id = $1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Validate that a version has not been released.
    /// Returns an error if the version is released, preventing feature assignment to past versions.
    async fn validate_version_not_released(&self, version_id: Uuid) -> Result<()> {
        let version = self
            .get_version(version_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Version"))?;

        if version.released_at.is_some() {
            return Err(ManifestError::validation(format!(
                "Cannot assign features to released version '{}'. Use list_versions to find unreleased versions.",
                version.name
            ))
            .into());
        }

        Ok(())
    }

    // ============================================================
    // Feature operations
    // ============================================================

    pub async fn get_all_features_paginated(
        &self,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Feature>> {
        let rows = match (limit, offset) {
            (Some(lim), Some(off)) => {
                sqlx::query(
                    "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                     FROM features ORDER BY priority, title LIMIT $1 OFFSET $2",
                )
                .bind(lim as i64)
                .bind(off as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(lim), None) => {
                sqlx::query(
                    "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                     FROM features ORDER BY priority, title LIMIT $1",
                )
                .bind(lim as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(off)) => {
                let sql = format!(
                    "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                     FROM features ORDER BY priority, title {} OFFSET $1",
                    self.dialect.unlimited_offset_sql()
                );
                sqlx::query(&sql)
                    .bind(off as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                     FROM features ORDER BY priority, title",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };

        rows.iter().map(row_to_feature).collect()
    }

    pub async fn get_all_features(&self) -> Result<Vec<Feature>> {
        self.get_all_features_paginated(None, None).await
    }

    pub async fn get_features_by_project_paginated(
        &self,
        project_id: Uuid,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Feature>> {
        let rows = match (limit, offset) {
            (Some(lim), Some(off)) => {
                sqlx::query(
                    "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                     FROM features WHERE project_id = $1 ORDER BY priority, title LIMIT $2 OFFSET $3",
                )
                .bind(project_id.to_string())
                .bind(lim as i64)
                .bind(off as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(lim), None) => {
                sqlx::query(
                    "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                     FROM features WHERE project_id = $1 ORDER BY priority, title LIMIT $2",
                )
                .bind(project_id.to_string())
                .bind(lim as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(off)) => {
                let sql = format!(
                    "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                     FROM features WHERE project_id = $1 ORDER BY priority, title {} OFFSET $2",
                    self.dialect.unlimited_offset_sql()
                );
                sqlx::query(&sql)
                    .bind(project_id.to_string())
                    .bind(off as i64)
                    .fetch_all(&self.pool)
                    .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                     FROM features WHERE project_id = $1 ORDER BY priority, title",
                )
                .bind(project_id.to_string())
                .fetch_all(&self.pool)
                .await?
            }
        };

        rows.iter().map(row_to_feature).collect()
    }

    pub async fn get_features_by_project(&self, project_id: Uuid) -> Result<Vec<Feature>> {
        self.get_features_by_project_paginated(project_id, None, None)
            .await
    }

    pub async fn get_feature(&self, id: Uuid) -> Result<Option<Feature>> {
        let row = sqlx::query(
            "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
             FROM features WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_feature).transpose()
    }

    pub async fn get_feature_diff(&self, id: Uuid) -> Result<Option<FeatureDiff>> {
        let feature = match self.get_feature(id).await? {
            Some(f) => f,
            None => return Ok(None),
        };

        let has_changes =
            feature.desired_details.is_some() && feature.desired_details != feature.details;

        Ok(Some(FeatureDiff {
            has_changes,
            current: feature.details,
            desired: feature.desired_details,
        }))
    }

    pub async fn create_feature(
        &self,
        project_id: Uuid,
        input: CreateFeatureInput,
    ) -> Result<Feature> {
        let project = self
            .get_project(project_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        // Guard rail: features must always have a parent
        let parent_id = input
            .parent_id
            .or(project.root_feature_id)
            .ok_or_else(|| ManifestError::validation("Feature must have a parent"))?;

        let id = input.id.unwrap_or_else(Uuid::new_v4);
        let now = Utc::now();
        let state = input.state.unwrap_or(FeatureState::Proposed);
        let priority = input.priority.unwrap_or(0);

        // Guard rail: reject assignment to released versions
        if let Some(vid) = input.target_version_id {
            self.validate_version_not_released(vid).await?;
        }

        // Guard rail: in-progress/implemented features must be in the "next" version
        let target_version_id =
            if state == FeatureState::InProgress || state == FeatureState::Implemented {
                let next_version = self.get_next_version(project_id).await?.map(|v| v.id);
                next_version.or(input.target_version_id)
            } else {
                match input.target_version_id {
                    Some(vid) => Some(vid),
                    None => {
                        if project.default_feature_destination == "next" {
                            self.get_next_version(project_id).await?.map(|v| v.id)
                        } else {
                            None // backlog
                        }
                    }
                }
            };

        sqlx::query(
            "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, target_version_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(id.to_string())
        .bind(project_id.to_string())
        .bind(Some(parent_id.to_string()))
        .bind(&input.title)
        .bind(&input.details)
        .bind(state.as_str())
        .bind(priority)
        .bind(target_version_id.map(|u| u.to_string()))
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        let _ = self.events.send(FeatureEvent::Created { project_id });

        Ok(Feature {
            id,
            project_id,
            parent_id: Some(parent_id),
            title: input.title,
            details: input.details,
            desired_details: None,
            state,
            priority,
            target_version_id,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn create_features_bulk(
        &self,
        project_id: Uuid,
        inputs: Vec<CreateFeatureInput>,
    ) -> Result<Vec<Feature>> {
        let project = self
            .get_project(project_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        let now = Utc::now();
        let mut features = Vec::with_capacity(inputs.len());

        // Get default version and "next" version based on project setting
        let next_version_id = self.get_next_version(project_id).await?.map(|v| v.id);
        let default_version_id = if project.default_feature_destination == "next" {
            next_version_id
        } else {
            None // backlog
        };

        // Guard rail: reject assignment to released versions (validate once per unique version)
        let mut validated_versions = std::collections::HashSet::new();
        for input in &inputs {
            if let Some(vid) = input.target_version_id {
                if validated_versions.insert(vid) {
                    self.validate_version_not_released(vid).await?;
                }
            }
        }

        let mut tx = self.pool.begin().await?;

        for input in inputs {
            let id = input.id.unwrap_or_else(Uuid::new_v4);
            let state = input.state.unwrap_or(FeatureState::Proposed);
            let priority = input.priority.unwrap_or(0);

            // Guard rail: features must always have a parent
            let parent_id = input
                .parent_id
                .or(project.root_feature_id)
                .ok_or_else(|| ManifestError::validation("Feature must have a parent"))?;

            // Guard rail: in-progress/implemented features must be in the "next" version
            let target_version_id =
                if state == FeatureState::InProgress || state == FeatureState::Implemented {
                    next_version_id.or(input.target_version_id)
                } else {
                    input.target_version_id.or(default_version_id)
                };

            sqlx::query(
                "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, target_version_id, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
            )
            .bind(id.to_string())
            .bind(project_id.to_string())
            .bind(Some(parent_id.to_string()))
            .bind(&input.title)
            .bind(&input.details)
            .bind(state.as_str())
            .bind(priority)
            .bind(target_version_id.map(|u| u.to_string()))
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .execute(&mut *tx)
            .await?;

            features.push(Feature {
                id,
                project_id,
                parent_id: Some(parent_id),
                title: input.title,
                details: input.details,
                desired_details: None,
                state,
                priority,
                target_version_id,
                created_at: now,
                updated_at: now,
            });
        }

        tx.commit().await?;

        let _ = self.events.send(FeatureEvent::Created { project_id });

        Ok(features)
    }

    pub async fn update_feature(
        &self,
        id: Uuid,
        input: UpdateFeatureInput,
    ) -> Result<Option<Feature>> {
        let Some(existing) = self.get_feature(id).await? else {
            return Ok(None);
        };

        let now = Utc::now();
        let title = input.title.unwrap_or(existing.title);
        let details = input.details.or(existing.details);
        // Guard rail: setting desired_details (proposing changes) must not change state.
        // AI agents may accidentally set state alongside desired_details.
        let had_desired_details = existing.desired_details.is_some();
        let desired_details = input.desired_details.unwrap_or(existing.desired_details);
        let is_proposing_changes = desired_details.is_some() && !had_desired_details;
        let state = if is_proposing_changes {
            existing.state
        } else {
            input.state.unwrap_or(existing.state)
        };
        let parent_id = input.parent_id.or(existing.parent_id);
        let priority = input.priority.unwrap_or(existing.priority);
        // Guard rail: reject explicit assignment to released versions
        if let Some(Some(vid)) = &input.target_version_id {
            self.validate_version_not_released(*vid).await?;
        }

        let mut target_version_id = input
            .target_version_id
            .unwrap_or(existing.target_version_id);

        // Guard rail: in-progress features must always be in the "next" version
        if state == FeatureState::InProgress && existing.state != FeatureState::InProgress {
            if let Some(next_ver) = self.get_next_version(existing.project_id).await? {
                target_version_id = Some(next_ver.id);
            }
        }

        // Guard rail: implemented features must have a version (assign to "next" if none)
        if state == FeatureState::Implemented && target_version_id.is_none() {
            target_version_id = self
                .get_next_version(existing.project_id)
                .await?
                .map(|v| v.id);
        }

        sqlx::query(
            "UPDATE features SET parent_id = $1, title = $2, details = $3, desired_details = $4, state = $5, priority = $6, target_version_id = $7, updated_at = $8 WHERE id = $9",
        )
        .bind(parent_id.map(|u| u.to_string()))
        .bind(&title)
        .bind(&details)
        .bind(&desired_details)
        .bind(state.as_str())
        .bind(priority)
        .bind(target_version_id.map(|u| u.to_string()))
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;

        let _ = self.events.send(FeatureEvent::Updated {
            project_id: existing.project_id,
        });

        Ok(Some(Feature {
            id,
            project_id: existing.project_id,
            parent_id,
            title,
            details,
            desired_details,
            state,
            priority,
            target_version_id,
            created_at: existing.created_at,
            updated_at: now,
        }))
    }

    pub async fn delete_feature(&self, id: Uuid) -> Result<bool> {
        // Get project_id before deleting
        let project_id: Option<Uuid> =
            sqlx::query_scalar("SELECT project_id FROM features WHERE id = $1")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await?
                .map(|s: String| parse_uuid(s))
                .transpose()?;

        let id_str = id.to_string();

        // Delete feature history for descendants (recursive CTE)
        sqlx::query(
            "DELETE FROM feature_history WHERE feature_id IN (
                WITH RECURSIVE descendants AS (
                    SELECT id FROM features WHERE id = $1
                    UNION ALL
                    SELECT f.id FROM features f
                    INNER JOIN descendants d ON f.parent_id = d.id
                )
                SELECT id FROM descendants
            )",
        )
        .bind(&id_str)
        .execute(&self.pool)
        .await?;

        // Delete descendants and feature
        let result = sqlx::query(
            "DELETE FROM features WHERE id IN (
                WITH RECURSIVE descendants AS (
                    SELECT id FROM features WHERE id = $1
                    UNION ALL
                    SELECT f.id FROM features f
                    INNER JOIN descendants d ON f.parent_id = d.id
                )
                SELECT id FROM descendants
            )",
        )
        .bind(&id_str)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() > 0 {
            if let Some(project_id) = project_id {
                let _ = self.events.send(FeatureEvent::Deleted { project_id });
            }
        }

        Ok(result.rows_affected() > 0)
    }

    pub async fn get_root_features(&self, project_id: Uuid) -> Result<Vec<Feature>> {
        let project = self.get_project(project_id).await?;

        let rows = match project.and_then(|p| p.root_feature_id) {
            Some(root_id) => {
                sqlx::query(
                    "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                     FROM features WHERE project_id = $1 AND parent_id = $2 ORDER BY priority, title",
                )
                .bind(project_id.to_string())
                .bind(root_id.to_string())
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query(
                    "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                     FROM features WHERE project_id = $1 AND parent_id IS NULL ORDER BY priority, title",
                )
                .bind(project_id.to_string())
                .fetch_all(&self.pool)
                .await?
            }
        };

        rows.iter().map(row_to_feature).collect()
    }

    pub async fn get_children(&self, parent_id: Uuid) -> Result<Vec<Feature>> {
        let rows = sqlx::query(
            "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
             FROM features WHERE parent_id = $1 ORDER BY priority, title",
        )
        .bind(parent_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_feature).collect()
    }

    pub async fn is_leaf(&self, feature_id: Uuid) -> Result<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM features WHERE parent_id = $1")
            .bind(feature_id.to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(count == 0)
    }

    pub async fn search_features(
        &self,
        query: &str,
        project_id: Option<Uuid>,
        limit: Option<u32>,
    ) -> Result<Vec<FeatureSummary>> {
        let escaped_query = escape_like_pattern(query);
        let search_pattern = format!("%{}%", escaped_query);
        let limit_val = limit.unwrap_or(10) as i64;

        let rows = match project_id {
            Some(pid) => {
                sqlx::query(
                    "SELECT id, project_id, parent_id, title, state, priority, target_version_id
                     FROM features
                     WHERE project_id = $1 AND (title LIKE $2 ESCAPE '\\' OR details LIKE $2 ESCAPE '\\')
                     ORDER BY
                         CASE WHEN title LIKE $2 ESCAPE '\\' THEN 0 ELSE 1 END,
                         priority,
                         title
                     LIMIT $3",
                )
                .bind(pid.to_string())
                .bind(&search_pattern)
                .bind(limit_val)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query(
                    "SELECT id, project_id, parent_id, title, state, priority, target_version_id
                     FROM features
                     WHERE title LIKE $1 ESCAPE '\\' OR details LIKE $1 ESCAPE '\\'
                     ORDER BY
                         CASE WHEN title LIKE $1 ESCAPE '\\' THEN 0 ELSE 1 END,
                         priority,
                         title
                     LIMIT $2",
                )
                .bind(&search_pattern)
                .bind(limit_val)
                .fetch_all(&self.pool)
                .await?
            }
        };

        rows.iter().map(row_to_feature_summary).collect()
    }

    pub async fn get_feature_tree(&self, project_id: Uuid) -> Result<Vec<FeatureTreeNode>> {
        let project = self.get_project(project_id).await?;
        let root_feature_id = project.and_then(|p| p.root_feature_id);
        let features = self.get_features_by_project(project_id).await?;

        let mut children_map: std::collections::HashMap<Option<Uuid>, Vec<Feature>> =
            std::collections::HashMap::new();
        let mut root_feature: Option<Feature> = None;

        for feature in features {
            if Some(feature.id) == root_feature_id {
                root_feature = Some(feature);
                continue;
            }
            children_map
                .entry(feature.parent_id)
                .or_default()
                .push(feature);
        }

        fn build_subtree(
            parent_id: Option<Uuid>,
            children_map: &std::collections::HashMap<Option<Uuid>, Vec<Feature>>,
        ) -> Vec<FeatureTreeNode> {
            children_map
                .get(&parent_id)
                .map(|features| {
                    features
                        .iter()
                        .map(|f| FeatureTreeNode {
                            feature: f.clone(),
                            children: build_subtree(Some(f.id), children_map),
                            is_root: false,
                        })
                        .collect()
                })
                .unwrap_or_default()
        }

        if let Some(root) = root_feature {
            let children = build_subtree(Some(root.id), &children_map);
            Ok(vec![FeatureTreeNode {
                feature: root,
                children,
                is_root: true,
            }])
        } else {
            Ok(build_subtree(None, &children_map))
        }
    }

    // ============================================================
    // Feature History operations
    // ============================================================

    pub async fn create_history_entry(&self, input: CreateHistoryInput) -> Result<FeatureHistory> {
        let id = Uuid::new_v4();
        let now = Utc::now();

        // Get version_id from input or feature's target_version_id
        let version_id = match input.version_id {
            Some(vid) => Some(vid),
            None => sqlx::query_scalar::<_, Option<String>>(
                "SELECT target_version_id FROM features WHERE id = $1",
            )
            .bind(input.feature_id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .flatten()
            .map(parse_uuid)
            .transpose()?,
        };

        let details_json = serde_json::to_string(&input.details)?;

        sqlx::query(
            "INSERT INTO feature_history (id, feature_id, version_id, summary, details, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id.to_string())
        .bind(input.feature_id.to_string())
        .bind(version_id.map(|u| u.to_string()))
        .bind(&input.details.summary)
        .bind(&details_json)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(FeatureHistory {
            id,
            feature_id: input.feature_id,
            version_id,
            details: input.details,
            created_at: now,
        })
    }

    pub async fn get_feature_history(&self, feature_id: Uuid) -> Result<Vec<FeatureHistory>> {
        let rows = sqlx::query(
            "SELECT id, feature_id, version_id, details, created_at
             FROM feature_history WHERE feature_id = $1 ORDER BY created_at DESC",
        )
        .bind(feature_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_feature_history).collect()
    }

    pub async fn get_project_history(
        &self,
        project_id: Uuid,
        version_id: Option<Uuid>,
        limit: Option<u32>,
        offset: Option<u32>,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<ProjectHistoryEntry>> {
        let limit_val = limit.unwrap_or(50) as i64;
        let offset_val = offset.unwrap_or(0) as i64;

        // Build dynamic query based on filters
        let rows = match (version_id, since) {
            (Some(vid), Some(since_dt)) => {
                sqlx::query(
                    "SELECT fh.id, fh.feature_id, f.title, f.state, fh.version_id, v.name, fh.details, fh.created_at
                     FROM feature_history fh
                     INNER JOIN features f ON f.id = fh.feature_id
                     LEFT JOIN versions v ON v.id = fh.version_id
                     WHERE f.project_id = $1 AND fh.version_id = $2 AND fh.created_at > $3
                     ORDER BY fh.created_at DESC LIMIT $4 OFFSET $5",
                )
                .bind(project_id.to_string())
                .bind(vid.to_string())
                .bind(since_dt.to_rfc3339())
                .bind(limit_val)
                .bind(offset_val)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(vid), None) => {
                sqlx::query(
                    "SELECT fh.id, fh.feature_id, f.title, f.state, fh.version_id, v.name, fh.details, fh.created_at
                     FROM feature_history fh
                     INNER JOIN features f ON f.id = fh.feature_id
                     LEFT JOIN versions v ON v.id = fh.version_id
                     WHERE f.project_id = $1 AND fh.version_id = $2
                     ORDER BY fh.created_at DESC LIMIT $3 OFFSET $4",
                )
                .bind(project_id.to_string())
                .bind(vid.to_string())
                .bind(limit_val)
                .bind(offset_val)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(since_dt)) => {
                sqlx::query(
                    "SELECT fh.id, fh.feature_id, f.title, f.state, fh.version_id, v.name, fh.details, fh.created_at
                     FROM feature_history fh
                     INNER JOIN features f ON f.id = fh.feature_id
                     LEFT JOIN versions v ON v.id = fh.version_id
                     WHERE f.project_id = $1 AND fh.created_at > $2
                     ORDER BY fh.created_at DESC LIMIT $3 OFFSET $4",
                )
                .bind(project_id.to_string())
                .bind(since_dt.to_rfc3339())
                .bind(limit_val)
                .bind(offset_val)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT fh.id, fh.feature_id, f.title, f.state, fh.version_id, v.name, fh.details, fh.created_at
                     FROM feature_history fh
                     INNER JOIN features f ON f.id = fh.feature_id
                     LEFT JOIN versions v ON v.id = fh.version_id
                     WHERE f.project_id = $1
                     ORDER BY fh.created_at DESC LIMIT $2 OFFSET $3",
                )
                .bind(project_id.to_string())
                .bind(limit_val)
                .bind(offset_val)
                .fetch_all(&self.pool)
                .await?
            }
        };

        rows.iter().map(row_to_project_history_entry).collect()
    }

    // ============================================================
    // Feature Context
    // ============================================================

    pub async fn get_feature_with_context(&self, id: Uuid) -> Result<Option<FeatureWithContext>> {
        let feature = match self.get_feature(id).await? {
            Some(f) => f,
            None => return Ok(None),
        };

        // Get parent
        let parent = if let Some(parent_id) = feature.parent_id {
            sqlx::query("SELECT id, title, state FROM features WHERE id = $1")
                .bind(parent_id.to_string())
                .fetch_optional(&self.pool)
                .await?
                .map(|row| row_to_feature_summary_context(&row))
                .transpose()?
        } else {
            None
        };

        // Get siblings
        let siblings: Vec<FeatureSummaryContext> = if let Some(parent_id) = feature.parent_id {
            let rows = sqlx::query(
                "SELECT id, title, state FROM features
                 WHERE parent_id = $1 AND id != $2
                 ORDER BY priority, title",
            )
            .bind(parent_id.to_string())
            .bind(id.to_string())
            .fetch_all(&self.pool)
            .await?;
            rows.iter()
                .map(row_to_feature_summary_context)
                .collect::<Result<Vec<_>>>()?
        } else {
            let rows = sqlx::query(
                "SELECT id, title, state FROM features
                 WHERE project_id = $1 AND parent_id IS NULL AND id != $2
                 ORDER BY priority, title",
            )
            .bind(feature.project_id.to_string())
            .bind(id.to_string())
            .fetch_all(&self.pool)
            .await?;
            rows.iter()
                .map(row_to_feature_summary_context)
                .collect::<Result<Vec<_>>>()?
        };

        // Get children
        let children_rows = sqlx::query(
            "SELECT id, title, state FROM features
             WHERE parent_id = $1
             ORDER BY priority, title",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let children: Vec<FeatureSummaryContext> = children_rows
            .iter()
            .map(row_to_feature_summary_context)
            .collect::<Result<Vec<_>>>()?;

        // Get breadcrumb using recursive CTE (includes details for ancestor context)
        let breadcrumb_rows = sqlx::query(
            "WITH RECURSIVE ancestors AS (
                SELECT id, parent_id, title, details, 0 as depth FROM features WHERE id = $1
                UNION ALL
                SELECT f.id, f.parent_id, f.title, f.details, a.depth + 1
                FROM features f
                INNER JOIN ancestors a ON f.id = a.parent_id
            )
            SELECT id, title, details FROM ancestors ORDER BY depth DESC",
        )
        .bind(id.to_string())
        .fetch_all(&self.pool)
        .await?;

        let breadcrumb: Vec<BreadcrumbItem> = breadcrumb_rows
            .iter()
            .map(|row| -> Result<BreadcrumbItem> {
                Ok(BreadcrumbItem {
                    id: parse_uuid(row.get::<String, _>("id"))?,
                    title: row.get("title"),
                    details: row.get("details"),
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Some(FeatureWithContext {
            feature,
            parent,
            siblings,
            children,
            breadcrumb,
        }))
    }

    pub async fn get_next_workable_feature(
        &self,
        project_id: Uuid,
        version_id: Option<Uuid>,
    ) -> Result<Option<Feature>> {
        let row = if let Some(vid) = version_id {
            sqlx::query(
                "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                 FROM features
                 WHERE project_id = $1
                   AND target_version_id = $2
                   AND state IN ('proposed', 'in_progress')
                 ORDER BY priority ASC, created_at ASC
                 LIMIT 1",
            )
            .bind(project_id.to_string())
            .bind(vid.to_string())
            .fetch_optional(&self.pool)
            .await?
        } else {
            sqlx::query(
                "WITH next_version AS (
                    SELECT id FROM versions
                    WHERE project_id = $1 AND released_at IS NULL
                    ORDER BY created_at ASC LIMIT 1
                )
                SELECT f.id, f.project_id, f.parent_id, f.title, f.details, f.desired_details, f.state, f.priority, f.target_version_id, f.created_at, f.updated_at
                FROM features f
                LEFT JOIN next_version nv ON f.target_version_id = nv.id
                WHERE f.project_id = $1
                  AND f.state IN ('proposed', 'in_progress')
                ORDER BY
                    CASE WHEN f.target_version_id IS NOT NULL AND f.target_version_id = (SELECT id FROM next_version) THEN 0
                         WHEN f.target_version_id IS NULL THEN 1
                         ELSE 2 END,
                    f.priority ASC,
                    f.created_at ASC
                LIMIT 1",
            )
            .bind(project_id.to_string())
            .fetch_optional(&self.pool)
            .await?
        };

        row.as_ref().map(row_to_feature).transpose()
    }

    // ============================================================
    // Data Migration
    // ============================================================

    pub async fn migrate_to_root_features(&self) -> Result<RootFeatureMigrationReport> {
        let mut report = RootFeatureMigrationReport::default();
        let projects = self.get_all_projects().await?;

        for project in projects {
            if project.root_feature_id.is_some() {
                report.projects_skipped += 1;
                continue;
            }

            let now = Utc::now();
            let root_feature_id = Uuid::new_v4();

            let mut tx = self.pool.begin().await?;

            // Create root feature
            sqlx::query(
                "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, created_at, updated_at)
                 VALUES ($1, $2, NULL, $3, $4, 'implemented', 0, $5, $6)",
            )
            .bind(root_feature_id.to_string())
            .bind(project.id.to_string())
            .bind(&project.name)
            .bind(&project.instructions)
            .bind(now.to_rfc3339())
            .bind(now.to_rfc3339())
            .execute(&mut *tx)
            .await?;

            // Re-parent existing root features
            let result = sqlx::query(
                "UPDATE features SET parent_id = $1 WHERE project_id = $2 AND parent_id IS NULL AND id != $1",
            )
            .bind(root_feature_id.to_string())
            .bind(project.id.to_string())
            .execute(&mut *tx)
            .await?;
            report.features_reparented += result.rows_affected() as usize;

            // Update project
            sqlx::query("UPDATE projects SET root_feature_id = $1, updated_at = $2 WHERE id = $3")
                .bind(root_feature_id.to_string())
                .bind(now.to_rfc3339())
                .bind(project.id.to_string())
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;

            report.projects_migrated += 1;
        }

        Ok(report)
    }

    // ============================================================
    // User operations
    // ============================================================

    /// Get a user by their ID.
    pub async fn get_user(&self, id: Uuid) -> Result<Option<User>> {
        let row = sqlx::query(
            "SELECT id, email, email_verified_at, display_name, avatar_url, created_at, updated_at
             FROM users WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_user).transpose()
    }

    /// Get a user by their email address.
    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<User>> {
        let row = sqlx::query(
            "SELECT id, email, email_verified_at, display_name, avatar_url, created_at, updated_at
             FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_user).transpose()
    }

    /// Get a user by their Clerk ID (via oauth_identities table).
    pub async fn get_user_by_clerk_id(&self, clerk_id: &str) -> Result<Option<User>> {
        let row = sqlx::query(
            "SELECT u.id, u.email, u.email_verified_at, u.display_name, u.avatar_url, u.created_at, u.updated_at
             FROM users u
             INNER JOIN oauth_identities o ON u.id = o.user_id
             WHERE o.provider = 'clerk' AND o.provider_user_id = $1",
        )
        .bind(clerk_id)
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_user).transpose()
    }

    /// Get a user by OAuth provider and provider user ID.
    pub async fn get_user_by_oauth_provider(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<Option<User>> {
        let row = sqlx::query(
            "SELECT u.id, u.email, u.email_verified_at, u.display_name, u.avatar_url, u.created_at, u.updated_at
             FROM users u
             INNER JOIN oauth_identities o ON u.id = o.user_id
             WHERE o.provider = $1 AND o.provider_user_id = $2",
        )
        .bind(provider)
        .bind(provider_user_id)
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_user).transpose()
    }

    /// Create a new user.
    pub async fn create_user(
        &self,
        id: Uuid,
        email: &str,
        display_name: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<User> {
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO users (id, email, display_name, avatar_url, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id.to_string())
        .bind(email)
        .bind(display_name)
        .bind(avatar_url)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(User {
            id,
            email: email.to_string(),
            email_verified_at: None,
            display_name: display_name.map(String::from),
            avatar_url: avatar_url.map(String::from),
            created_at: now,
            updated_at: now,
        })
    }

    /// Update an existing user's profile.
    pub async fn update_user(
        &self,
        id: Uuid,
        display_name: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<bool> {
        let now = Utc::now();

        let result = sqlx::query(
            "UPDATE users SET display_name = $1, avatar_url = $2, updated_at = $3 WHERE id = $4",
        )
        .bind(display_name)
        .bind(avatar_url)
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    // ============================================================
    // OAuth Identity operations
    // ============================================================

    /// Create an OAuth identity linking a provider account to a user.
    pub async fn create_oauth_identity(
        &self,
        id: Uuid,
        user_id: Uuid,
        provider: &str,
        provider_user_id: &str,
        provider_email: Option<&str>,
    ) -> Result<OAuthIdentity> {
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO oauth_identities (id, user_id, provider, provider_user_id, provider_email, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(id.to_string())
        .bind(user_id.to_string())
        .bind(provider)
        .bind(provider_user_id)
        .bind(provider_email)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(OAuthIdentity {
            id,
            user_id,
            provider: provider.to_string(),
            provider_user_id: provider_user_id.to_string(),
            provider_email: provider_email.map(String::from),
            access_token: None,
            refresh_token: None,
            token_expires_at: None,
            created_at: now,
        })
    }

    /// Get OAuth identities for a user.
    pub async fn get_oauth_identities_for_user(&self, user_id: Uuid) -> Result<Vec<OAuthIdentity>> {
        let rows = sqlx::query(
            "SELECT id, user_id, provider, provider_user_id, provider_email, access_token, refresh_token, token_expires_at, created_at
             FROM oauth_identities WHERE user_id = $1",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_oauth_identity).collect()
    }

    // ============================================================
    // Project Membership operations
    // ============================================================

    /// Get a user's membership in a project.
    pub async fn get_project_membership(
        &self,
        project_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<ProjectMembership>> {
        let row = sqlx::query(
            "SELECT id, project_id, user_id, role, invited_by, created_at
             FROM project_memberships WHERE project_id = $1 AND user_id = $2",
        )
        .bind(project_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_project_membership).transpose()
    }

    /// Get all project IDs a user can access (via membership).
    pub async fn get_user_project_ids(&self, user_id: Uuid) -> Result<Vec<Uuid>> {
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT project_id FROM project_memberships WHERE user_id = $1")
                .bind(user_id.to_string())
                .fetch_all(&self.pool)
                .await?;

        rows.into_iter().map(parse_uuid).collect()
    }

    /// Get all projects a user can access (via membership).
    pub async fn get_user_projects(&self, user_id: Uuid) -> Result<Vec<Project>> {
        let rows = sqlx::query(
            "SELECT p.id, p.slug, p.name, p.description, p.instructions, p.current_version_id, p.root_feature_id, p.default_feature_destination, p.created_at, p.updated_at
             FROM projects p
             INNER JOIN project_memberships pm ON p.id = pm.project_id
             WHERE pm.user_id = $1
             ORDER BY p.name",
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_project).collect()
    }

    /// Create a project with an owner membership.
    /// This ensures the creating user automatically becomes the owner.
    pub async fn create_project_with_owner(
        &self,
        input: CreateProjectInput,
        owner_id: Uuid,
    ) -> Result<Project> {
        let project_id = Uuid::new_v4();
        let root_feature_id = Uuid::new_v4();
        let membership_id = Uuid::new_v4();
        let now = Utc::now();

        // Generate slug from name if not provided
        let slug = input.slug.unwrap_or_else(|| slugify(&input.name));

        let mut tx = self.pool.begin().await?;

        // Create project with owner_id
        sqlx::query(
            "INSERT INTO projects (id, slug, name, description, instructions, root_feature_id, owner_id, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(project_id.to_string())
        .bind(&slug)
        .bind(&input.name)
        .bind(&input.description)
        .bind(&input.instructions)
        .bind(root_feature_id.to_string())
        .bind(owner_id.to_string())
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        // Create root feature
        sqlx::query(
            "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, created_at, updated_at)
             VALUES ($1, $2, NULL, $3, $4, 'implemented', 0, $5, $6)",
        )
        .bind(root_feature_id.to_string())
        .bind(project_id.to_string())
        .bind(&input.name)
        .bind(&input.instructions)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        // Create owner membership
        sqlx::query(
            "INSERT INTO project_memberships (id, project_id, user_id, role, created_at)
             VALUES ($1, $2, $3, 'owner', $4)",
        )
        .bind(membership_id.to_string())
        .bind(project_id.to_string())
        .bind(owner_id.to_string())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(Project {
            id: project_id,
            slug,
            name: input.name,
            description: input.description,
            instructions: input.instructions,
            current_version_id: None,
            root_feature_id: Some(root_feature_id),
            default_feature_destination: "backlog".to_string(),
            created_at: now,
            updated_at: now,
        })
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

// ============================================================
// Helper functions
// ============================================================

fn parse_uuid(s: String) -> Result<Uuid> {
    Uuid::parse_str(&s).map_err(|_| anyhow::anyhow!("Invalid UUID stored in database: {}", s))
}

fn parse_datetime(s: String) -> Result<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").map(|dt| dt.and_utc())
        })
        .map_err(|_| anyhow::anyhow!("Invalid timestamp stored in database: {}", s))
}

/// Convert a name to a URL-friendly slug.
/// - Lowercase
/// - Replace non-alphanumeric with hyphens
/// - Collapse multiple hyphens
/// - Trim leading/trailing hyphens
fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut last_was_hyphen = true; // Start true to skip leading hyphens

    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_was_hyphen = false;
        } else if !last_was_hyphen {
            slug.push('-');
            last_was_hyphen = true;
        }
    }

    // Trim trailing hyphen
    if slug.ends_with('-') {
        slug.pop();
    }

    slug
}

/// Validate that a version name is a semantic version: `MAJOR.MINOR.PATCH` with optional `v` prefix.
/// Examples: `0.1.0`, `v1.0.0`, `v2.3.1`.
fn is_valid_semver(name: &str) -> bool {
    let name = name.strip_prefix('v').unwrap_or(name);
    let parts: Vec<&str> = name.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.parse::<u32>().is_ok())
}

/// Compute the next version name by parsing existing versions and incrementing the minor version.
/// Falls back to "0.1.0" if no parseable versions exist.
fn compute_next_version_name(versions: &[Version]) -> String {
    struct SemVer {
        major: u32,
        minor: u32,
    }

    let parsed: Vec<SemVer> = versions
        .iter()
        .filter_map(|v| {
            // Match "0.1.0" or "v0.1.0" style names
            let name = v.name.strip_prefix('v').unwrap_or(&v.name);
            let parts: Vec<&str> = name.split('.').collect();
            if parts.len() >= 2 {
                let major = parts[0].parse::<u32>().ok()?;
                let minor = parts[1].parse::<u32>().ok()?;
                Some(SemVer { major, minor })
            } else {
                None
            }
        })
        .collect();

    if parsed.is_empty() {
        return "0.1.0".to_string();
    }

    let highest = parsed.iter().max_by_key(|v| (v.major, v.minor)).unwrap();
    format!("{}.{}.0", highest.major, highest.minor + 1)
}

fn row_to_project(row: &AnyRow) -> Result<Project> {
    Ok(Project {
        id: parse_uuid(row.get("id"))?,
        slug: row.get("slug"),
        name: row.get("name"),
        description: row.get("description"),
        instructions: row.get("instructions"),
        current_version_id: row
            .get::<Option<String>, _>("current_version_id")
            .map(parse_uuid)
            .transpose()?,
        root_feature_id: row
            .get::<Option<String>, _>("root_feature_id")
            .map(parse_uuid)
            .transpose()?,
        default_feature_destination: row
            .get::<Option<String>, _>("default_feature_destination")
            .unwrap_or_else(|| "backlog".to_string()),
        created_at: parse_datetime(row.get("created_at"))?,
        updated_at: parse_datetime(row.get("updated_at"))?,
    })
}

fn row_to_project_directory(row: &AnyRow) -> Result<ProjectDirectory> {
    Ok(ProjectDirectory {
        id: parse_uuid(row.get("id"))?,
        project_id: parse_uuid(row.get("project_id"))?,
        path: row.get("path"),
        git_remote: row.get("git_remote"),
        is_primary: row.get::<i32, _>("is_primary") != 0,
        instructions: row.get("instructions"),
        created_at: parse_datetime(row.get("created_at"))?,
    })
}

fn row_to_version(row: &AnyRow) -> Result<Version> {
    Ok(Version {
        id: parse_uuid(row.get("id"))?,
        project_id: parse_uuid(row.get("project_id"))?,
        name: row.get("name"),
        description: row.get("description"),
        released_at: row
            .get::<Option<String>, _>("released_at")
            .map(parse_datetime)
            .transpose()?,
        created_at: parse_datetime(row.get("created_at"))?,
        updated_at: parse_datetime(row.get("updated_at"))?,
    })
}

fn row_to_feature(row: &AnyRow) -> Result<Feature> {
    Ok(Feature {
        id: parse_uuid(row.get("id"))?,
        project_id: parse_uuid(row.get("project_id"))?,
        parent_id: row
            .get::<Option<String>, _>("parent_id")
            .map(parse_uuid)
            .transpose()?,
        title: row.get("title"),
        details: row.get("details"),
        desired_details: row.get("desired_details"),
        state: FeatureState::from_str(&row.get::<String, _>("state"))
            .unwrap_or(FeatureState::Proposed),
        priority: row.get("priority"),
        target_version_id: row
            .get::<Option<String>, _>("target_version_id")
            .map(parse_uuid)
            .transpose()?,
        created_at: parse_datetime(row.get("created_at"))?,
        updated_at: parse_datetime(row.get("updated_at"))?,
    })
}

fn row_to_feature_summary(row: &AnyRow) -> Result<FeatureSummary> {
    Ok(FeatureSummary {
        id: parse_uuid(row.get("id"))?,
        project_id: parse_uuid(row.get("project_id"))?,
        parent_id: row
            .get::<Option<String>, _>("parent_id")
            .map(parse_uuid)
            .transpose()?,
        title: row.get("title"),
        state: FeatureState::from_str(&row.get::<String, _>("state"))
            .unwrap_or(FeatureState::Proposed),
        priority: row.get("priority"),
        target_version_id: row
            .get::<Option<String>, _>("target_version_id")
            .map(parse_uuid)
            .transpose()?,
    })
}

fn row_to_feature_summary_context(row: &AnyRow) -> Result<FeatureSummaryContext> {
    Ok(FeatureSummaryContext {
        id: parse_uuid(row.get("id"))?,
        title: row.get("title"),
        state: FeatureState::from_str(&row.get::<String, _>("state"))
            .unwrap_or(FeatureState::Proposed),
    })
}

fn row_to_feature_history(row: &AnyRow) -> Result<FeatureHistory> {
    let details_json: String = row.get("details");
    let details: HistoryDetails = serde_json::from_str(&details_json).unwrap_or_default();

    Ok(FeatureHistory {
        id: parse_uuid(row.get("id"))?,
        feature_id: parse_uuid(row.get("feature_id"))?,
        version_id: row
            .get::<Option<String>, _>("version_id")
            .map(parse_uuid)
            .transpose()?,
        details,
        created_at: parse_datetime(row.get("created_at"))?,
    })
}

fn row_to_project_history_entry(row: &AnyRow) -> Result<ProjectHistoryEntry> {
    let details_json: String = row.get("details");
    let details: HistoryDetails = serde_json::from_str(&details_json).unwrap_or_default();

    Ok(ProjectHistoryEntry {
        id: parse_uuid(row.get::<String, _>("id"))?,
        feature_id: parse_uuid(row.get::<String, _>("feature_id"))?,
        feature_title: row.get("title"),
        feature_state: FeatureState::from_str(&row.get::<String, _>("state"))
            .unwrap_or(FeatureState::Proposed),
        version_id: row
            .get::<Option<String>, _>("version_id")
            .map(parse_uuid)
            .transpose()?,
        version_name: row.get("name"),
        summary: details.summary,
        commits: details.commits,
        created_at: parse_datetime(row.get::<String, _>("created_at"))?,
    })
}

fn row_to_user(row: &AnyRow) -> Result<User> {
    Ok(User {
        id: parse_uuid(row.get("id"))?,
        email: row.get("email"),
        email_verified_at: row
            .get::<Option<String>, _>("email_verified_at")
            .map(parse_datetime)
            .transpose()?,
        display_name: row.get("display_name"),
        avatar_url: row.get("avatar_url"),
        created_at: parse_datetime(row.get("created_at"))?,
        updated_at: parse_datetime(row.get("updated_at"))?,
    })
}

fn row_to_oauth_identity(row: &AnyRow) -> Result<OAuthIdentity> {
    Ok(OAuthIdentity {
        id: parse_uuid(row.get("id"))?,
        user_id: parse_uuid(row.get("user_id"))?,
        provider: row.get("provider"),
        provider_user_id: row.get("provider_user_id"),
        provider_email: row.get("provider_email"),
        access_token: row.get("access_token"),
        refresh_token: row.get("refresh_token"),
        token_expires_at: row
            .get::<Option<String>, _>("token_expires_at")
            .map(parse_datetime)
            .transpose()?,
        created_at: parse_datetime(row.get("created_at"))?,
    })
}

fn row_to_project_membership(row: &AnyRow) -> Result<ProjectMembership> {
    Ok(ProjectMembership {
        id: parse_uuid(row.get("id"))?,
        project_id: parse_uuid(row.get("project_id"))?,
        user_id: parse_uuid(row.get("user_id"))?,
        role: MembershipRole::from_str(&row.get::<String, _>("role"))
            .unwrap_or(MembershipRole::Viewer),
        invited_by: row
            .get::<Option<String>, _>("invited_by")
            .map(parse_uuid)
            .transpose()?,
        created_at: parse_datetime(row.get("created_at"))?,
    })
}
