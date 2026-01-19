-- Add root_feature_id to projects for hierarchical feature organization
ALTER TABLE projects ADD COLUMN root_feature_id TEXT REFERENCES features(id) ON DELETE SET NULL;
CREATE INDEX idx_projects_root_feature ON projects(root_feature_id) WHERE root_feature_id IS NOT NULL;
