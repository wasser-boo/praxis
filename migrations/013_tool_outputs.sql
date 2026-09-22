-- Private saved tool responses, independent of compacted prompt history.
-- owner_id and session_id are separate columns, never a caller-supplied scope.
CREATE TABLE IF NOT EXISTS tool_outputs (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    owner_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    call_id TEXT NOT NULL,
    content TEXT NOT NULL,
    retained_bytes INTEGER NOT NULL CHECK(retained_bytes >= 0),
    source_bytes INTEGER NOT NULL CHECK(source_bytes >= retained_bytes),
    source_chars INTEGER NOT NULL CHECK(source_chars >= 0),
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS tool_outputs_scope ON tool_outputs(owner_id, session_id, id);
CREATE INDEX IF NOT EXISTS tool_outputs_age ON tool_outputs(created_at);
