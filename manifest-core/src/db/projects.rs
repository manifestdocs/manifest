use anyhow::Result;
use chrono::Utc;

use super::helpers::*;
use super::{Database, ManifestError};
use crate::models::*;

/// SELECT columns for the projects table.
const PROJECT_COLS: &str = "id, slug, name, description, instructions, current_version_id, root_feature_id, default_feature_destination, test_adapter, context_budget, key_prefix, created_at, updated_at";

impl Database {
    /// Get all projects ordered by name.
    pub async fn get_all_projects(&self) -> Result<Vec<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects ORDER BY name");
        let mut rows = self
            .conn
            .query(&sql, ())
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            results.push(row_to_project(&row)?);
        }
        Ok(results)
    }

    /// Get a project by its ID.
    pub async fn get_project(&self, id: ProjectId) -> Result<Option<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE id = ?1");
        let mut rows = self
            .conn
            .query(&sql, libsql::params![id.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_project(&row)?)),
            None => Ok(None),
        }
    }

    /// Get a project by its URL-friendly slug.
    pub async fn get_project_by_slug(&self, slug: &str) -> Result<Option<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE slug = ?1");
        let mut rows = self
            .conn
            .query(&sql, libsql::params![slug.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_project(&row)?)),
            None => Ok(None),
        }
    }

    /// Create a new project with an auto-generated root feature.
    pub async fn create_project(&self, input: CreateProjectInput) -> Result<Project> {
        let project_id = input.id.unwrap_or_else(ProjectId::new);
        let root_feature_id = FeatureId::new();
        let now = Utc::now();

        // Generate slug from name if not provided
        let slug = input.slug.unwrap_or_else(|| slugify(&input.name));
        let key_prefix = input.key_prefix.unwrap_or_else(|| derive_key_prefix(&slug));

        self.conn
            .execute("BEGIN", ())
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Create project with root_feature_id
        let project_result = self
            .conn
            .execute(
                "INSERT INTO projects (id, slug, name, description, instructions, root_feature_id, key_prefix, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                libsql::params![
                    project_id.to_string(),
                    slug.clone(),
                    input.name.clone(),
                    match &input.description {
                        Some(d) => libsql::Value::Text(d.clone()),
                        None => libsql::Value::Null,
                    },
                    match &input.instructions {
                        Some(i) => libsql::Value::Text(i.clone()),
                        None => libsql::Value::Null,
                    },
                    root_feature_id.to_string(),
                    key_prefix.clone(),
                    now.to_rfc3339(),
                    now.to_rfc3339()
                ],
            )
            .await;

        if let Err(e) = project_result {
            let _ = self.conn.execute("ROLLBACK", ()).await;
            return Err(anyhow::anyhow!("{}", e));
        }

        // Create root feature
        let feature_result = self
            .conn
            .execute(
                "INSERT INTO features (id, project_id, parent_id, title, details, state, priority, created_at, updated_at)
                 VALUES (?1, ?2, NULL, ?3, ?4, 'implemented', 0, ?5, ?6)",
                libsql::params![
                    root_feature_id.to_string(),
                    project_id.to_string(),
                    input.name.clone(),
                    match &input.instructions {
                        Some(i) => libsql::Value::Text(i.clone()),
                        None => libsql::Value::Null,
                    },
                    now.to_rfc3339(),
                    now.to_rfc3339()
                ],
            )
            .await;

        if let Err(e) = feature_result {
            let _ = self.conn.execute("ROLLBACK", ()).await;
            return Err(anyhow::anyhow!("{}", e));
        }

        // Create default spec template
        let template_id = TemplateId::new();
        let template_result = self
            .conn
            .execute(
                "INSERT INTO spec_templates (id, project_id, name, description, content, is_default, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7)",
                libsql::params![
                    template_id.to_string(),
                    project_id.to_string(),
                    "Default".to_string(),
                    "General-purpose feature specification template".to_string(),
                    DEFAULT_TEMPLATE_CONTENT.to_string(),
                    now.to_rfc3339(),
                    now.to_rfc3339()
                ],
            )
            .await;

        if let Err(e) = template_result {
            let _ = self.conn.execute("ROLLBACK", ()).await;
            return Err(anyhow::anyhow!("{}", e));
        }

        self.conn
            .execute("COMMIT", ())
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(Project {
            id: project_id,
            slug,
            name: input.name,
            description: input.description,
            instructions: input.instructions,
            current_version_id: None,
            root_feature_id: Some(root_feature_id),
            default_feature_destination: "backlog".to_string(),
            test_adapter: None,
            context_budget: None,
            key_prefix,
            created_at: now,
            updated_at: now,
        })
    }

    /// Update an existing project's fields.
    ///
    /// Renaming a project also updates the root feature's title to stay in sync.
    pub async fn update_project(
        &self,
        id: ProjectId,
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
        let test_adapter = input.test_adapter.or(existing.test_adapter);
        let context_budget = input.context_budget.or(existing.context_budget);
        let key_prefix = input.key_prefix.unwrap_or(existing.key_prefix);

        self.conn
            .execute(
                "UPDATE projects SET slug = ?1, name = ?2, description = ?3, instructions = ?4, current_version_id = ?5, default_feature_destination = ?6, testing_policy = 'tdd', test_adapter = ?7, context_budget = ?8, key_prefix = ?9, updated_at = ?10 WHERE id = ?11",
                libsql::params![
                    slug.clone(),
                    name.clone(),
                    match &description {
                        Some(d) => libsql::Value::Text(d.clone()),
                        None => libsql::Value::Null,
                    },
                    match &instructions {
                        Some(i) => libsql::Value::Text(i.clone()),
                        None => libsql::Value::Null,
                    },
                    match &current_version_id {
                        Some(v) => libsql::Value::Text(v.to_string()),
                        None => libsql::Value::Null,
                    },
                    default_feature_destination.clone(),
                    match &test_adapter {
                        Some(t) => libsql::Value::Text(t.clone()),
                        None => libsql::Value::Null,
                    },
                    match context_budget {
                        Some(b) => libsql::Value::Integer(b),
                        None => libsql::Value::Null,
                    },
                    key_prefix.clone(),
                    now.to_rfc3339(),
                    id.to_string()
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Sync name to root feature title if changed
        if name_changed {
            if let Some(root_id) = existing.root_feature_id {
                self.conn
                    .execute(
                        "UPDATE features SET title = ?1, updated_at = ?2 WHERE id = ?3",
                        libsql::params![name.clone(), now.to_rfc3339(), root_id.to_string()],
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
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
            test_adapter,
            context_budget,
            key_prefix,
            created_at: existing.created_at,
            updated_at: now,
        }))
    }

    /// Delete a project and all associated data (features, history, directories, versions).
    #[must_use = "check whether the project existed"]
    pub async fn delete_project(&self, id: ProjectId) -> Result<bool> {
        self.conn
            .execute("BEGIN", ())
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let id_str = id.to_string();

        // Delete feature history
        if let Err(e) = self
            .conn
            .execute(
                "DELETE FROM feature_history WHERE feature_id IN (SELECT id FROM features WHERE project_id = ?1)",
                libsql::params![id_str.clone()],
            )
            .await
        {
            let _ = self.conn.execute("ROLLBACK", ()).await;
            return Err(anyhow::anyhow!("{}", e));
        }

        // Delete features
        if let Err(e) = self
            .conn
            .execute(
                "DELETE FROM features WHERE project_id = ?1",
                libsql::params![id_str.clone()],
            )
            .await
        {
            let _ = self.conn.execute("ROLLBACK", ()).await;
            return Err(anyhow::anyhow!("{}", e));
        }

        // Delete directories
        if let Err(e) = self
            .conn
            .execute(
                "DELETE FROM project_directories WHERE project_id = ?1",
                libsql::params![id_str.clone()],
            )
            .await
        {
            let _ = self.conn.execute("ROLLBACK", ()).await;
            return Err(anyhow::anyhow!("{}", e));
        }

        // Delete spec templates
        if let Err(e) = self
            .conn
            .execute(
                "DELETE FROM spec_templates WHERE project_id = ?1",
                libsql::params![id_str.clone()],
            )
            .await
        {
            let _ = self.conn.execute("ROLLBACK", ()).await;
            return Err(anyhow::anyhow!("{}", e));
        }

        // Delete versions
        if let Err(e) = self
            .conn
            .execute(
                "DELETE FROM versions WHERE project_id = ?1",
                libsql::params![id_str.clone()],
            )
            .await
        {
            let _ = self.conn.execute("ROLLBACK", ()).await;
            return Err(anyhow::anyhow!("{}", e));
        }

        // Delete project
        let result = self
            .conn
            .execute(
                "DELETE FROM projects WHERE id = ?1",
                libsql::params![id_str],
            )
            .await;

        match result {
            Ok(rows_affected) => {
                self.conn
                    .execute("COMMIT", ())
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                Ok(rows_affected > 0)
            }
            Err(e) => {
                let _ = self.conn.execute("ROLLBACK", ()).await;
                Err(anyhow::anyhow!("{}", e))
            }
        }
    }

    // ============================================================
    // Project Directory operations
    // ============================================================

    /// Get all directories associated with a project.
    pub async fn get_project_directories(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<ProjectDirectory>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, project_id, path, git_remote, is_primary, instructions, created_at
                 FROM project_directories WHERE project_id = ?1 ORDER BY is_primary DESC, path",
                libsql::params![project_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            results.push(row_to_project_directory(&row)?);
        }
        Ok(results)
    }

    /// Add a filesystem directory to a project.
    pub async fn add_project_directory(
        &self,
        project_id: ProjectId,
        input: AddDirectoryInput,
    ) -> Result<ProjectDirectory> {
        self.get_project(project_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        let id = DirectoryId::new();
        let now = Utc::now();

        self.conn
            .execute(
                "INSERT INTO project_directories (id, project_id, path, git_remote, is_primary, instructions, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                libsql::params![
                    id.to_string(),
                    project_id.to_string(),
                    input.path.clone(),
                    match &input.git_remote {
                        Some(r) => libsql::Value::Text(r.clone()),
                        None => libsql::Value::Null,
                    },
                    if input.is_primary { 1i64 } else { 0i64 },
                    match &input.instructions {
                        Some(i) => libsql::Value::Text(i.clone()),
                        None => libsql::Value::Null,
                    },
                    now.to_rfc3339()
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

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

    /// Remove a directory association from a project.
    #[must_use = "check whether the directory existed"]
    pub async fn remove_project_directory(&self, id: DirectoryId) -> Result<bool> {
        let rows_affected = self
            .conn
            .execute(
                "DELETE FROM project_directories WHERE id = ?1",
                libsql::params![id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(rows_affected > 0)
    }

    /// Get a project along with all its associated directories.
    pub async fn get_project_with_directories(
        &self,
        id: ProjectId,
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

    // ============================================================
    // Project Focus operations
    // ============================================================

    /// Set the focused feature for a project. Pass `None` to clear focus.
    pub async fn set_project_focus(
        &self,
        project_id: ProjectId,
        feature_id: Option<FeatureId>,
    ) -> Result<()> {
        match feature_id {
            Some(fid) => {
                let now = Utc::now();
                // Upsert: SQLite supports INSERT ... ON CONFLICT
                self.conn
                    .execute(
                        "INSERT INTO project_focus (project_id, feature_id, updated_at)
                         VALUES (?1, ?2, ?3)
                         ON CONFLICT (project_id) DO UPDATE SET feature_id = ?2, updated_at = ?3",
                        libsql::params![project_id.to_string(), fid.to_string(), now.to_rfc3339()],
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
            }
            None => {
                self.conn
                    .execute(
                        "DELETE FROM project_focus WHERE project_id = ?1",
                        libsql::params![project_id.to_string()],
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
            }
        }
        Ok(())
    }

    /// Get the focused feature for a project.
    /// Returns `(feature_id, feature_title, feature_state)` or `None` if no focus is set.
    pub async fn get_project_focus(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<(FeatureId, String, String)>> {
        let mut rows = self
            .conn
            .query(
                "SELECT f.id, f.title, f.state
                 FROM project_focus pf
                 JOIN features f ON f.id = pf.feature_id
                 WHERE pf.project_id = ?1",
                libsql::params![project_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => {
                let id_str: String = row.get(0).map_err(|e| anyhow::anyhow!("{}", e))?;
                let title: String = row.get(1).map_err(|e| anyhow::anyhow!("{}", e))?;
                let state: String = row.get(2).map_err(|e| anyhow::anyhow!("{}", e))?;
                let feature_id: FeatureId = parse_id(id_str)?;
                Ok(Some((feature_id, title, state)))
            }
            None => Ok(None),
        }
    }

    /// Find a project by matching a filesystem path against registered directories.
    ///
    /// Matches the longest directory prefix, so nested paths resolve to the correct project.
    pub async fn get_project_by_directory(
        &self,
        path: &str,
    ) -> Result<Option<ProjectWithDirectories>> {
        let mut rows = self
            .conn
            .query(
                "SELECT project_id, path FROM project_directories ORDER BY length(path) DESC",
                (),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        while let Some(row) = rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            let project_id_str: String = row.get(0).map_err(|e| anyhow::anyhow!("{}", e))?;
            let dir_path: String = row.get(1).map_err(|e| anyhow::anyhow!("{}", e))?;
            if path == dir_path || path.starts_with(&format!("{}/", dir_path)) {
                let project_id: ProjectId = parse_id(project_id_str)?;
                return self.get_project_with_directories(project_id).await;
            }
        }

        Ok(None)
    }
}
