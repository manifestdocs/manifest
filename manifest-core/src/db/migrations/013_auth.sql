-- Migration 013: Authentication tables
-- Adds users, OAuth identities, sessions, and API keys for cloud mode.

-- Users (identity, not credentials - OAuth-only)
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    email_verified_at TEXT,
    display_name TEXT,
    avatar_url TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_users_email ON users(email);

-- OAuth identities (multiple providers per user)
CREATE TABLE oauth_identities (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,  -- 'google', 'github'
    provider_user_id TEXT NOT NULL,
    provider_email TEXT,
    access_token TEXT,  -- Encrypted, for API access on behalf of user
    refresh_token TEXT, -- Encrypted, for token refresh
    token_expires_at TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(provider, provider_user_id)
);

CREATE INDEX idx_oauth_user ON oauth_identities(user_id);
CREATE INDEX idx_oauth_provider ON oauth_identities(provider, provider_user_id);

-- User sessions (web auth via cookies)
CREATE TABLE user_sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_agent TEXT,
    ip_address TEXT,
    expires_at TEXT NOT NULL,
    last_activity_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_sessions_user ON user_sessions(user_id);
CREATE INDEX idx_sessions_expires ON user_sessions(expires_at);

-- API keys (programmatic access)
CREATE TABLE api_keys (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL,      -- Argon2 hash of the full key
    key_prefix TEXT NOT NULL,    -- First 8 chars for identification
    scopes TEXT,                 -- JSON array of allowed scopes
    last_used_at TEXT,
    expires_at TEXT,             -- NULL means no expiration
    revoked_at TEXT,             -- NULL means active
    created_at TEXT NOT NULL
);

CREATE INDEX idx_api_keys_user ON api_keys(user_id);
CREATE INDEX idx_api_keys_prefix ON api_keys(key_prefix);

-- JWT revocation list (for logout, compromised tokens)
CREATE TABLE revoked_tokens (
    jti TEXT PRIMARY KEY,        -- JWT ID
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TEXT NOT NULL,    -- When the token would have expired (cleanup)
    revoked_at TEXT NOT NULL,
    reason TEXT                  -- 'logout', 'password_change', 'compromised'
);

CREATE INDEX idx_revoked_user ON revoked_tokens(user_id);
CREATE INDEX idx_revoked_expires ON revoked_tokens(expires_at);
