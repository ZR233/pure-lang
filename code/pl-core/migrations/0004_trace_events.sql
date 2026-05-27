CREATE TABLE trace_events (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX idx_trace_events_session_sequence
    ON trace_events(session_id, sequence ASC);

CREATE INDEX idx_trace_events_session_timestamp
    ON trace_events(session_id, timestamp ASC);
