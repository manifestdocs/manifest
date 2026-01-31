//! User and OAuth identity models for authentication.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{MembershipId, ProjectId, UserId};

/// A user account in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    /// Primary email address, used for login and notifications.
    pub email: String,
    /// When the email was verified, or None if unverified.
    pub email_verified_at: Option<DateTime<Utc>>,
    /// User-chosen display name shown in the UI.
    pub display_name: Option<String>,
    /// URL to the user's avatar image.
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An OAuth identity linked to a user account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthIdentity {
    pub id: Uuid,
    pub user_id: UserId,
    /// OAuth provider name (e.g., "github", "google").
    pub provider: String,
    /// The user's unique ID on the OAuth provider.
    pub provider_user_id: String,
    /// Email address from the OAuth provider, if available.
    pub provider_email: Option<String>,
    /// Current OAuth access token for API calls.
    pub access_token: Option<String>,
    /// OAuth refresh token for obtaining new access tokens.
    pub refresh_token: Option<String>,
    /// When the access token expires.
    pub token_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Input for creating a new user.
#[derive(Debug, Clone)]
pub struct CreateUserInput {
    /// Pre-generated user ID (typically from the auth provider).
    pub id: UserId,
    /// Primary email address for the user.
    pub email: String,
    /// User-chosen display name shown in the UI.
    pub display_name: Option<String>,
    /// URL to the user's avatar image.
    pub avatar_url: Option<String>,
}

/// Input for updating an existing user.
#[derive(Debug, Clone, Default)]
pub struct UpdateUserInput {
    /// New display name, or None to leave unchanged.
    pub display_name: Option<String>,
    /// New avatar URL, or None to leave unchanged.
    pub avatar_url: Option<String>,
    /// Set to mark the user's email as verified.
    pub email_verified_at: Option<DateTime<Utc>>,
}

/// Input for creating an OAuth identity.
#[derive(Debug, Clone)]
pub struct CreateOAuthIdentityInput {
    /// Pre-generated identity ID.
    pub id: Uuid,
    /// The user this identity belongs to.
    pub user_id: UserId,
    /// OAuth provider name (e.g., "github", "google").
    pub provider: String,
    /// The user's unique ID on the OAuth provider.
    pub provider_user_id: String,
    /// Email address from the OAuth provider, if available.
    pub provider_email: Option<String>,
}

/// Project membership roles with hierarchical permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipRole {
    /// Read-only access to project and features.
    Viewer,
    /// Can create, update, delete features and versions.
    Member,
    /// Member permissions + can manage non-owner members.
    Admin,
    /// Full access including project deletion and ownership transfer.
    Owner,
}

impl FromStr for MembershipRole {
    type Err = super::ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "viewer" => Ok(Self::Viewer),
            "member" => Ok(Self::Member),
            "admin" => Ok(Self::Admin),
            "owner" => Ok(Self::Owner),
            _ => Err(super::ParseEnumError(s.to_string())),
        }
    }
}

impl MembershipRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Member => "member",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }
}

/// A user's membership in a project with a specific role.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMembership {
    pub id: MembershipId,
    pub project_id: ProjectId,
    pub user_id: UserId,
    /// The user's role determining their permissions in this project.
    pub role: MembershipRole,
    /// The user who invited this member, if applicable.
    pub invited_by: Option<UserId>,
    pub created_at: DateTime<Utc>,
}
