use anyhow::Result;
use chrono::Utc;

use super::helpers::*;
use super::{Database, ManifestError};
use crate::models::*;

impl Database {
    /// Get all versions for a project, ordered by creation date.
    pub async fn get_versions_by_project(&self, project_id: ProjectId) -> Result<Vec<Version>> {
        let rows = sqlx::query(
            "SELECT id, project_id, name, description, released_at, created_at, updated_at
             FROM versions WHERE project_id = $1 ORDER BY created_at",
        )
        .bind(project_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_version).collect()
    }

    /// Get a single version by ID.
    pub async fn get_version(&self, id: VersionId) -> Result<Option<Version>> {
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
    pub async fn get_next_version(&self, project_id: ProjectId) -> Result<Option<Version>> {
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
    pub async fn get_latest_version(&self, project_id: ProjectId) -> Result<Option<Version>> {
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
        project_id: ProjectId,
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

    /// Create a new version for a project.
    ///
    /// Version names must be valid semantic versions. Projects are capped at 6 unreleased versions.
    pub async fn create_version(
        &self,
        project_id: ProjectId,
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

        let id = VersionId::new();
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

    /// Update an existing version's name, description, or release timestamp.
    pub async fn update_version(
        &self,
        id: VersionId,
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

    /// Delete a version by ID.
    pub async fn delete_version(&self, id: VersionId) -> Result<bool> {
        let result = sqlx::query("DELETE FROM versions WHERE id = $1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Validate that a version has not been released.
    /// Returns an error if the version is released, preventing feature assignment to past versions.
    pub(crate) async fn validate_version_not_released(&self, version_id: VersionId) -> Result<()> {
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
}
