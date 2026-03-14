use anyhow::Result;
use chrono::Utc;
use uuid::Uuid;

use super::helpers::*;
use super::Database;
use crate::models::*;

/// Map a database row to a [`User`].
fn row_to_user(row: &libsql::Row) -> Result<User> {
    Ok(User {
        id: parse_id(row_get_str(row, "id"))?,
        email: row_get_str(row, "email"),
        email_verified_at: row_get_opt_str(row, "email_verified_at")
            .map(parse_datetime)
            .transpose()?,
        display_name: row_get_opt_str(row, "display_name"),
        avatar_url: row_get_opt_str(row, "avatar_url"),
        created_at: parse_datetime(row_get_str(row, "created_at"))?,
        updated_at: parse_datetime(row_get_str(row, "updated_at"))?,
    })
}

/// Map a database row to an [`OAuthIdentity`].
fn row_to_oauth_identity(row: &libsql::Row) -> Result<OAuthIdentity> {
    Ok(OAuthIdentity {
        id: Uuid::parse_str(&row_get_str(row, "id"))
            .map_err(|_| anyhow::anyhow!("Invalid UUID for oauth_identity.id"))?,
        user_id: parse_id(row_get_str(row, "user_id"))?,
        provider: row_get_str(row, "provider"),
        provider_user_id: row_get_str(row, "provider_user_id"),
        provider_email: row_get_opt_str(row, "provider_email"),
        access_token: row_get_opt_str(row, "access_token"),
        refresh_token: row_get_opt_str(row, "refresh_token"),
        token_expires_at: row_get_opt_str(row, "token_expires_at")
            .map(parse_datetime)
            .transpose()?,
        created_at: parse_datetime(row_get_str(row, "created_at"))?,
    })
}

/// Map a database row to a [`ProjectMembership`].
fn row_to_project_membership(row: &libsql::Row) -> Result<ProjectMembership> {
    Ok(ProjectMembership {
        id: parse_id(row_get_str(row, "id"))?,
        project_id: parse_id(row_get_str(row, "project_id"))?,
        user_id: parse_id(row_get_str(row, "user_id"))?,
        role: std::str::FromStr::from_str(&row_get_str(row, "role"))
            .unwrap_or(MembershipRole::Viewer),
        invited_by: row_get_opt_str(row, "invited_by")
            .map(parse_id)
            .transpose()?,
        created_at: parse_datetime(row_get_str(row, "created_at"))?,
    })
}

impl Database {
    /// Get a user by their ID.
    pub async fn get_user(&self, id: UserId) -> Result<Option<User>> {
        let sql = "SELECT id, email, email_verified_at, display_name, avatar_url, created_at, updated_at
             FROM users WHERE id = ?1";

        let mut rows = self
            .conn
            .query(sql, libsql::params![id.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_user(&row)?)),
            None => Ok(None),
        }
    }

    /// Get a user by their email address.
    pub async fn get_user_by_email(&self, email: &str) -> Result<Option<User>> {
        let sql = "SELECT id, email, email_verified_at, display_name, avatar_url, created_at, updated_at
             FROM users WHERE email = ?1";

        let mut rows = self
            .conn
            .query(sql, libsql::params![email.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_user(&row)?)),
            None => Ok(None),
        }
    }

    /// Get a user by their Clerk ID (via oauth_identities table).
    pub async fn get_user_by_clerk_id(&self, clerk_id: &str) -> Result<Option<User>> {
        let sql = "SELECT u.id, u.email, u.email_verified_at, u.display_name, u.avatar_url, u.created_at, u.updated_at
             FROM users u
             INNER JOIN oauth_identities o ON u.id = o.user_id
             WHERE o.provider = 'clerk' AND o.provider_user_id = ?1";

        let mut rows = self
            .conn
            .query(sql, libsql::params![clerk_id.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_user(&row)?)),
            None => Ok(None),
        }
    }

    /// Get a user by OAuth provider and provider user ID.
    pub async fn get_user_by_oauth_provider(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<Option<User>> {
        let sql = "SELECT u.id, u.email, u.email_verified_at, u.display_name, u.avatar_url, u.created_at, u.updated_at
             FROM users u
             INNER JOIN oauth_identities o ON u.id = o.user_id
             WHERE o.provider = ?1 AND o.provider_user_id = ?2";

        let mut rows = self
            .conn
            .query(
                sql,
                libsql::params![provider.to_string(), provider_user_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_user(&row)?)),
            None => Ok(None),
        }
    }

    /// Create a new user.
    pub async fn create_user(
        &self,
        id: UserId,
        email: &str,
        display_name: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<User> {
        let now = Utc::now();

        self.conn
            .execute(
                "INSERT INTO users (id, email, display_name, avatar_url, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                libsql::params![
                    id.to_string(),
                    email.to_string(),
                    match display_name {
                        Some(v) => libsql::Value::Text(v.to_string()),
                        None => libsql::Value::Null,
                    },
                    match avatar_url {
                        Some(v) => libsql::Value::Text(v.to_string()),
                        None => libsql::Value::Null,
                    },
                    now.to_rfc3339(),
                    now.to_rfc3339()
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

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
        id: UserId,
        display_name: Option<&str>,
        avatar_url: Option<&str>,
    ) -> Result<bool> {
        let now = Utc::now();

        let rows_affected = self
            .conn
            .execute(
                "UPDATE users SET display_name = ?1, avatar_url = ?2, updated_at = ?3 WHERE id = ?4",
                libsql::params![
                    match display_name {
                        Some(v) => libsql::Value::Text(v.to_string()),
                        None => libsql::Value::Null,
                    },
                    match avatar_url {
                        Some(v) => libsql::Value::Text(v.to_string()),
                        None => libsql::Value::Null,
                    },
                    now.to_rfc3339(),
                    id.to_string()
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        Ok(rows_affected > 0)
    }

    // ============================================================
    // OAuth Identity operations
    // ============================================================

    /// Create an OAuth identity linking a provider account to a user.
    pub async fn create_oauth_identity(
        &self,
        id: Uuid,
        user_id: UserId,
        provider: &str,
        provider_user_id: &str,
        provider_email: Option<&str>,
    ) -> Result<OAuthIdentity> {
        let now = Utc::now();

        self.conn
            .execute(
                "INSERT INTO oauth_identities (id, user_id, provider, provider_user_id, provider_email, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                libsql::params![
                    id.to_string(),
                    user_id.to_string(),
                    provider.to_string(),
                    provider_user_id.to_string(),
                    match provider_email {
                        Some(v) => libsql::Value::Text(v.to_string()),
                        None => libsql::Value::Null,
                    },
                    now.to_rfc3339()
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

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
    pub async fn get_oauth_identities_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<OAuthIdentity>> {
        let sql = "SELECT id, user_id, provider, provider_user_id, provider_email, access_token, refresh_token, token_expires_at, created_at
             FROM oauth_identities WHERE user_id = ?1";

        let mut rows = self
            .conn
            .query(sql, libsql::params![user_id.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut identities = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            identities.push(row_to_oauth_identity(&row)?);
        }

        Ok(identities)
    }

    // ============================================================
    // Project Membership operations
    // ============================================================

    /// Get a user's membership in a project.
    pub async fn get_project_membership(
        &self,
        project_id: ProjectId,
        user_id: UserId,
    ) -> Result<Option<ProjectMembership>> {
        let sql = "SELECT id, project_id, user_id, role, invited_by, created_at
             FROM project_memberships WHERE project_id = ?1 AND user_id = ?2";

        let mut rows = self
            .conn
            .query(
                sql,
                libsql::params![project_id.to_string(), user_id.to_string()],
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        match rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            Some(row) => Ok(Some(row_to_project_membership(&row)?)),
            None => Ok(None),
        }
    }

    /// Get all project IDs a user can access (via membership).
    pub async fn get_user_project_ids(&self, user_id: UserId) -> Result<Vec<ProjectId>> {
        let sql = "SELECT project_id FROM project_memberships WHERE user_id = ?1";

        let mut rows = self
            .conn
            .query(sql, libsql::params![user_id.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut ids = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            ids.push(parse_id(row.get::<String>(0).map_err(|e| anyhow::anyhow!("{}", e))?)?);
        }

        Ok(ids)
    }

    /// Get all projects a user can access (via membership).
    pub async fn get_user_projects(&self, user_id: UserId) -> Result<Vec<Project>> {
        let sql = "SELECT p.id, p.slug, p.name, p.description, p.instructions, p.current_version_id, p.root_feature_id, p.default_feature_destination, p.test_adapter, p.context_budget, p.key_prefix, p.created_at, p.updated_at
             FROM projects p
             INNER JOIN project_memberships pm ON p.id = pm.project_id
             WHERE pm.user_id = ?1
             ORDER BY p.name";

        let mut rows = self
            .conn
            .query(sql, libsql::params![user_id.to_string()])
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let mut projects = Vec::new();
        while let Some(row) = rows.next().await.map_err(|e| anyhow::anyhow!("{}", e))? {
            projects.push(row_to_project(&row)?);
        }

        Ok(projects)
    }

    /// Create a project with an owner membership.
    /// This ensures the creating user automatically becomes the owner.
    pub async fn create_project_with_owner(
        &self,
        input: CreateProjectInput,
        owner_id: UserId,
    ) -> Result<Project> {
        let project_id = ProjectId::new();
        let root_feature_id = FeatureId::new();
        let membership_id = MembershipId::new();
        let now = Utc::now();

        // Generate slug from name if not provided
        let slug = input.slug.unwrap_or_else(|| slugify(&input.name));
        let key_prefix = input.key_prefix.unwrap_or_else(|| derive_key_prefix(&slug));

        let tx = self.conn.transaction().await.map_err(|e| anyhow::anyhow!("{}", e))?;

        // Create project with owner_id
        tx.execute(
            "INSERT INTO projects (id, slug, name, description, instructions, root_feature_id, owner_id, key_prefix, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                owner_id.to_string(),
                key_prefix.clone(),
                now.to_rfc3339(),
                now.to_rfc3339()
            ],
        )
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Create root feature
        tx.execute(
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
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Create owner membership
        tx.execute(
            "INSERT INTO project_memberships (id, project_id, user_id, role, created_at)
             VALUES (?1, ?2, ?3, 'owner', ?4)",
            libsql::params![
                membership_id.to_string(),
                project_id.to_string(),
                owner_id.to_string(),
                now.to_rfc3339()
            ],
        )
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Create default spec template
        let template_id = TemplateId::new();
        tx.execute(
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
        .await
        .map_err(|e| anyhow::anyhow!("{}", e))?;

        tx.commit().await.map_err(|e| anyhow::anyhow!("{}", e))?;

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
}
