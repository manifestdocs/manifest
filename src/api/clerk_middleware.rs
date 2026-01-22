//! Clerk authentication middleware for Axum.
//!
//! This middleware:
//! 1. Extracts JWT from Authorization header or __session cookie
//! 2. Verifies the token using Clerk's JWKS
//! 3. Syncs user profile to local database
//! 4. Injects AuthenticatedUser into request extensions

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use super::clerk::{ClerkClaims, ClerkError, ClerkVerifier};
use manifest_core::db::Database;

/// Authenticated user context available in request extensions.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    /// Our internal user ID (UUID)
    pub id: Uuid,
    /// Clerk's user ID (e.g., "user_abc123")
    pub clerk_id: String,
    /// User's email address
    pub email: String,
    /// Display name
    pub display_name: Option<String>,
    /// Avatar URL
    pub avatar_url: Option<String>,
}

/// State required for Clerk authentication middleware.
#[derive(Clone)]
pub struct ClerkAuthState {
    pub verifier: ClerkVerifier,
    pub db: Database,
}

/// Clerk authentication middleware.
///
/// This middleware verifies Clerk JWTs and syncs user data to the local database.
/// If verification fails, returns 401 Unauthorized.
pub async fn clerk_auth_middleware(
    State(state): State<ClerkAuthState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract token from Authorization header or __session cookie
    let token = extract_token(&request);

    let token = match token {
        Some(t) => t,
        None => {
            tracing::debug!("No authentication token found");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    // Verify the token
    let claims = match state.verifier.verify(&token).await {
        Ok(claims) => claims,
        Err(e) => {
            tracing::warn!("Token verification failed: {}", e);
            return Err(match e {
                ClerkError::TokenExpired => StatusCode::UNAUTHORIZED,
                ClerkError::InvalidAudience { .. } => StatusCode::FORBIDDEN,
                _ => StatusCode::UNAUTHORIZED,
            });
        }
    };

    // Sync user to local database
    let user = match sync_user(&state.db, &claims).await {
        Ok(user) => user,
        Err(e) => {
            tracing::error!("Failed to sync user: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    // Inject authenticated user into request extensions
    request.extensions_mut().insert(user);

    Ok(next.run(request).await)
}

/// Optional Clerk authentication middleware.
///
/// Same as `clerk_auth_middleware`, but allows unauthenticated requests.
/// If a valid token is present, the user is authenticated; otherwise, the request proceeds
/// without authentication. Use this for endpoints that have different behavior for
/// authenticated vs unauthenticated users.
pub async fn optional_clerk_auth_middleware(
    State(state): State<ClerkAuthState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    // Extract token from Authorization header or __session cookie
    if let Some(token) = extract_token(&request) {
        // Try to verify the token
        if let Ok(claims) = state.verifier.verify(&token).await {
            // Try to sync user
            if let Ok(user) = sync_user(&state.db, &claims).await {
                request.extensions_mut().insert(user);
            }
        }
    }

    next.run(request).await
}

/// Extract JWT from Authorization header or __session cookie.
fn extract_token(request: &Request<Body>) -> Option<String> {
    // Try Authorization header first (Bearer token)
    if let Some(auth_header) = request.headers().get("Authorization") {
        if let Ok(value) = auth_header.to_str() {
            if let Some(token) = value.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
        }
    }

    // Fall back to __session cookie (Clerk's default cookie name)
    if let Some(cookie_header) = request.headers().get("Cookie") {
        if let Ok(cookies) = cookie_header.to_str() {
            for cookie in cookies.split(';') {
                let cookie = cookie.trim();
                if let Some(token) = cookie.strip_prefix("__session=") {
                    return Some(token.to_string());
                }
            }
        }
    }

    None
}

/// Sync user from Clerk claims to local database.
async fn sync_user(db: &Database, claims: &ClerkClaims) -> anyhow::Result<AuthenticatedUser> {
    // Look up existing user by Clerk ID
    let existing_user = db.get_user_by_clerk_id(&claims.sub).await?;

    let (user_id, email) = match existing_user {
        Some(user) => {
            // Update user profile if changed
            let display_name = claims.display_name();
            let avatar_url = claims.image_url.clone();

            if user.display_name.as_deref() != Some(&display_name) || user.avatar_url != avatar_url
            {
                db.update_user(user.id, Some(&display_name), avatar_url.as_deref())
                    .await?;
            }

            (user.id, user.email)
        }
        None => {
            // Create new user
            let user_id = Uuid::new_v4();
            let email = claims
                .email
                .clone()
                .unwrap_or_else(|| format!("{}@clerk.user", claims.sub));
            let display_name = claims.display_name();

            db.create_user(
                user_id,
                &email,
                Some(&display_name),
                claims.image_url.as_deref(),
            )
            .await?;

            // Create OAuth identity linking Clerk ID to our user
            db.create_oauth_identity(
                Uuid::new_v4(),
                user_id,
                "clerk",
                &claims.sub,
                claims.email.as_deref(),
            )
            .await?;

            (user_id, email)
        }
    };

    Ok(AuthenticatedUser {
        id: user_id,
        clerk_id: claims.sub.clone(),
        email,
        display_name: Some(claims.display_name()),
        avatar_url: claims.image_url.clone(),
    })
}

/// Error response for authentication failures.
pub struct AuthError {
    status: StatusCode,
    message: String,
}

impl AuthError {
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}
