-- Add 'copilot' to the agent_type CHECK constraint on tasks table.
-- SQLite doesn't support ALTER CHECK, so we recreate the table.
-- The tasks table is ephemeral (rows deleted when sessions complete).

CREATE TABLE tasks_new (
    id TEXT PRIMARY KEY,
    session_id TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES tasks_new(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    scope TEXT,
    status TEXT DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'completed', 'failed')),
    agent_type TEXT CHECK (agent_type IN ('claude', 'gemini', 'codex', 'copilot')),
    worktree_path TEXT,
    branch TEXT,
    created_at TEXT NOT NULL
);

INSERT INTO tasks_new SELECT * FROM tasks;
DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;

CREATE INDEX idx_tasks_session ON tasks(session_id);
CREATE INDEX idx_tasks_parent ON tasks(parent_id);
