-- Version entity for release planning
CREATE TABLE versions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    released_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(project_id, name)
);

CREATE INDEX idx_versions_project ON versions(project_id);

-- Project tracks current version
ALTER TABLE projects ADD COLUMN current_version_id TEXT REFERENCES versions(id) ON DELETE SET NULL;

-- Features can target a version
ALTER TABLE features ADD COLUMN target_version_id TEXT REFERENCES versions(id) ON DELETE SET NULL;

CREATE INDEX idx_features_target_version ON features(target_version_id) WHERE target_version_id IS NOT NULL;
