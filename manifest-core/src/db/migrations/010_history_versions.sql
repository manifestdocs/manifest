-- Add version tracking to feature_history, remove session_id, and clean up session/task tables
-- Note: Migration 008 lost idx_history_feature when recreating the table - we restore it here

-- Disable FK checks during migration (re-enabled at end of transaction)
PRAGMA foreign_keys = OFF;

-- Recreate feature_history with version_id and without session_id
CREATE TABLE feature_history_new (
    id TEXT PRIMARY KEY,
    feature_id TEXT REFERENCES features(id) ON DELETE CASCADE,
    version_id TEXT REFERENCES versions(id) ON DELETE SET NULL,
    summary TEXT NOT NULL,
    details JSON,
    created_at TEXT NOT NULL
);

-- Copy data from old table (dropping session_id, files_changed, author)
-- Only copy rows where feature still exists to maintain referential integrity
INSERT INTO feature_history_new (id, feature_id, summary, details, created_at)
SELECT h.id, h.feature_id, h.summary, h.details, h.created_at
FROM feature_history h
WHERE EXISTS (SELECT 1 FROM features f WHERE f.id = h.feature_id);

-- Drop old table and rename new one
DROP TABLE feature_history;
ALTER TABLE feature_history_new RENAME TO feature_history;

-- Recreate index lost in migration 008 + new indexes
CREATE INDEX idx_history_feature ON feature_history(feature_id);
CREATE INDEX idx_history_version ON feature_history(version_id) WHERE version_id IS NOT NULL;
CREATE INDEX idx_history_created ON feature_history(created_at DESC);

-- Drop session and task tables (CLI agents handle their own work tracking)
DROP TABLE IF EXISTS implementation_notes;
DROP TABLE IF EXISTS tasks;
DROP TABLE IF EXISTS sessions;

-- Re-enable FK checks
PRAGMA foreign_keys = ON;
