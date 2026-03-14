use anyhow::Result;
use chrono::Utc;

use super::helpers::*;
use super::Database;
use crate::models::*;

impl Database {
    /// Get a spec template by ID.
    pub async fn get_template(&self, id: TemplateId) -> Result<Option<SpecTemplate>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, project_id, name, description, content, is_default, created_at, updated_at
                 FROM spec_templates WHERE id = ?1",
                libsql::params![id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_spec_template(&row)?)),
            None => Ok(None),
        }
    }

    /// Get the default spec template for a project.
    pub async fn get_default_template(
        &self,
        project_id: ProjectId,
    ) -> Result<Option<SpecTemplate>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id, project_id, name, description, content, is_default, created_at, updated_at
                 FROM spec_templates WHERE project_id = ?1 AND is_default = 1",
                libsql::params![project_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_spec_template(&row)?)),
            None => Ok(None),
        }
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

        let tx = self
            .conn
            .transaction()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // If this template is default, clear existing defaults
        if input.is_default {
            tx.execute(
                "UPDATE spec_templates SET is_default = 0, updated_at = ?1 WHERE project_id = ?2 AND is_default = 1",
                libsql::params![now.to_rfc3339(), project_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        }

        tx.execute(
            "INSERT INTO spec_templates (id, project_id, name, description, content, is_default, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            libsql::params![
                id.to_string(),
                project_id.to_string(),
                input.name.clone(),
                match &input.description {
                    Some(d) => libsql::Value::Text(d.clone()),
                    None => libsql::Value::Null,
                },
                input.content.clone(),
                if input.is_default { 1i32 } else { 0i32 },
                now.to_rfc3339(),
                now.to_rfc3339()
            ],
        )
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        tx.commit().await.map_err(|e| anyhow::anyhow!("{}", e))?;

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

        let tx = self
            .conn
            .transaction()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // If becoming default, clear other defaults first
        if is_default && !existing.is_default {
            tx.execute(
                "UPDATE spec_templates SET is_default = 0, updated_at = ?1 WHERE project_id = ?2 AND is_default = 1",
                libsql::params![now.to_rfc3339(), existing.project_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        }

        tx.execute(
            "UPDATE spec_templates SET name = ?1, description = ?2, content = ?3, is_default = ?4, updated_at = ?5 WHERE id = ?6",
            libsql::params![
                name.clone(),
                match &description {
                    Some(d) => libsql::Value::Text(d.clone()),
                    None => libsql::Value::Null,
                },
                content.clone(),
                if is_default { 1i32 } else { 0i32 },
                now.to_rfc3339(),
                id.to_string()
            ],
        )
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        tx.commit().await.map_err(|e| anyhow::anyhow!("{}", e))?;

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
