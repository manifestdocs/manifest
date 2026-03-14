use anyhow::Result;
use chrono::Utc;

use super::helpers::*;
use super::{Database, ManifestError};
use crate::models::*;

/// SELECT columns for the remotes table.
const REMOTE_COLS: &str =
    "id, name, provider, url, auth_token, sync_enabled, created_at, updated_at";

/// SELECT columns for the project_remotes table.
const PROJECT_REMOTE_COLS: &str = "project_id, remote_id, sync_state, last_synced_at";

impl Database {
    // ── Remote CRUD ────────────────────────────────────────────────────

    /// Get all configured remotes.
    pub async fn list_remotes(&self) -> Result<Vec<Remote>> {
        let sql = format!("SELECT {REMOTE_COLS} FROM remotes ORDER BY name");
        let mut rows = self
            .conn
            .query(&sql, libsql::params![])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            results.push(row_to_remote(&row)?);
        }
        Ok(results)
    }

    /// Get a remote by ID.
    pub async fn get_remote(&self, id: RemoteId) -> Result<Option<Remote>> {
        let sql = format!("SELECT {REMOTE_COLS} FROM remotes WHERE id = ?1");
        let mut rows = self
            .conn
            .query(&sql, libsql::params![id.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_remote(&row)?)),
            None => Ok(None),
        }
    }

    /// Get the auth token for a remote by ID.
    ///
    /// The token is stored in the DB but not included in the `Remote` struct
    /// for safety. Use this method when you need the actual token value.
    pub async fn get_remote_token(&self, id: RemoteId) -> Result<Option<String>> {
        let mut rows = self
            .conn
            .query(
                "SELECT auth_token FROM remotes WHERE id = ?1",
                libsql::params![id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => {
                let token: Option<String> =
                    row.get::<Option<String>>(0).map_err(|e| anyhow::anyhow!("{}", e))?;
                Ok(token)
            }
            None => Ok(None),
        }
    }

    /// Get a remote by name.
    pub async fn get_remote_by_name(&self, name: &str) -> Result<Option<Remote>> {
        let sql = format!("SELECT {REMOTE_COLS} FROM remotes WHERE name = ?1");
        let mut rows = self
            .conn
            .query(&sql, libsql::params![name.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_remote(&row)?)),
            None => Ok(None),
        }
    }

    /// Create a new remote.
    ///
    /// The `token` is stored as-is in the `auth_token` column. Callers are
    /// responsible for encrypting the token before passing it here (e.g., via
    /// OS keychain or local encryption).
    pub async fn create_remote(&self, input: &CreateRemoteInput) -> Result<Remote> {
        // Check for duplicate name
        if self.get_remote_by_name(&input.name).await?.is_some() {
            return Err(ManifestError::validation(format!(
                "Remote '{}' already exists. Use a different name or update the existing remote.",
                input.name
            ))
            .into());
        }

        let id = RemoteId::new();
        let now = Utc::now();
        let provider = input.provider.as_deref().unwrap_or("turso");

        self.conn
            .execute(
                "INSERT INTO remotes (id, name, provider, url, auth_token, sync_enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7)",
                libsql::params![
                    id.to_string(),
                    input.name.clone(),
                    provider.to_string(),
                    input.url.clone(),
                    input.token.clone(),
                    now.to_rfc3339(),
                    now.to_rfc3339()
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(Remote {
            id,
            name: input.name.clone(),
            provider: provider.to_string(),
            url: input.url.clone(),
            sync_enabled: true,
            created_at: now,
            updated_at: now,
        })
    }

    /// Update an existing remote's URL, token, or sync_enabled flag.
    pub async fn update_remote(
        &self,
        id: RemoteId,
        input: &UpdateRemoteInput,
    ) -> Result<Option<Remote>> {
        let Some(existing) = self.get_remote(id).await? else {
            return Ok(None);
        };

        let now = Utc::now();
        let url = input.url.as_deref().unwrap_or(&existing.url);
        let sync_enabled = input.sync_enabled.unwrap_or(existing.sync_enabled);

        // For auth_token, we need to read the current value if not updating.
        // We store it in the DB but don't include it in the Remote struct for safety.
        // If a new token is provided, use it; otherwise keep existing.
        if let Some(token) = &input.token {
            self.conn
                .execute(
                    "UPDATE remotes SET url = ?1, auth_token = ?2, sync_enabled = ?3, updated_at = ?4 WHERE id = ?5",
                    libsql::params![
                        url.to_string(),
                        token.clone(),
                        sync_enabled,
                        now.to_rfc3339(),
                        id.to_string()
                    ],
                )
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        } else {
            self.conn
                .execute(
                    "UPDATE remotes SET url = ?1, sync_enabled = ?2, updated_at = ?3 WHERE id = ?4",
                    libsql::params![
                        url.to_string(),
                        sync_enabled,
                        now.to_rfc3339(),
                        id.to_string()
                    ],
                )
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
        }

        Ok(Some(Remote {
            id,
            name: existing.name,
            provider: existing.provider,
            url: url.to_string(),
            sync_enabled,
            created_at: existing.created_at,
            updated_at: now,
        }))
    }

    /// Delete a remote and orphan all linked projects.
    ///
    /// Sets `sync_state = 'orphaned'` on all project_remotes entries for this
    /// remote, then deletes the remote. Local project data is preserved.
    pub async fn delete_remote(&self, id: RemoteId) -> Result<bool> {
        // Orphan linked projects first
        self.conn
            .execute(
                "UPDATE project_remotes SET sync_state = 'orphaned' WHERE remote_id = ?1",
                libsql::params![id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let rows_affected = self
            .conn
            .execute(
                "DELETE FROM remotes WHERE id = ?1",
                libsql::params![id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(rows_affected > 0)
    }

    // ── Project-Remote Linking ─────────────────────────────────────────

    /// Link a project to a remote.
    pub async fn link_project_remote(
        &self,
        project_id: ProjectId,
        remote_id: RemoteId,
    ) -> Result<ProjectRemote> {
        // Verify project exists
        self.get_project(project_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Project"))?;

        // Verify remote exists
        self.get_remote(remote_id)
            .await?
            .ok_or_else(|| ManifestError::not_found("Remote"))?;

        // Check if already linked
        if let Some(existing) = self.get_project_remote(project_id, remote_id).await? {
            if existing.sync_state != SyncState::Orphaned {
                return Err(ManifestError::validation(format!(
                    "Project is already linked to this remote (state: {}).",
                    existing.sync_state.as_str()
                ))
                .into());
            }
            // Re-activate orphaned link
            self.conn
                .execute(
                    "UPDATE project_remotes SET sync_state = 'active', last_synced_at = NULL WHERE project_id = ?1 AND remote_id = ?2",
                    libsql::params![project_id.to_string(), remote_id.to_string()],
                )
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            return Ok(ProjectRemote {
                project_id,
                remote_id,
                sync_state: SyncState::Active,
                last_synced_at: None,
            });
        }

        self.conn
            .execute(
                "INSERT INTO project_remotes (project_id, remote_id, sync_state)
                 VALUES (?1, ?2, 'active')",
                libsql::params![project_id.to_string(), remote_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(ProjectRemote {
            project_id,
            remote_id,
            sync_state: SyncState::Active,
            last_synced_at: None,
        })
    }

    /// Unlink a project from a remote. Preserves local data.
    pub async fn unlink_project_remote(
        &self,
        project_id: ProjectId,
        remote_id: RemoteId,
    ) -> Result<bool> {
        let rows_affected = self
            .conn
            .execute(
                "DELETE FROM project_remotes WHERE project_id = ?1 AND remote_id = ?2",
                libsql::params![project_id.to_string(), remote_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(rows_affected > 0)
    }

    /// Get a specific project-remote binding.
    pub async fn get_project_remote(
        &self,
        project_id: ProjectId,
        remote_id: RemoteId,
    ) -> Result<Option<ProjectRemote>> {
        let sql = format!(
            "SELECT {PROJECT_REMOTE_COLS} FROM project_remotes WHERE project_id = ?1 AND remote_id = ?2"
        );
        let mut rows = self
            .conn
            .query(
                &sql,
                libsql::params![project_id.to_string(), remote_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_project_remote(&row)?)),
            None => Ok(None),
        }
    }

    /// Get all project-remote bindings for a remote.
    pub async fn get_remote_projects(&self, remote_id: RemoteId) -> Result<Vec<ProjectRemote>> {
        let sql = format!(
            "SELECT {PROJECT_REMOTE_COLS} FROM project_remotes WHERE remote_id = ?1 ORDER BY project_id"
        );
        let mut rows = self
            .conn
            .query(&sql, libsql::params![remote_id.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            results.push(row_to_project_remote(&row)?);
        }
        Ok(results)
    }

    /// Get all remotes linked to a project.
    pub async fn get_project_remotes(&self, project_id: ProjectId) -> Result<Vec<ProjectRemote>> {
        let sql = format!(
            "SELECT {PROJECT_REMOTE_COLS} FROM project_remotes WHERE project_id = ?1 ORDER BY remote_id"
        );
        let mut rows = self
            .conn
            .query(&sql, libsql::params![project_id.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut results = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            results.push(row_to_project_remote(&row)?);
        }
        Ok(results)
    }
}
