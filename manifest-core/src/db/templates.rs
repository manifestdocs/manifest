use anyhow::Result;
use chrono::Utc;

use super::helpers::*;
use super::Database;
use crate::models::*;

impl Database {
    /// Get a spec template by ID.
    pub async fn get_template(&self, id: TemplateId) -> Result<Option<SpecTemplate>> {
        let row = sqlx::query(
            "SELECT id, project_id, name, description, content, is_default, created_at, updated_at
             FROM spec_templates WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_spec_template).transpose()
    }

    /// Get the default spec template for a project.
    pub async fn get_default_template(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<SpecTemplate>> {
        let row = sqlx::query(
            "SELECT id, project_id, name, description, content, is_default, created_at, updated_at
             FROM spec_templates WHERE project_id = $1 AND is_default = 1",
        )
        .bind(project_id.to_string())
        .fetch_optional(&self.pool)
        .await?;

        row.as_ref().map(row_to_spec_template).transpose()
    }

    /// Create a new spec template for a project.
    ///
    /// If `is_default` is true, clears any existing default for the project first.
    pub async fn create_template(
        &self,
        project_id: ProjectId,
        input: CreateTemplateInput,
    ) -> Result<SpecTemplate> {
        let id = TemplateId::new();
        let now = Utc::now();

        let mut tx = self.pool.begin().await?;

        // If this template is default, clear existing defaults
        if input.is_default {
            sqlx::query(
                "UPDATE spec_templates SET is_default = 0, updated_at = $1 WHERE project_id = $2 AND is_default = 1",
            )
            .bind(now.to_rfc3339())
            .bind(project_id.to_string())
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            "INSERT INTO spec_templates (id, project_id, name, description, content, is_default, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(id.to_string())
        .bind(project_id.to_string())
        .bind(&input.name)
        .bind(&input.description)
        .bind(&input.content)
        .bind(if input.is_default { 1i32 } else { 0i32 })
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(SpecTemplate {
            id,
            project_id,
            name: input.name,
            description: input.description,
            content: input.content,
            is_default: input.is_default,
            created_at: now,
            updated_at: now,
        })
    }

    /// Update an existing spec template.
    ///
    /// If `is_default` is set to true, clears any other default for the same project.
    pub async fn update_template(
        &self,
        id: TemplateId,
        input: UpdateTemplateInput,
    ) -> Result<Option<SpecTemplate>> {
        let Some(existing) = self.get_template(id).await? else {
            return Ok(None);
        };

        let now = Utc::now();
        let name = input.name.unwrap_or(existing.name);
        let description = input.description.or(existing.description);
        let content = input.content.unwrap_or(existing.content);
        let is_default = input.is_default.unwrap_or(existing.is_default);

        let mut tx = self.pool.begin().await?;

        // If becoming default, clear other defaults first
        if is_default && !existing.is_default {
            sqlx::query(
                "UPDATE spec_templates SET is_default = 0, updated_at = $1 WHERE project_id = $2 AND is_default = 1",
            )
            .bind(now.to_rfc3339())
            .bind(existing.project_id.to_string())
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query(
            "UPDATE spec_templates SET name = $1, description = $2, content = $3, is_default = $4, updated_at = $5 WHERE id = $6",
        )
        .bind(&name)
        .bind(&description)
        .bind(&content)
        .bind(if is_default { 1i32 } else { 0i32 })
        .bind(now.to_rfc3339())
        .bind(id.to_string())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(Some(SpecTemplate {
            id,
            project_id: existing.project_id,
            name,
            description,
            content,
            is_default,
            created_at: existing.created_at,
            updated_at: now,
        }))
    }
}
