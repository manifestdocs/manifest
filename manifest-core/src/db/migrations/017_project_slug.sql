-- Add slug field to projects for human-readable URLs
-- Slug is auto-generated from name on creation, must be unique

-- Add slug column
ALTER TABLE projects ADD COLUMN slug TEXT;

-- Generate slugs for existing projects from their names
-- Convert to lowercase, replace non-alphanumeric with hyphens, collapse multiple hyphens
UPDATE projects SET slug = LOWER(REPLACE(REPLACE(REPLACE(REPLACE(name, ' ', '-'), '_', '-'), '.', '-'), '--', '-'));

-- Handle potential duplicates by appending row number
WITH duplicates AS (
    SELECT id, slug, ROW_NUMBER() OVER (PARTITION BY slug ORDER BY created_at) as rn
    FROM projects
)
UPDATE projects SET slug = slug || '-' || (
    SELECT rn FROM duplicates WHERE duplicates.id = projects.id
)
WHERE id IN (SELECT id FROM duplicates WHERE rn > 1);

-- Now make it NOT NULL and UNIQUE
-- SQLite requires table recreation for adding NOT NULL constraint
CREATE TABLE projects_new (
    id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    description TEXT,
    instructions TEXT,
    current_version_id TEXT,
    root_feature_id TEXT,
    owner_id TEXT,
    visibility TEXT NOT NULL DEFAULT 'private',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CONSTRAINT chk_projects_visibility CHECK (visibility IN ('private', 'public'))
);

INSERT INTO projects_new (id, slug, name, description, instructions, current_version_id, root_feature_id, owner_id, visibility, created_at, updated_at)
SELECT id, slug, name, description, instructions, current_version_id, root_feature_id, owner_id, visibility, created_at, updated_at
FROM projects;

DROP TABLE projects;
ALTER TABLE projects_new RENAME TO projects;

-- Recreate indexes
CREATE INDEX idx_projects_root_feature ON projects(root_feature_id);
CREATE INDEX idx_projects_slug ON projects(slug);
