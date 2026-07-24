ALTER TABLE sessions ADD COLUMN root_session_id TEXT;
ALTER TABLE sessions ADD COLUMN session_kind TEXT NOT NULL DEFAULT 'root';
ALTER TABLE sessions ADD COLUMN owner_agent_id TEXT;
ALTER TABLE sessions ADD COLUMN owner_role TEXT NOT NULL DEFAULT 'planner';
ALTER TABLE sessions ADD COLUMN agent_status TEXT NOT NULL DEFAULT 'idle';
ALTER TABLE sessions ADD COLUMN agent_summary TEXT;
ALTER TABLE sessions ADD COLUMN agent_error TEXT;
ALTER TABLE sessions ADD COLUMN agent_updated_at INTEGER;

UPDATE sessions
SET root_session_id = id,
    session_kind = 'root',
    owner_agent_id = 'studio:' || id,
    owner_role = 'planner',
    agent_updated_at = updated_at
WHERE root_session_id IS NULL OR owner_agent_id IS NULL;

CREATE INDEX idx_sessions_root_session
    ON sessions(root_session_id, created_at, id);

CREATE UNIQUE INDEX idx_sessions_owner_runtime_session
    ON sessions(owner_agent_id, id);

PRAGMA user_version = 3;
