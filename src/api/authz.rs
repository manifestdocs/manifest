//! Authorization service for role-based access control (RBAC).
//!
//! Implements project-level permissions based on membership roles.
//!
//! In local mode (no authentication), all operations are allowed.
//! In cloud mode, users must have appropriate membership roles.

use crate::db::Database;
use manifest_core::models::MembershipRole;
use thiserror::Error;
use uuid::Uuid;

/// Authorization errors.
#[derive(Debug, Error)]
pub enum AuthzError {
    #[error("Access denied: insufficient permissions")]
    Forbidden,

    #[error("Resource not found")]
    NotFound,

    #[error("Database error: {0}")]
    Database(String),
}

/// Alias for MembershipRole for backwards compatibility.
pub type Role = MembershipRole;

impl RoleExt for MembershipRole {
    /// Check if this role has at least the required permission level.
    fn has_permission(&self, required: Permission) -> bool {
        match required {
            Permission::ProjectRead => true, // All roles can read
            Permission::ProjectUpdate => *self >= MembershipRole::Admin,
            Permission::ProjectDelete => *self >= MembershipRole::Owner,
            Permission::ProjectManageMembers => *self >= MembershipRole::Admin,
            Permission::FeatureRead => true,
            Permission::FeatureCreate => *self >= MembershipRole::Member,
            Permission::FeatureUpdate => *self >= MembershipRole::Member,
            Permission::FeatureDelete => *self >= MembershipRole::Member,
            Permission::VersionRead => true,
            Permission::VersionCreate => *self >= MembershipRole::Member,
            Permission::VersionUpdate => *self >= MembershipRole::Member,
            Permission::VersionDelete => *self >= MembershipRole::Member,
        }
    }
}

/// Extension trait for permission checking on roles.
pub trait RoleExt {
    fn has_permission(&self, required: Permission) -> bool;
}

/// Granular permissions for authorization checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    // Project permissions
    ProjectRead,
    ProjectUpdate,
    ProjectDelete,
    ProjectManageMembers,

    // Feature permissions
    FeatureRead,
    FeatureCreate,
    FeatureUpdate,
    FeatureDelete,

    // Version permissions
    VersionRead,
    VersionCreate,
    VersionUpdate,
    VersionDelete,
}

/// Re-export ProjectMembership from manifest_core.
pub use manifest_core::models::ProjectMembership as Membership;

/// Authorization service that checks permissions against the database.
///
/// In local mode (cloud_mode=false), all operations are allowed.
/// In cloud mode (cloud_mode=true), users must have appropriate membership roles.
#[derive(Clone)]
pub struct AuthzService {
    db: Database,
    cloud_mode: bool,
}

impl AuthzService {
    /// Create a new authorization service for local mode (no auth checks).
    pub fn new(db: Database) -> Self {
        Self {
            db,
            cloud_mode: false,
        }
    }

    /// Create a new authorization service for cloud mode (enforces auth).
    pub fn new_cloud(db: Database) -> Self {
        Self {
            db,
            cloud_mode: true,
        }
    }

    /// Check if running in cloud mode.
    pub fn is_cloud_mode(&self) -> bool {
        self.cloud_mode
    }

    /// Get a user's membership in a project.
    ///
    /// In local mode, returns None (no membership required).
    /// In cloud mode, queries the database for actual membership.
    pub async fn get_membership(
        &self,
        project_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<Membership>, AuthzError> {
        if !self.cloud_mode {
            return Ok(None);
        }

        self.db
            .get_project_membership(project_id, user_id)
            .await
            .map_err(|e| AuthzError::Database(e.to_string()))
    }

    /// Check if a user has a specific permission on a project.
    ///
    /// Returns Ok(()) if allowed, Err(AuthzError::Forbidden) if denied.
    /// In local mode, always allows access.
    /// In cloud mode, checks membership and role permissions.
    pub async fn require_permission(
        &self,
        user_id: Uuid,
        project_id: Uuid,
        permission: Permission,
    ) -> Result<(), AuthzError> {
        if !self.cloud_mode {
            return Ok(());
        }

        let membership = self.get_membership(project_id, user_id).await?;

        match membership {
            Some(m) if m.role.has_permission(permission) => Ok(()),
            Some(_) => {
                tracing::warn!(
                    "Permission denied: user {} lacks {:?} on project {}",
                    user_id,
                    permission,
                    project_id
                );
                Err(AuthzError::Forbidden)
            }
            None => {
                tracing::warn!(
                    "Access denied: user {} has no membership in project {}",
                    user_id,
                    project_id
                );
                Err(AuthzError::Forbidden)
            }
        }
    }

    /// Get a user's role in a project, if they have membership.
    pub async fn get_role(
        &self,
        user_id: Uuid,
        project_id: Uuid,
    ) -> Result<Option<Role>, AuthzError> {
        Ok(self
            .get_membership(project_id, user_id)
            .await?
            .map(|m| m.role))
    }

    /// Check if a user has owner role on a project.
    pub async fn is_owner(&self, user_id: Uuid, project_id: Uuid) -> Result<bool, AuthzError> {
        Ok(self.get_role(user_id, project_id).await? == Some(MembershipRole::Owner))
    }

    /// List all project IDs a user can access.
    /// In local mode, returns all projects.
    /// In cloud mode, returns only projects where user has membership.
    pub async fn list_user_projects(&self, user_id: Uuid) -> Result<Vec<Uuid>, AuthzError> {
        if !self.cloud_mode {
            let projects = self
                .db
                .get_all_projects()
                .await
                .map_err(|e| AuthzError::Database(e.to_string()))?;
            return Ok(projects.into_iter().map(|p| p.id).collect());
        }

        self.db
            .get_user_project_ids(user_id)
            .await
            .map_err(|e| AuthzError::Database(e.to_string()))
    }
}

/// Trait for authorization context in handlers.
pub trait RequirePermission {
    fn require_permission(
        &self,
        user_id: Uuid,
        project_id: Uuid,
        permission: Permission,
    ) -> impl std::future::Future<Output = Result<(), AuthzError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_ordering() {
        assert!(Role::Owner > Role::Admin);
        assert!(Role::Admin > Role::Member);
        assert!(Role::Member > Role::Viewer);
    }

    #[test]
    fn test_role_permissions() {
        assert!(Role::Owner.has_permission(Permission::ProjectDelete));
        assert!(!Role::Admin.has_permission(Permission::ProjectDelete));
        assert!(Role::Admin.has_permission(Permission::ProjectManageMembers));
        assert!(!Role::Member.has_permission(Permission::ProjectManageMembers));
        assert!(Role::Member.has_permission(Permission::FeatureCreate));
        assert!(!Role::Viewer.has_permission(Permission::FeatureCreate));
        assert!(Role::Viewer.has_permission(Permission::ProjectRead));
    }

    #[test]
    fn test_role_from_str() {
        assert_eq!(Role::from_str("owner"), Some(Role::Owner));
        assert_eq!(Role::from_str("admin"), Some(Role::Admin));
        assert_eq!(Role::from_str("member"), Some(Role::Member));
        assert_eq!(Role::from_str("viewer"), Some(Role::Viewer));
        assert_eq!(Role::from_str("invalid"), None);
    }
}
