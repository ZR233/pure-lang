ALTER TABLE sessions
    ADD COLUMN visibility TEXT NOT NULL DEFAULT 'active';

UPDATE sessions
    SET visibility = CASE WHEN archived = 1 THEN 'archived' ELSE 'active' END;

CREATE TABLE IF NOT EXISTS session_handoffs (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    origin_session_id TEXT NOT NULL,
    target_session_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    plan_id TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(origin_session_id, plan_id, kind),
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY(origin_session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY(target_session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_session_handoffs_project_updated
    ON session_handoffs(project_id, updated_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_session_handoffs_target
    ON session_handoffs(target_session_id);
