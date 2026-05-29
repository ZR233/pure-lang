CREATE TABLE timeline_events (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX idx_timeline_events_session_sequence
    ON timeline_events(session_id, sequence ASC);

CREATE INDEX idx_timeline_events_session_created
    ON timeline_events(session_id, created_at ASC);
