mod schema;

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use uuid::Uuid;

use crate::models::*;

/// Domain errors that can be meaningfully handled by callers.
/// These are distinct from infrastructure errors (SQLite failures, etc.)
/// which propagate as `anyhow::Error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// Resource does not exist (project, feature, session, task)
    NotFound(String),
    /// Input validation failed (e.g., sessions only on leaf features)
    Validation(String),
    /// Operation not allowed in current state (e.g., session not active)
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

    /// Returns true if this is a client error (4xx), false if server error (5xx)
    pub fn is_client_error(&self) -> bool {
        true // All ManifestError variants are client errors
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::NotFound(msg) => write!(f, "{}", msg),
            ManifestError::Validation(msg) => write!(f, "{}", msg),
            ManifestError::InvalidState(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for ManifestError {}

pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(path: PathBuf) -> Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Database path has no parent directory"))?;
        std::fs::create_dir_all(parent)?;
        let conn = Connection::open(&path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Explicitly disable foreign key enforcement (SQLite default) to avoid
        // FK constraint errors when updating features with version references.
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn open_default() -> Result<Self> {
        // Check for custom data directory from environment
        let db_path = if let Ok(data_dir) = std::env::var("MANIFEST_DATA_DIR") {
            PathBuf::from(data_dir).join("manifest.db")
        } else {
            let dirs = directories::ProjectDirs::from("", "", "manifest")
                .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?;
            dirs.data_dir().join("manifest.db")
        };
        Self::open(db_path)
    }

    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().expect("database lock poisoned");
        schema::run_migrations(&conn)
    }

    // ============================================================
    // Project operations
    // ============================================================

    pub fn get_all_projects(&self) -> Result<Vec<Project>> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, name, description, instructions, current_version_id, created_at, updated_at
             FROM projects ORDER BY name",
        )?;

        let projects = stmt
            .query_map([], |row| {
                Ok(Project {
                    id: parse_uuid(row.get::<_, String>(0)?),
                    name: row.get(1)?,
                    description: row.get(2)?,
                    instructions: row.get(3)?,
                    current_version_id: row.get::<_, Option<String>>(4)?.map(parse_uuid),
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                    updated_at: parse_datetime(row.get::<_, String>(6)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(projects)
    }

    pub fn get_project(&self, id: Uuid) -> Result<Option<Project>> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, name, description, instructions, current_version_id, created_at, updated_at
             FROM projects WHERE id = ?",
        )?;

        let mut rows = stmt.query([id.to_string()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Project {
                id: parse_uuid(row.get::<_, String>(0)?),
                name: row.get(1)?,
                description: row.get(2)?,
                instructions: row.get(3)?,
                current_version_id: row.get::<_, Option<String>>(4)?.map(parse_uuid),
                created_at: parse_datetime(row.get::<_, String>(5)?),
                updated_at: parse_datetime(row.get::<_, String>(6)?),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn create_project(&self, input: CreateProjectInput) -> Result<Project> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let id = Uuid::new_v4();
        let now = Utc::now();

        conn.execute(
            "INSERT INTO projects (id, name, description, instructions, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            (
                id.to_string(),
                &input.name,
                &input.description,
                &input.instructions,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ),
        )?;

        Ok(Project {
            id,
            name: input.name,
            description: input.description,
            instructions: input.instructions,
            current_version_id: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update_project(&self, id: Uuid, input: UpdateProjectInput) -> Result<Option<Project>> {
        let Some(existing) = self.get_project(id)? else {
            return Ok(None);
        };

        let conn = self.conn.lock().expect("database lock poisoned");
        let now = Utc::now();
        let name = input.name.unwrap_or(existing.name);
        let description = input.description.or(existing.description);
        let instructions = input.instructions.or(existing.instructions);
        let current_version_id = input.current_version_id.or(existing.current_version_id);

        conn.execute(
            "UPDATE projects SET name = ?, description = ?, instructions = ?, current_version_id = ?, updated_at = ? WHERE id = ?",
            (
                &name,
                &description,
                &instructions,
                current_version_id.map(|u| u.to_string()),
                now.to_rfc3339(),
                id.to_string(),
            ),
        )?;

        Ok(Some(Project {
            id,
            name,
            description,
            instructions,
            current_version_id,
            created_at: existing.created_at,
            updated_at: now,
        }))
    }

    pub fn delete_project(&self, id: Uuid) -> Result<bool> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let rows = conn.execute("DELETE FROM projects WHERE id = ?", [id.to_string()])?;
        Ok(rows > 0)
    }

    // ============================================================
    // Project Directory operations
    // ============================================================

    pub fn get_project_directories(&self, project_id: Uuid) -> Result<Vec<ProjectDirectory>> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, project_id, path, git_remote, is_primary, instructions, created_at
             FROM project_directories WHERE project_id = ? ORDER BY is_primary DESC, path",
        )?;

        let dirs = stmt
            .query_map([project_id.to_string()], |row| {
                Ok(ProjectDirectory {
                    id: parse_uuid(row.get::<_, String>(0)?),
                    project_id: parse_uuid(row.get::<_, String>(1)?),
                    path: row.get(2)?,
                    git_remote: row.get(3)?,
                    is_primary: row.get::<_, i32>(4)? != 0,
                    instructions: row.get(5)?,
                    created_at: parse_datetime(row.get::<_, String>(6)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(dirs)
    }

    pub fn add_project_directory(
        &self,
        project_id: Uuid,
        input: AddDirectoryInput,
    ) -> Result<ProjectDirectory> {
        self.get_project(project_id)?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        let conn = self.conn.lock().expect("database lock poisoned");
        let id = Uuid::new_v4();
        let now = Utc::now();

        conn.execute(
            "INSERT INTO project_directories (id, project_id, path, git_remote, is_primary, instructions, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            (
                id.to_string(),
                project_id.to_string(),
                &input.path,
                &input.git_remote,
                if input.is_primary { 1 } else { 0 },
                &input.instructions,
                now.to_rfc3339(),
            ),
        )?;

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

    pub fn remove_project_directory(&self, id: Uuid) -> Result<bool> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let rows = conn.execute(
            "DELETE FROM project_directories WHERE id = ?",
            [id.to_string()],
        )?;
        Ok(rows > 0)
    }

    pub fn get_project_with_directories(&self, id: Uuid) -> Result<Option<ProjectWithDirectories>> {
        let project = match self.get_project(id)? {
            Some(p) => p,
            None => return Ok(None),
        };

        let directories = self.get_project_directories(id)?;

        Ok(Some(ProjectWithDirectories {
            project,
            directories,
        }))
    }

    /// Find a project by a directory path.
    ///
    /// Returns the project and matching directory if the path matches exactly,
    /// or if the path is a subdirectory of a registered project directory.
    pub fn get_project_by_directory(&self, path: &str) -> Result<Option<ProjectWithDirectories>> {
        let conn = self.conn.lock().expect("database lock poisoned");

        // Get all directories ordered by path length (longest first for best match)
        let mut stmt = conn.prepare(
            "SELECT project_id, path FROM project_directories ORDER BY length(path) DESC",
        )?;

        let mut rows = stmt.query([])?;
        let mut found_project_id = None;

        while let Some(row) = rows.next()? {
            let dir_path: String = row.get(1)?;
            // Check exact match or subdirectory match
            if path == dir_path || path.starts_with(&format!("{}/", dir_path)) {
                found_project_id = Some(row.get::<_, String>(0)?);
                break;
            }
        }

        drop(rows);
        drop(stmt);
        drop(conn);

        match found_project_id {
            Some(id) => self.get_project_with_directories(parse_uuid(id)),
            None => Ok(None),
        }
    }

    // ============================================================
    // Version operations
    // ============================================================

    /// Get all versions for a project.
    pub fn get_versions_by_project(&self, project_id: Uuid) -> Result<Vec<Version>> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, project_id, name, description, released_at, created_at, updated_at
             FROM versions WHERE project_id = ? ORDER BY created_at",
        )?;

        let versions = stmt
            .query_map([project_id.to_string()], |row| {
                Ok(Version {
                    id: parse_uuid(row.get::<_, String>(0)?),
                    project_id: parse_uuid(row.get::<_, String>(1)?),
                    name: row.get(2)?,
                    description: row.get(3)?,
                    released_at: row.get::<_, Option<String>>(4)?.map(parse_datetime),
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                    updated_at: parse_datetime(row.get::<_, String>(6)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(versions)
    }

    /// Get a version by ID.
    pub fn get_version(&self, id: Uuid) -> Result<Option<Version>> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, project_id, name, description, released_at, created_at, updated_at
             FROM versions WHERE id = ?",
        )?;

        let mut rows = stmt.query([id.to_string()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Version {
                id: parse_uuid(row.get::<_, String>(0)?),
                project_id: parse_uuid(row.get::<_, String>(1)?),
                name: row.get(2)?,
                description: row.get(3)?,
                released_at: row.get::<_, Option<String>>(4)?.map(parse_datetime),
                created_at: parse_datetime(row.get::<_, String>(5)?),
                updated_at: parse_datetime(row.get::<_, String>(6)?),
            }))
        } else {
            Ok(None)
        }
    }

    /// Create a new version.
    pub fn create_version(&self, project_id: Uuid, input: CreateVersionInput) -> Result<Version> {
        // Verify project exists
        self.get_project(project_id)?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        let conn = self.conn.lock().expect("database lock poisoned");
        let id = Uuid::new_v4();
        let now = Utc::now();

        conn.execute(
            "INSERT INTO versions (id, project_id, name, description, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            (
                id.to_string(),
                project_id.to_string(),
                &input.name,
                &input.description,
                now.to_rfc3339(),
                now.to_rfc3339(),
            ),
        )?;

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

    /// Update an existing version.
    pub fn update_version(&self, id: Uuid, input: UpdateVersionInput) -> Result<Option<Version>> {
        let Some(existing) = self.get_version(id)? else {
            return Ok(None);
        };

        let conn = self.conn.lock().expect("database lock poisoned");
        let now = Utc::now();
        let name = input.name.unwrap_or(existing.name);
        let description = input.description.or(existing.description);
        let released_at = input.released_at.or(existing.released_at);

        conn.execute(
            "UPDATE versions SET name = ?, description = ?, released_at = ?, updated_at = ? WHERE id = ?",
            (
                &name,
                &description,
                released_at.map(|d| d.to_rfc3339()),
                now.to_rfc3339(),
                id.to_string(),
            ),
        )?;

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

    /// Delete a version.
    pub fn delete_version(&self, id: Uuid) -> Result<bool> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let rows = conn.execute("DELETE FROM versions WHERE id = ?", [id.to_string()])?;
        Ok(rows > 0)
    }

    // ============================================================
    // Feature operations
    // ============================================================

    /// Get all features with optional SQL-based pagination.
    pub fn get_all_features_paginated(
        &self,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Feature>> {
        let conn = self.conn.lock().expect("database lock poisoned");

        let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = match (limit, offset) {
            (Some(lim), Some(off)) => (
                "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                 FROM features ORDER BY priority, title LIMIT ? OFFSET ?".to_string(),
                vec![Box::new(lim) as Box<dyn rusqlite::ToSql>, Box::new(off)],
            ),
            (Some(lim), None) => (
                "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                 FROM features ORDER BY priority, title LIMIT ?".to_string(),
                vec![Box::new(lim) as Box<dyn rusqlite::ToSql>],
            ),
            (None, Some(off)) => (
                "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                 FROM features ORDER BY priority, title LIMIT -1 OFFSET ?".to_string(),
                vec![Box::new(off) as Box<dyn rusqlite::ToSql>],
            ),
            (None, None) => (
                "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                 FROM features ORDER BY priority, title".to_string(),
                vec![],
            ),
        };

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let features = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(Feature {
                    id: parse_uuid(row.get::<_, String>(0)?),
                    project_id: parse_uuid(row.get::<_, String>(1)?),
                    parent_id: row.get::<_, Option<String>>(2)?.map(parse_uuid),
                    title: row.get(3)?,
                    details: row.get(4)?,
                    desired_details: row.get(5)?,
                    state: FeatureState::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or(FeatureState::Proposed),
                    priority: row.get(7)?,
                    target_version_id: row.get::<_, Option<String>>(8)?.map(parse_uuid),
                    created_at: parse_datetime(row.get::<_, String>(9)?),
                    updated_at: parse_datetime(row.get::<_, String>(10)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(features)
    }

    /// Get all features (unpaginated, for backwards compatibility).
    pub fn get_all_features(&self) -> Result<Vec<Feature>> {
        self.get_all_features_paginated(None, None)
    }

    /// Get features by project with optional SQL-based pagination.
    pub fn get_features_by_project_paginated(
        &self,
        project_id: Uuid,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Feature>> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let project_id_str = project_id.to_string();

        let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = match (limit, offset) {
            (Some(lim), Some(off)) => (
                "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                 FROM features WHERE project_id = ? ORDER BY priority, title LIMIT ? OFFSET ?".to_string(),
                vec![
                    Box::new(project_id_str.clone()) as Box<dyn rusqlite::ToSql>,
                    Box::new(lim),
                    Box::new(off),
                ],
            ),
            (Some(lim), None) => (
                "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                 FROM features WHERE project_id = ? ORDER BY priority, title LIMIT ?".to_string(),
                vec![
                    Box::new(project_id_str.clone()) as Box<dyn rusqlite::ToSql>,
                    Box::new(lim),
                ],
            ),
            (None, Some(off)) => (
                "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                 FROM features WHERE project_id = ? ORDER BY priority, title LIMIT -1 OFFSET ?".to_string(),
                vec![
                    Box::new(project_id_str.clone()) as Box<dyn rusqlite::ToSql>,
                    Box::new(off),
                ],
            ),
            (None, None) => (
                "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
                 FROM features WHERE project_id = ? ORDER BY priority, title".to_string(),
                vec![Box::new(project_id_str.clone()) as Box<dyn rusqlite::ToSql>],
            ),
        };

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let features = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(Feature {
                    id: parse_uuid(row.get::<_, String>(0)?),
                    project_id: parse_uuid(row.get::<_, String>(1)?),
                    parent_id: row.get::<_, Option<String>>(2)?.map(parse_uuid),
                    title: row.get(3)?,
                    details: row.get(4)?,
                    desired_details: row.get(5)?,
                    state: FeatureState::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or(FeatureState::Proposed),
                    priority: row.get(7)?,
                    target_version_id: row.get::<_, Option<String>>(8)?.map(parse_uuid),
                    created_at: parse_datetime(row.get::<_, String>(9)?),
                    updated_at: parse_datetime(row.get::<_, String>(10)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(features)
    }

    /// Get features by project (unpaginated, for backwards compatibility).
    pub fn get_features_by_project(&self, project_id: Uuid) -> Result<Vec<Feature>> {
        self.get_features_by_project_paginated(project_id, None, None)
    }

    pub fn get_feature(&self, id: Uuid) -> Result<Option<Feature>> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
             FROM features WHERE id = ?",
        )?;

        let mut rows = stmt.query([id.to_string()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Feature {
                id: parse_uuid(row.get::<_, String>(0)?),
                project_id: parse_uuid(row.get::<_, String>(1)?),
                parent_id: row.get::<_, Option<String>>(2)?.map(parse_uuid),
                title: row.get(3)?,
                details: row.get(4)?,
                desired_details: row.get(5)?,
                state: FeatureState::from_str(&row.get::<_, String>(6)?)
                    .unwrap_or(FeatureState::Proposed),
                priority: row.get(7)?,
                target_version_id: row.get::<_, Option<String>>(8)?.map(parse_uuid),
                created_at: parse_datetime(row.get::<_, String>(9)?),
                updated_at: parse_datetime(row.get::<_, String>(10)?),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the diff between current and desired details for a feature.
    pub fn get_feature_diff(&self, id: Uuid) -> Result<Option<FeatureDiff>> {
        let feature = match self.get_feature(id)? {
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

    pub fn create_feature(&self, project_id: Uuid, input: CreateFeatureInput) -> Result<Feature> {
        // Verify project exists
        self.get_project(project_id)?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        let conn = self.conn.lock().expect("database lock poisoned");
        let id = input.id.unwrap_or_else(Uuid::new_v4);
        let now = Utc::now();
        let state = input.state.unwrap_or(FeatureState::Proposed);
        let priority = input.priority.unwrap_or(0);

        conn.execute(
            "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, target_version_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (
                id.to_string(),
                project_id.to_string(),
                input.parent_id.map(|u| u.to_string()),
                &input.title,
                &input.details,
                state.as_str(),
                priority,
                input.target_version_id.map(|u| u.to_string()),
                now.to_rfc3339(),
                now.to_rfc3339(),
            ),
        )?;

        Ok(Feature {
            id,
            project_id,
            parent_id: input.parent_id,
            title: input.title,
            details: input.details,
            desired_details: None,
            state,
            priority,
            target_version_id: input.target_version_id,
            created_at: now,
            updated_at: now,
        })
    }

    /// Create multiple features in a single transaction.
    /// All features are created atomically - if any fails, all are rolled back.
    pub fn create_features_bulk(
        &self,
        project_id: Uuid,
        inputs: Vec<CreateFeatureInput>,
    ) -> Result<Vec<Feature>> {
        // Verify project exists
        self.get_project(project_id)?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        let mut conn = self.conn.lock().expect("database lock poisoned");
        let tx = conn.transaction()?;
        let now = Utc::now();

        let mut features = Vec::with_capacity(inputs.len());

        for input in inputs {
            let id = input.id.unwrap_or_else(Uuid::new_v4);
            let state = input.state.unwrap_or(FeatureState::Proposed);
            let priority = input.priority.unwrap_or(0);

            tx.execute(
                "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, target_version_id, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    id.to_string(),
                    project_id.to_string(),
                    input.parent_id.map(|u| u.to_string()),
                    &input.title,
                    &input.details,
                    state.as_str(),
                    priority,
                    input.target_version_id.map(|u| u.to_string()),
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ),
            )?;

            features.push(Feature {
                id,
                project_id,
                parent_id: input.parent_id,
                title: input.title,
                details: input.details,
                desired_details: None,
                state,
                priority,
                target_version_id: input.target_version_id,
                created_at: now,
                updated_at: now,
            });
        }

        tx.commit()?;
        Ok(features)
    }

    pub fn update_feature(&self, id: Uuid, input: UpdateFeatureInput) -> Result<Option<Feature>> {
        let Some(existing) = self.get_feature(id)? else {
            return Ok(None);
        };

        let conn = self.conn.lock().expect("database lock poisoned");
        let now = Utc::now();
        let title = input.title.unwrap_or(existing.title);
        let details = input.details.or(existing.details);
        let desired_details = input.desired_details.or(existing.desired_details);
        let state = input.state.unwrap_or(existing.state);
        let parent_id = input.parent_id.or(existing.parent_id);
        let priority = input.priority.unwrap_or(existing.priority);
        // Double Option: None = don't update (keep existing), Some(x) = set to x (including Some(None) to clear)
        let target_version_id = input
            .target_version_id
            .unwrap_or(existing.target_version_id);

        conn.execute(
            "UPDATE features SET parent_id = ?, title = ?, details = ?, desired_details = ?, state = ?, priority = ?, target_version_id = ?, updated_at = ? WHERE id = ?",
            (
                parent_id.map(|u| u.to_string()),
                &title,
                &details,
                &desired_details,
                state.as_str(),
                priority,
                target_version_id.map(|u| u.to_string()),
                now.to_rfc3339(),
                id.to_string(),
            ),
        )?;

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

    pub fn delete_feature(&self, id: Uuid) -> Result<bool> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let rows = conn.execute("DELETE FROM features WHERE id = ?", [id.to_string()])?;
        Ok(rows > 0)
    }

    pub fn get_root_features(&self, project_id: Uuid) -> Result<Vec<Feature>> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
             FROM features WHERE project_id = ? AND parent_id IS NULL ORDER BY priority, title",
        )?;

        let features = stmt
            .query_map([project_id.to_string()], |row| {
                Ok(Feature {
                    id: parse_uuid(row.get::<_, String>(0)?),
                    project_id: parse_uuid(row.get::<_, String>(1)?),
                    parent_id: None,
                    title: row.get(3)?,
                    details: row.get(4)?,
                    desired_details: row.get(5)?,
                    state: FeatureState::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or(FeatureState::Proposed),
                    priority: row.get(7)?,
                    target_version_id: row.get::<_, Option<String>>(8)?.map(parse_uuid),
                    created_at: parse_datetime(row.get::<_, String>(9)?),
                    updated_at: parse_datetime(row.get::<_, String>(10)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(features)
    }

    pub fn get_children(&self, parent_id: Uuid) -> Result<Vec<Feature>> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
             FROM features WHERE parent_id = ? ORDER BY priority, title",
        )?;

        let features = stmt
            .query_map([parent_id.to_string()], |row| {
                Ok(Feature {
                    id: parse_uuid(row.get::<_, String>(0)?),
                    project_id: parse_uuid(row.get::<_, String>(1)?),
                    parent_id: row.get::<_, Option<String>>(2)?.map(parse_uuid),
                    title: row.get(3)?,
                    details: row.get(4)?,
                    desired_details: row.get(5)?,
                    state: FeatureState::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or(FeatureState::Proposed),
                    priority: row.get(7)?,
                    target_version_id: row.get::<_, Option<String>>(8)?.map(parse_uuid),
                    created_at: parse_datetime(row.get::<_, String>(9)?),
                    updated_at: parse_datetime(row.get::<_, String>(10)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(features)
    }

    pub fn is_leaf(&self, feature_id: Uuid) -> Result<bool> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let count: i32 = conn.query_row(
            "SELECT COUNT(*) FROM features WHERE parent_id = ?",
            [feature_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(count == 0)
    }

    /// Search features by title and details.
    /// Returns summaries ranked by relevance (title matches first, then details matches).
    pub fn search_features(
        &self,
        query: &str,
        project_id: Option<Uuid>,
        limit: Option<u32>,
    ) -> Result<Vec<FeatureSummary>> {
        let conn = self.conn.lock().expect("database lock poisoned");

        // Use LIKE for case-insensitive search
        // Ranking: title matches get higher priority than details matches
        let search_pattern = format!("%{}%", query);
        let limit_val = limit.unwrap_or(10) as i64;

        let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = match project_id {
            Some(pid) => (
                "SELECT id, project_id, parent_id, title, state, priority, target_version_id
                 FROM features
                 WHERE project_id = ?1 AND (title LIKE ?2 OR details LIKE ?2)
                 ORDER BY
                     CASE WHEN title LIKE ?2 THEN 0 ELSE 1 END,
                     priority,
                     title
                 LIMIT ?3"
                    .to_string(),
                vec![
                    Box::new(pid.to_string()),
                    Box::new(search_pattern),
                    Box::new(limit_val),
                ],
            ),
            None => (
                "SELECT id, project_id, parent_id, title, state, priority, target_version_id
                 FROM features
                 WHERE title LIKE ?1 OR details LIKE ?1
                 ORDER BY
                     CASE WHEN title LIKE ?1 THEN 0 ELSE 1 END,
                     priority,
                     title
                 LIMIT ?2"
                    .to_string(),
                vec![Box::new(search_pattern), Box::new(limit_val)],
            ),
        };

        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;

        let features = stmt
            .query_map(params_ref.as_slice(), |row| {
                Ok(FeatureSummary {
                    id: parse_uuid(row.get::<_, String>(0)?),
                    project_id: parse_uuid(row.get::<_, String>(1)?),
                    parent_id: row.get::<_, Option<String>>(2)?.map(parse_uuid),
                    title: row.get(3)?,
                    state: FeatureState::from_str(&row.get::<_, String>(4)?)
                        .unwrap_or(FeatureState::Proposed),
                    priority: row.get(5)?,
                    target_version_id: row.get::<_, Option<String>>(6)?.map(parse_uuid),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(features)
    }

    pub fn get_feature_tree(&self, project_id: Uuid) -> Result<Vec<FeatureTreeNode>> {
        let features = self.get_features_by_project(project_id)?;

        // Group features by parent_id
        let mut children_map: std::collections::HashMap<Option<Uuid>, Vec<Feature>> =
            std::collections::HashMap::new();
        for feature in features {
            children_map
                .entry(feature.parent_id)
                .or_default()
                .push(feature);
        }

        // Recursively build tree starting from roots (parent_id = None)
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
                        })
                        .collect()
                })
                .unwrap_or_default()
        }

        Ok(build_subtree(None, &children_map))
    }

    // ============================================================
    // Feature History operations
    // ============================================================

    pub fn create_history_entry(&self, input: CreateHistoryInput) -> Result<FeatureHistory> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let id = Uuid::new_v4();
        let now = Utc::now();

        // If version_id not provided, use feature's target_version_id
        let version_id = match input.version_id {
            Some(vid) => Some(vid),
            None => {
                // Look up feature's target_version_id
                let mut stmt =
                    conn.prepare("SELECT target_version_id FROM features WHERE id = ?")?;
                stmt.query_row([input.feature_id.to_string()], |row| {
                    row.get::<_, Option<String>>(0)
                })
                .ok()
                .flatten()
                .map(parse_uuid)
            }
        };

        let details_json = serde_json::to_string(&input.details)?;

        conn.execute(
            "INSERT INTO feature_history (id, feature_id, version_id, summary, details, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
            (
                id.to_string(),
                input.feature_id.to_string(),
                version_id.map(|u| u.to_string()),
                &input.details.summary,
                &details_json,
                now.to_rfc3339(),
            ),
        )?;

        Ok(FeatureHistory {
            id,
            feature_id: input.feature_id,
            version_id,
            details: input.details,
            created_at: now,
        })
    }

    pub fn get_feature_history(&self, feature_id: Uuid) -> Result<Vec<FeatureHistory>> {
        let conn = self.conn.lock().expect("database lock poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, feature_id, version_id, details, created_at
             FROM feature_history WHERE feature_id = ? ORDER BY created_at DESC",
        )?;

        let entries = stmt
            .query_map([feature_id.to_string()], |row| {
                let details_json: String = row.get(3)?;
                let details: HistoryDetails =
                    serde_json::from_str(&details_json).unwrap_or_default();

                Ok(FeatureHistory {
                    id: parse_uuid(row.get::<_, String>(0)?),
                    feature_id: parse_uuid(row.get::<_, String>(1)?),
                    version_id: row.get::<_, Option<String>>(2)?.map(parse_uuid),
                    details,
                    created_at: parse_datetime(row.get::<_, String>(4)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entries)
    }

    /// Get project-wide history with feature context.
    ///
    /// Returns history entries across all features in a project, ordered by
    /// creation date (newest first). Each entry includes the feature title,
    /// state, and version info for display without additional lookups.
    ///
    /// Supports optional filtering by `version_id` (for release notes),
    /// `since` datetime, and pagination via `limit` and `offset`.
    pub fn get_project_history(
        &self,
        project_id: Uuid,
        version_id: Option<Uuid>,
        limit: Option<u32>,
        offset: Option<u32>,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<ProjectHistoryEntry>> {
        let conn = self.conn.lock().expect("database lock poisoned");

        // Build query with optional filters
        // Join with versions to get version_name
        let base_query = r#"
            SELECT fh.id, fh.feature_id, f.title, f.state, fh.version_id, v.name, fh.details, fh.created_at
            FROM feature_history fh
            INNER JOIN features f ON f.id = fh.feature_id
            LEFT JOIN versions v ON v.id = fh.version_id
            WHERE f.project_id = ?1
        "#;

        let limit_val = limit.unwrap_or(50) as i64;
        let offset_val = offset.unwrap_or(0) as i64;

        // Build dynamic SQL with optional filters
        let mut conditions = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(project_id.to_string())];
        let mut param_idx = 2;

        if let Some(vid) = version_id {
            conditions.push(format!("fh.version_id = ?{}", param_idx));
            params.push(Box::new(vid.to_string()));
            param_idx += 1;
        }

        if let Some(since_dt) = since {
            conditions.push(format!("fh.created_at > ?{}", param_idx));
            params.push(Box::new(since_dt.to_rfc3339()));
            param_idx += 1;
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" AND {}", conditions.join(" AND "))
        };

        let sql = format!(
            "{}{} ORDER BY fh.created_at DESC LIMIT ?{} OFFSET ?{}",
            base_query,
            where_clause,
            param_idx,
            param_idx + 1
        );
        params.push(Box::new(limit_val));
        params.push(Box::new(offset_val));

        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

        let entries = stmt
            .query_map(params_refs.as_slice(), |row| {
                let details_json: String = row.get(6)?;
                let details: HistoryDetails =
                    serde_json::from_str(&details_json).unwrap_or_default();

                Ok(ProjectHistoryEntry {
                    id: parse_uuid(row.get::<_, String>(0)?),
                    feature_id: parse_uuid(row.get::<_, String>(1)?),
                    feature_title: row.get(2)?,
                    feature_state: FeatureState::from_str(&row.get::<_, String>(3)?)
                        .unwrap_or(FeatureState::Proposed),
                    version_id: row.get::<_, Option<String>>(4)?.map(parse_uuid),
                    version_name: row.get(5)?,
                    summary: details.summary,
                    commits: details.commits,
                    created_at: parse_datetime(row.get::<_, String>(7)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(entries)
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            conn: self.conn.clone(),
        }
    }
}

fn parse_uuid(s: String) -> Uuid {
    Uuid::parse_str(&s).unwrap_or_else(|_| panic!("Invalid UUID stored in database: {}", s))
}

fn parse_datetime(s: String) -> DateTime<Utc> {
    // Try RFC3339 first (e.g., 2026-01-11T18:51:25Z)
    chrono::DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            // Fall back to SQLite format (e.g., 2026-01-11 18:51:25)
            chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").map(|dt| dt.and_utc())
        })
        .unwrap_or_else(|_| panic!("Invalid timestamp stored in database: {}", s))
}
