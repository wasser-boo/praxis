CREATE TABLE IF NOT EXISTS cron_jobs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    schedule TEXT NOT NULL,
    timezone TEXT DEFAULT 'UTC',
    user_id TEXT NOT NULL,
    channel_id TEXT,
    template TEXT NOT NULL,
    prompt TEXT NOT NULL,
    context_overrides TEXT,
    enabled INTEGER DEFAULT 1,
    trigger_type TEXT DEFAULT 'cron',
    webhook_secret TEXT,
    event_type TEXT,
    last_run TEXT,
    next_run TEXT,
    run_count INTEGER DEFAULT 0,
    last_error TEXT,
    created_at TEXT DEFAULT (datetime('now')),
    updated_at TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_cron_next_run ON cron_jobs(next_run) WHERE enabled = 1;
CREATE INDEX IF NOT EXISTS idx_cron_user ON cron_jobs(user_id);
