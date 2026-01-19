-- Rename 'specified' state to 'in_progress' for clarity
-- The state name now reflects work status rather than documentation completeness

-- Step 1: Update existing data
UPDATE features SET state = 'in_progress' WHERE state = 'specified';

-- Step 2: Create new table with updated CHECK constraint
CREATE TABLE features_new (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES features_new(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    details TEXT,
    desired_details TEXT,
    state TEXT NOT NULL DEFAULT 'proposed' CHECK (state IN ('proposed', 'in_progress', 'implemented', 'deprecated')),
    priority INTEGER NOT NULL DEFAULT 0,
    target_version_id TEXT REFERENCES versions(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Step 3: Copy data
INSERT INTO features_new (id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at)
SELECT id, project_id, parent_id, title, details, desired_details, state, priority, target_version_id, created_at, updated_at
FROM features;

-- Step 4: Drop old table and rename new
DROP TABLE features;
ALTER TABLE features_new RENAME TO features;

-- Step 5: Recreate indexes
CREATE INDEX idx_features_project ON features(project_id);
CREATE INDEX idx_features_parent ON features(parent_id);
CREATE INDEX idx_features_target_version ON features(target_version_id) WHERE target_version_id IS NOT NULL;
