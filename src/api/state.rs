//! Application state for request handling.
//!
//! This module provides:
//! - `AppState`: Shared application state containing the database
//! - `CurrentUser`: A synthetic local user (authentication is optional in self-hosted mode)

use axum::extract::FromRef;
use std::sync::LazyLock;
use uuid::Uuid;

use crate::db::Database;

/// Stable synthetic user ID for local mode.
/// This ensures consistent behavior when no authentication is configured.
static LOCAL_USER_ID: LazyLock<Uuid> = LazyLock::new(|| {
    // Use a stable UUID derived from a fixed seed so it's consistent across restarts
    Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap()
});

/// Shared application state for all handlers.
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool.
    pub db: Database,
}

impl AppState {
    /// Create application state.
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

// Allow extracting Database from AppState for backward compatibility
impl FromRef<AppState> for Database {
    fn from_ref(state: &AppState) -> Self {
        state.db.clone()
    }
}

/// User context for handlers.
///
/// In self-hosted mode, this contains a synthetic user with a stable ID.
/// If API key auth is configured, requests are protected but user identity
/// is still synthetic (suitable for single-user or team deployments).
#[derive(Debug, Clone)]
pub struct CurrentUser {
    /// User's internal UUID.
    pub id: Uuid,
    /// User's email (synthetic in local mode).
    pub email: String,
    /// Display name.
    pub display_name: Option<String>,
}

impl CurrentUser {
    /// Create a synthetic local user.
    pub fn local_user() -> Self {
        Self {
            id: *LOCAL_USER_ID,
            email: "local@localhost".to_string(),
            display_name: Some("Local User".to_string()),
        }
    }
}

impl Default for CurrentUser {
    fn default() -> Self {
        Self::local_user()
    }
}
