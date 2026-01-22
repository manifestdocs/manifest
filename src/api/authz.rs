//! Authorization service for role-based access control (RBAC).
//!
//! Implements project-level permissions based on membership roles.

use crate::db::Database;
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

/// Project membership roles with hierarchical permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// Read-only access to project and features.
    Viewer = 0,
    /// Can create, update, delete features and versions.
    Member = 1,
    /// Member permissions + can manage non-owner members.
    Admin = 2,
    /// Full access including project deletion and ownership transfer.
    Owner = 3,
}

impl Role {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "viewer" => Some(Self::Viewer),
            "member" => Some(Self::Member),
            "admin" => Some(Self::Admin),
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Member => "member",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }

    /// Check if this role has at least the required permission level.
    pub fn has_permission(&self, required: Permission) -> bool {
        match required {
            Permission::ProjectRead => true, // All roles can read
            Permission::ProjectUpdate => *self >= Role::Admin,
            Permission::ProjectDelete => *self >= Role::Owner,
            Permission::ProjectManageMembers => *self >= Role::Admin,
            Permission::FeatureRead => true,
            Permission::FeatureCreate => *self >= Role::Member,
            Permission::FeatureUpdate => *self >= Role::Member,
            Permission::FeatureDelete => *self >= Role::Member,
            Permission::VersionRead => true,
            Permission::VersionCreate => *self >= Role::Member,
            Permission::VersionUpdate => *self >= Role::Member,
            Permission::VersionDelete => *self >= Role::Member,
        }
    }
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

/// Project membership record.
#[derive(Debug, Clone)]
pub struct Membership {
    pub id: Uuid,
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub role: Role,
}

/// Authorization service that checks permissions against the database.
///
/// Note: This service is a placeholder for cloud deployment. In local mode,
/// there's no authentication and all operations are allowed. The full
/// implementation requires Database methods for membership queries.
#[derive(Clone)]
pub struct AuthzService {
    db: Database,
}

impl AuthzService {
    /// Create a new authorization service with a database reference.
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Get a user's membership in a project.
    ///
    /// Note: Membership queries will be implemented when cloud mode is enabled.
    /// For now, returns None (no membership required in local mode).
    pub async fn get_membership(
        &self,
        _project_id: Uuid,
        _user_id: Uuid,
    ) -> Result<Option<Membership>, AuthzError> {
        // Local mode: no memberships tracked
        Ok(None)
    }

    /// Check if a user has a specific permission on a project.
    ///
    /// Returns Ok(()) if allowed, Err(AuthzError::Forbidden) if denied.
    /// In local mode (no authentication), always allows access.
    pub async fn require_permission(
        &self,
        _user_id: Uuid,
        _project_id: Uuid,
        _permission: Permission,
    ) -> Result<(), AuthzError> {
        // Local mode: always allow
        Ok(())
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
        Ok(self.get_role(user_id, project_id).await? == Some(Role::Owner))
    }

    /// List all project IDs a user can access.
    /// In local mode, returns all projects.
    pub async fn list_user_projects(&self, _user_id: Uuid) -> Result<Vec<Uuid>, AuthzError> {
        let projects = self
            .db
            .get_all_projects()
            .await
            .map_err(|e| AuthzError::Database(e.to_string()))?;
        Ok(projects.into_iter().map(|p| p.id).collect())
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
