//! User and OAuth identity models for authentication.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A user account in the system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub email_verified_at: Option<DateTime<Utc>>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// An OAuth identity linked to a user account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthIdentity {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: String,
    pub provider_user_id: String,
    pub provider_email: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub token_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Input for creating a new user.
#[derive(Debug, Clone)]
pub struct CreateUserInput {
    pub id: Uuid,
    pub email: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

/// Input for updating an existing user.
#[derive(Debug, Clone, Default)]
pub struct UpdateUserInput {
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub email_verified_at: Option<DateTime<Utc>>,
}

/// Input for creating an OAuth identity.
#[derive(Debug, Clone)]
pub struct CreateOAuthIdentityInput {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: String,
    pub provider_user_id: String,
    pub provider_email: Option<String>,
}
