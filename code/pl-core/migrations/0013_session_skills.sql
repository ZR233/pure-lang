CREATE TABLE IF NOT EXISTS session_skills (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    skill_name TEXT NOT NULL,
    skill_name_key TEXT NOT NULL,
    source TEXT NOT NULL,
    path TEXT NOT NULL,
    first_turn_id TEXT NOT NULL,
    last_turn_id TEXT NOT NULL,
    last_tool_call_id TEXT NOT NULL,
    activated_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(session_id, skill_name_key),
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_session_skills_session_updated_at
    ON session_skills(session_id, updated_at DESC, skill_name_key ASC);
