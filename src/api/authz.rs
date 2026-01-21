//! Authorization service for role-based access control (RBAC).
//!
//! Implements project-level permissions based on membership roles.

use rusqlite::OptionalExtension;
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
pub struct AuthzService<'a> {
    conn: &'a rusqlite::Connection,
}

impl<'a> AuthzService<'a> {
    /// Create a new authorization service with a database connection.
    pub fn new(conn: &'a rusqlite::Connection) -> Self {
        Self { conn }
    }

    /// Get a user's membership in a project.
    pub fn get_membership(
        &self,
        project_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<Membership>, AuthzError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, project_id, user_id, role FROM project_memberships
                 WHERE project_id = ? AND user_id = ?",
            )
            .map_err(|e| AuthzError::Database(e.to_string()))?;

        let membership = stmt
            .query_row([project_id.to_string(), user_id.to_string()], |row| {
                let role_str: String = row.get(3)?;
                Ok(Membership {
                    id: row.get::<_, String>(0)?.parse().unwrap(),
                    project_id: row.get::<_, String>(1)?.parse().unwrap(),
                    user_id: row.get::<_, String>(2)?.parse().unwrap(),
                    role: Role::from_str(&role_str).unwrap_or(Role::Viewer),
                })
            })
            .optional()
            .map_err(|e| AuthzError::Database(e.to_string()))?;

        Ok(membership)
    }

    /// Check if a user has a specific permission on a project.
    ///
    /// Returns Ok(()) if allowed, Err(AuthzError::Forbidden) if denied.
    pub fn require_permission(
        &self,
        user_id: Uuid,
        project_id: Uuid,
        permission: Permission,
    ) -> Result<(), AuthzError> {
        // Check if project is public (for read permissions)
        if matches!(
            permission,
            Permission::ProjectRead | Permission::FeatureRead | Permission::VersionRead
        ) {
            if self.is_project_public(project_id)? {
                return Ok(());
            }
        }

        // Check membership
        let membership = self.get_membership(project_id, user_id)?;

        match membership {
            Some(m) if m.role.has_permission(permission) => Ok(()),
            _ => Err(AuthzError::Forbidden),
        }
    }

    /// Check if a project is public.
    fn is_project_public(&self, project_id: Uuid) -> Result<bool, AuthzError> {
        let visibility: Option<String> = self
            .conn
            .query_row(
                "SELECT visibility FROM projects WHERE id = ?",
                [project_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| AuthzError::Database(e.to_string()))?
            .flatten();

        Ok(visibility.as_deref() == Some("public"))
    }

    /// Get a user's role in a project, if they have membership.
    pub fn get_role(&self, user_id: Uuid, project_id: Uuid) -> Result<Option<Role>, AuthzError> {
        Ok(self.get_membership(project_id, user_id)?.map(|m| m.role))
    }

    /// Check if a user has owner role on a project.
    pub fn is_owner(&self, user_id: Uuid, project_id: Uuid) -> Result<bool, AuthzError> {
        Ok(self.get_role(user_id, project_id)? == Some(Role::Owner))
    }

    /// List all project IDs a user can access (via membership or public visibility).
    pub fn list_user_projects(&self, user_id: Uuid) -> Result<Vec<Uuid>, AuthzError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT project_id FROM project_memberships WHERE user_id = ?
                 UNION
                 SELECT id FROM projects WHERE visibility = 'public'",
            )
            .map_err(|e| AuthzError::Database(e.to_string()))?;

        let projects = stmt
            .query_map([user_id.to_string()], |row| {
                let id: String = row.get(0)?;
                Ok(id.parse::<Uuid>().unwrap())
            })
            .map_err(|e| AuthzError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AuthzError::Database(e.to_string()))?;

        Ok(projects)
    }
}

/// Trait for authorization context in handlers.
pub trait RequirePermission {
    fn require_permission(
        &self,
        user_id: Uuid,
        project_id: Uuid,
        permission: Permission,
    ) -> Result<(), AuthzError>;
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
