-- Keep the existing memory table/IDs untouched as the legacy standard profile.
CREATE TABLE IF NOT EXISTS memory_profiles (
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    data TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (user_id, name)
);
CREATE TABLE IF NOT EXISTS memory_profile_selections (
    user_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    mode TEXT NOT NULL,
    profile TEXT NOT NULL,
    PRIMARY KEY (user_id, session_id, mode)
);
