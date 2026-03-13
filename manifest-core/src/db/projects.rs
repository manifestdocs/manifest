use anyhow::Result;
use chrono::Utc;
use sqlx::Row;

use super::helpers::*;
use super::{Database, ManifestError};
use crate::models::*;

/// SELECT columns for the projects table.
const PROJECT_COLS: &str = "id, slug, name, description, instructions, current_version_id, root_feature_id, default_feature_destination, test_adapter, context_budget, key_prefix, created_at, updated_at";

impl Database {
    /// Get all projects ordered by name.
    pub async fn get_all_projects(&self) -> Result<Vec<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects ORDER BY name");
        let rows = sqlx::query(&sql).fetch_all(&self.pool).await?;

        rows.iter().map(row_to_project).collect()
    }

    /// Get a project by its ID.
    pub async fn get_project(&self, id: ProjectId) -> Result<Option<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE id = $1");
        let row = sqlx::query(&sql)
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;

        row.as_ref().map(row_to_project).transpose()
    }

    /// Get a project by its URL-friendly slug.
    pub async fn get_project_by_slug(&self, slug: &str) -> Result<Option<Project>> {
        let sql = format!("SELECT {PROJECT_COLS} FROM projects WHERE slug = $1");
        let row = sqlx::query(&sql)
            .bind(slug)
            .fetch_optional(&self.pool)
            .await?;

        row.as_ref().map(row_to_project).transpose()
    }

    /// Create a new project with an auto-generated root feature.
    pub async fn create_project(&self, input: CreateProjectInput) -> Result<Project> {
        let project_id = ProjectId::new();
        let root_feature_id = FeatureId::new();
        let now = Utc::now();

        // Generate slug from name if not provided
        let slug = input.slug.unwrap_or_else(|| slugify(&input.name));
        let key_prefix = input.key_prefix.unwrap_or_else(|| derive_key_prefix(&slug));

        let mut tx = self.pool.begin().await?;

        // Create project with root_feature_id
        sqlx::query(
            "INSERT INTO projects (id, slug, name, description, instructions, root_feature_id, key_prefix, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(project_id.to_string())
        .bind(&slug)
        .bind(&input.name)
        .bind(&input.description)
        .bind(&input.instructions)
        .bind(root_feature_id.to_string())
        .bind(&key_prefix)
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

        // Create default spec template
        let template_id = TemplateId::new();
        sqlx::query(
            "INSERT INTO spec_templates (id, project_id, name, description, content, is_default, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, 1, $6, $7)",
        )
        .bind(template_id.to_string())
        .bind(project_id.to_string())
        .bind("Default")
        .bind("General-purpose feature specification template")
        .bind(DEFAULT_TEMPLATE_CONTENT)
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

        sqlx::query(
            "UPDATE projects SET slug = $1, name = $2, description = $3, instructions = $4, current_version_id = $5, default_feature_destination = $6, testing_policy = 'tdd', test_adapter = $7, context_budget = $8, key_prefix = $9, updated_at = $10 WHERE id = $11",
        )
        .bind(&slug)
        .bind(&name)
        .bind(&description)
        .bind(&instructions)
        .bind(current_version_id.map(|u| u.to_string()))
        .bind(&default_feature_destination)
        .bind(&test_adapter)
        .bind(context_budget)
        .bind(&key_prefix)
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

        // Delete spec templates
        sqlx::query("DELETE FROM spec_templates WHERE project_id = $1")
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

    /// Get all directories associated with a project.
    pub async fn get_project_directories(
        &self,
        project_id: ProjectId,
    ) -> Result<Vec<ProjectDirectory>> {
        let rows = sqlx::query(
            "SELECT id, project_id, path, git_remote, is_primary, instructions, created_at
             FROM project_directories WHERE project_id = $1 ORDER BY is_primary DESC, path",
        )
        .bind(project_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_project_directory).collect()
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

    /// Remove a directory association from a project.
    #[must_use = "check whether the directory existed"]
    pub async fn remove_project_directory(&self, id: DirectoryId) -> Result<bool> {
        let result = sqlx::query("DELETE FROM project_directories WHERE id = $1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
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
                // Upsert: SQLite and Postgres both support this via INSERT ... ON CONFLICT
                sqlx::query(
                    "INSERT INTO project_focus (project_id, feature_id, updated_at)
                     VALUES ($1, $2, $3)
                     ON CONFLICT (project_id) DO UPDATE SET feature_id = $2, updated_at = $3",
                )
                .bind(project_id.to_string())
                .bind(fid.to_string())
                .bind(now.to_rfc3339())
                .execute(&self.pool)
                .await?;
            }
            None => {
                sqlx::query("DELETE FROM project_focus WHERE project_id = $1")
                    .bind(project_id.to_string())
                    .execute(&self.pool)
                    .await?;
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
        let row = sqlx::query(
            "SELECT f.id, f.title, f.state
             FROM project_focus pf
             JOIN features f ON f.id = pf.feature_id
             WHERE pf.project_id = $1",
        )
        .bind(project_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => {
                let id_str: String = row.get("id");
                let title: String = row.get("title");
                let state: String = row.get("state");
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
        let rows = sqlx::query(
            "SELECT project_id, path FROM project_directories ORDER BY length(path) DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        for row in &rows {
            let dir_path: String = row.get("path");
            if path == dir_path || path.starts_with(&format!("{}/", dir_path)) {
                let project_id_str: String = row.get("project_id");
                let project_id: ProjectId = parse_id(project_id_str)?;
                return self.get_project_with_directories(project_id).await;
            }
        }

        Ok(None)
    }
}
