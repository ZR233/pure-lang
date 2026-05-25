CREATE TABLE subagent_events (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    subagent_id TEXT NOT NULL,
    parent_id TEXT,
    role TEXT NOT NULL,
    task TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT,
    depth INTEGER NOT NULL,
    error TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX idx_subagent_events_session_created_at
    ON subagent_events(session_id, created_at ASC, id ASC);

CREATE INDEX idx_subagent_events_subagent_created_at
    ON subagent_events(subagent_id, created_at ASC, id ASC);
