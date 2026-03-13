-- Manifest Initial Schema
-- Compatible with SQLite and PostgreSQL via SQLx Any driver
-- All UUIDs stored as TEXT, all timestamps stored as TEXT (RFC3339)

-- ============================================================
-- Core Entities
-- ============================================================

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    email_verified_at TEXT,
    display_name TEXT,
    avatar_url TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT,
    instructions TEXT,
    current_version_id TEXT,
    root_feature_id TEXT,
    owner_id TEXT,
    visibility TEXT NOT NULL DEFAULT 'private',
    default_feature_destination TEXT NOT NULL DEFAULT 'backlog',
    testing_policy TEXT NOT NULL DEFAULT 'advisory',
    test_adapter TEXT,
    key_prefix TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT chk_projects_visibility CHECK (visibility IN ('private', 'public'))
);

CREATE INDEX IF NOT EXISTS idx_projects_root_feature ON projects(root_feature_id);
CREATE INDEX IF NOT EXISTS idx_projects_slug ON projects(slug);

CREATE TABLE IF NOT EXISTS spec_templates (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    content TEXT NOT NULL,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT fk_spec_templates_project FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    UNIQUE(project_id, name)
);

CREATE INDEX IF NOT EXISTS idx_spec_templates_project ON spec_templates(project_id);

CREATE TABLE IF NOT EXISTS project_directories (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    path TEXT NOT NULL,
    git_remote TEXT,
    is_primary INTEGER NOT NULL DEFAULT 0,
    instructions TEXT,
    created_at TEXT NOT NULL,
    CONSTRAINT fk_project_directories_project FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_project_directories_project ON project_directories(project_id);

CREATE TABLE IF NOT EXISTS versions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    released_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT fk_versions_project FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    UNIQUE(project_id, name)
);

CREATE INDEX IF NOT EXISTS idx_versions_project ON versions(project_id);

CREATE TABLE IF NOT EXISTS features (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    parent_id TEXT,
    title TEXT NOT NULL,
    details TEXT,
    desired_details TEXT,
    details_summary TEXT,
    state TEXT NOT NULL DEFAULT 'proposed',
    priority INTEGER NOT NULL DEFAULT 0,
    feature_number INTEGER,
    target_version_id TEXT,
    verification_result TEXT,
    verified_at TEXT,
    claimed_by TEXT,
    claimed_at TEXT,
    claim_metadata TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT fk_features_project FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    CONSTRAINT fk_features_parent FOREIGN KEY (parent_id) REFERENCES features(id) ON DELETE CASCADE,
    CONSTRAINT fk_features_version FOREIGN KEY (target_version_id) REFERENCES versions(id) ON DELETE SET NULL,
    CONSTRAINT chk_features_state CHECK (state IN ('proposed', 'blocked', 'in_progress', 'implemented', 'archived'))
);

CREATE INDEX IF NOT EXISTS idx_features_project ON features(project_id);
CREATE INDEX IF NOT EXISTS idx_features_parent ON features(parent_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_features_number ON features(project_id, feature_number);

-- FTS5 full-text search index is created by migrate_add_features_fts()
-- because triggers contain semicolons that break the schema splitter.

CREATE TABLE IF NOT EXISTS feature_blockers (
    feature_id TEXT NOT NULL,
    blocker_feature_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (feature_id, blocker_feature_id),
    CONSTRAINT fk_feature_blockers_feature FOREIGN KEY (feature_id) REFERENCES features(id) ON DELETE CASCADE,
    CONSTRAINT fk_feature_blockers_blocker FOREIGN KEY (blocker_feature_id) REFERENCES features(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS feature_history (
    id TEXT PRIMARY KEY,
    feature_id TEXT,
    version_id TEXT,
    summary TEXT NOT NULL,
    details TEXT,
    created_at TEXT NOT NULL,
    CONSTRAINT fk_history_feature FOREIGN KEY (feature_id) REFERENCES features(id) ON DELETE CASCADE,
    CONSTRAINT fk_history_version FOREIGN KEY (version_id) REFERENCES versions(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_history_feature ON feature_history(feature_id);
CREATE INDEX IF NOT EXISTS idx_history_created ON feature_history(created_at DESC);

-- ============================================================
-- Proofs (test evidence for features)
-- ============================================================

CREATE TABLE IF NOT EXISTS proofs (
    id TEXT PRIMARY KEY,
    feature_id TEXT NOT NULL,
    history_id TEXT,
    command TEXT NOT NULL,
    exit_code INTEGER NOT NULL,
    output TEXT,
    tests TEXT,
    evidence TEXT,
    commit_sha TEXT,
    agent_type TEXT,
    created_at TEXT NOT NULL,
    CONSTRAINT fk_proofs_feature FOREIGN KEY (feature_id) REFERENCES features(id) ON DELETE CASCADE,
    CONSTRAINT fk_proofs_history FOREIGN KEY (history_id) REFERENCES feature_history(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_proofs_feature ON proofs(feature_id);
CREATE INDEX IF NOT EXISTS idx_proofs_history ON proofs(history_id);

-- ============================================================
-- App Focus (tracks which feature is focused in the desktop app)
-- ============================================================

CREATE TABLE IF NOT EXISTS project_focus (
    project_id TEXT PRIMARY KEY,
    feature_id TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT fk_focus_project FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    CONSTRAINT fk_focus_feature FOREIGN KEY (feature_id) REFERENCES features(id) ON DELETE CASCADE
);

-- ============================================================
-- Authentication & Authorization
-- ============================================================

CREATE TABLE IF NOT EXISTS oauth_identities (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    provider_user_id TEXT NOT NULL,
    provider_email TEXT,
    access_token TEXT,
    refresh_token TEXT,
    token_expires_at TEXT,
    created_at TEXT NOT NULL,
    CONSTRAINT fk_oauth_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    UNIQUE(provider, provider_user_id)
);

CREATE INDEX IF NOT EXISTS idx_oauth_user ON oauth_identities(user_id);
CREATE INDEX IF NOT EXISTS idx_oauth_provider ON oauth_identities(provider, provider_user_id);

CREATE TABLE IF NOT EXISTS user_sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    user_agent TEXT,
    ip_address TEXT,
    expires_at TEXT NOT NULL,
    last_activity_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    CONSTRAINT fk_sessions_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_sessions_user ON user_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_expires ON user_sessions(expires_at);

CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL,
    key_prefix TEXT NOT NULL,
    scopes TEXT,
    last_used_at TEXT,
    expires_at TEXT,
    revoked_at TEXT,
    created_at TEXT NOT NULL,
    CONSTRAINT fk_api_keys_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_prefix ON api_keys(key_prefix);

CREATE TABLE IF NOT EXISTS revoked_tokens (
    jti TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    revoked_at TEXT NOT NULL,
    reason TEXT,
    CONSTRAINT fk_revoked_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_revoked_user ON revoked_tokens(user_id);
CREATE INDEX IF NOT EXISTS idx_revoked_expires ON revoked_tokens(expires_at);

CREATE TABLE IF NOT EXISTS project_memberships (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    role TEXT NOT NULL,
    invited_by TEXT,
    created_at TEXT NOT NULL,
    CONSTRAINT fk_memberships_project FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    CONSTRAINT fk_memberships_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    CONSTRAINT fk_memberships_invited_by FOREIGN KEY (invited_by) REFERENCES users(id),
    CONSTRAINT chk_memberships_role CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    UNIQUE(project_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_memberships_project ON project_memberships(project_id);
CREATE INDEX IF NOT EXISTS idx_memberships_user ON project_memberships(user_id);

CREATE TABLE IF NOT EXISTS project_invitations (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    email TEXT NOT NULL,
    role TEXT NOT NULL,
    invited_by TEXT NOT NULL,
    token TEXT NOT NULL UNIQUE,
    expires_at TEXT NOT NULL,
    accepted_at TEXT,
    created_at TEXT NOT NULL,
    CONSTRAINT fk_invitations_project FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    CONSTRAINT fk_invitations_invited_by FOREIGN KEY (invited_by) REFERENCES users(id),
    CONSTRAINT chk_invitations_role CHECK (role IN ('admin', 'member', 'viewer'))
);

CREATE INDEX IF NOT EXISTS idx_invitations_project ON project_invitations(project_id);
CREATE INDEX IF NOT EXISTS idx_invitations_email ON project_invitations(email);
CREATE INDEX IF NOT EXISTS idx_invitations_token ON project_invitations(token);

-- ============================================================
-- Audit Logging
-- ============================================================

CREATE TABLE IF NOT EXISTS audit_log (
    id TEXT PRIMARY KEY,
    user_id TEXT,
    project_id TEXT,
    action TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    resource_id TEXT,
    details TEXT,
    ip_address TEXT,
    user_agent TEXT,
    success INTEGER NOT NULL DEFAULT 1,
    error_message TEXT,
    created_at TEXT NOT NULL,
    CONSTRAINT fk_audit_user FOREIGN KEY (user_id) REFERENCES users(id),
    CONSTRAINT fk_audit_project FOREIGN KEY (project_id) REFERENCES projects(id)
);

CREATE INDEX IF NOT EXISTS idx_audit_user ON audit_log(user_id);
CREATE INDEX IF NOT EXISTS idx_audit_project ON audit_log(project_id);
CREATE INDEX IF NOT EXISTS idx_audit_action ON audit_log(action);
CREATE INDEX IF NOT EXISTS idx_audit_resource ON audit_log(resource_type, resource_id);
CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_log(created_at);
CREATE INDEX IF NOT EXISTS idx_audit_user_time ON audit_log(user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_audit_project_time ON audit_log(project_id, created_at);

-- ============================================================
-- Schema Migrations Tracking (for compatibility with existing DBs)
-- ============================================================

CREATE TABLE IF NOT EXISTS _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    success INTEGER NOT NULL,
    checksum TEXT NOT NULL,
    execution_time BIGINT NOT NULL
);
