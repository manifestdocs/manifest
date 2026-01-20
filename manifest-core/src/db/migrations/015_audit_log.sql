-- Migration 015: Audit logging for security and compliance
-- Records all security-relevant actions.

CREATE TABLE audit_log (
    id TEXT PRIMARY KEY,
    user_id TEXT REFERENCES users(id),  -- NULL for unauthenticated actions
    project_id TEXT REFERENCES projects(id),  -- NULL for non-project actions
    action TEXT NOT NULL,        -- create, update, delete, share, login, logout, etc.
    resource_type TEXT NOT NULL, -- project, feature, version, user, api_key, session
    resource_id TEXT,            -- ID of the affected resource
    details TEXT,                -- JSON with additional context
    ip_address TEXT,
    user_agent TEXT,
    success INTEGER NOT NULL DEFAULT 1,  -- 1 = success, 0 = failure
    error_message TEXT,          -- Error details if success = 0
    created_at TEXT NOT NULL
);

-- Indexes for common queries
CREATE INDEX idx_audit_user ON audit_log(user_id);
CREATE INDEX idx_audit_project ON audit_log(project_id);
CREATE INDEX idx_audit_action ON audit_log(action);
CREATE INDEX idx_audit_resource ON audit_log(resource_type, resource_id);
CREATE INDEX idx_audit_time ON audit_log(created_at);

-- Composite index for user activity queries
CREATE INDEX idx_audit_user_time ON audit_log(user_id, created_at);

-- Composite index for project activity queries
CREATE INDEX idx_audit_project_time ON audit_log(project_id, created_at);
