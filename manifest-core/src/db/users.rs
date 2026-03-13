use anyhow::Result;
use chrono::Utc;
use uuid::Uuid;

use super::helpers::*;
use super::Database;
use crate::models::*;

impl Database {
    /// Get a user by their ID.
    pub async fn get_user(&self, id: UserId) -> Result<Option<User>> {
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
        id: UserId,
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
        id: UserId,
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
        user_id: UserId,
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
    pub async fn get_oauth_identities_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<OAuthIdentity>> {
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
        project_id: ProjectId,
        user_id: UserId,
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
    pub async fn get_user_project_ids(&self, user_id: UserId) -> Result<Vec<ProjectId>> {
        let rows: Vec<String> =
            sqlx::query_scalar("SELECT project_id FROM project_memberships WHERE user_id = $1")
                .bind(user_id.to_string())
                .fetch_all(&self.pool)
                .await?;

        rows.into_iter().map(parse_id).collect()
    }

    /// Get all projects a user can access (via membership).
    pub async fn get_user_projects(&self, user_id: UserId) -> Result<Vec<Project>> {
        let rows = sqlx::query(
            "SELECT p.id, p.slug, p.name, p.description, p.instructions, p.current_version_id, p.root_feature_id, p.default_feature_destination, p.test_adapter, p.context_budget, p.key_prefix, p.created_at, p.updated_at
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
        owner_id: UserId,
    ) -> Result<Project> {
        let project_id = ProjectId::new();
        let root_feature_id = FeatureId::new();
        let membership_id = MembershipId::new();
        let now = Utc::now();

        // Generate slug from name if not provided
        let slug = input.slug.unwrap_or_else(|| slugify(&input.name));
        let key_prefix = input.key_prefix.unwrap_or_else(|| derive_key_prefix(&slug));

        let mut tx = self.pool.begin().await?;

        // Create project with owner_id
        sqlx::query(
            "INSERT INTO projects (id, slug, name, description, instructions, root_feature_id, owner_id, key_prefix, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(project_id.to_string())
        .bind(&slug)
        .bind(&input.name)
        .bind(&input.description)
        .bind(&input.instructions)
        .bind(root_feature_id.to_string())
        .bind(owner_id.to_string())
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
}
