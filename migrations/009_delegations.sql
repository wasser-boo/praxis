-- One-level task delegation records (parent -> child agent loops)
CREATE TABLE IF NOT EXISTS delegations (
    id TEXT PRIMARY KEY,
    parent_user_id TEXT NOT NULL,
    child_user_id TEXT NOT NULL,
    task TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'running',
    result TEXT,
    created_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_delegations_parent ON delegations(parent_user_id);
