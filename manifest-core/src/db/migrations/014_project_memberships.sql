-- Migration 014: Project memberships and authorization
-- Adds owner tracking to projects and membership roles.

-- Add owner_id to projects (nullable for backwards compatibility with local mode)
ALTER TABLE projects ADD COLUMN owner_id TEXT REFERENCES users(id);

-- Add visibility for public/private projects
ALTER TABLE projects ADD COLUMN visibility TEXT NOT NULL DEFAULT 'private'
    CHECK (visibility IN ('private', 'public'));

-- Project memberships for multi-user access
CREATE TABLE project_memberships (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'viewer')),
    invited_by TEXT REFERENCES users(id),
    created_at TEXT NOT NULL,
    UNIQUE(project_id, user_id)
);

CREATE INDEX idx_memberships_project ON project_memberships(project_id);
CREATE INDEX idx_memberships_user ON project_memberships(user_id);

-- Pending invitations for users not yet registered
CREATE TABLE project_invitations (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    email TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member', 'viewer')),
    invited_by TEXT NOT NULL REFERENCES users(id),
    token TEXT NOT NULL UNIQUE,  -- Unique token for accepting invitation
    expires_at TEXT NOT NULL,
    accepted_at TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX idx_invitations_project ON project_invitations(project_id);
CREATE INDEX idx_invitations_email ON project_invitations(email);
CREATE INDEX idx_invitations_token ON project_invitations(token);
