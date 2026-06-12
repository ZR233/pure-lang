CREATE TABLE interactions (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    item_id TEXT,
    tool_id TEXT,
    agent_path TEXT,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    resolution_json TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    resolved_at INTEGER,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX idx_interactions_session_status_updated
    ON interactions(session_id, status, updated_at DESC);

CREATE INDEX idx_interactions_session_turn
    ON interactions(session_id, turn_id);
