CREATE TABLE IF NOT EXISTS studio_events (
    id TEXT PRIMARY KEY,
    project_id TEXT,
    session_id TEXT,
    turn_id TEXT,
    sequence INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_studio_events_session_sequence
    ON studio_events(session_id, sequence)
    WHERE session_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_studio_events_project_sequence
    ON studio_events(project_id, sequence)
    WHERE project_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_studio_events_created
    ON studio_events(created_at ASC, id ASC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_timeline_events_session_sequence_unique
    ON timeline_events(session_id, sequence);

CREATE TABLE IF NOT EXISTS turns (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    status TEXT NOT NULL,
    reason TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_turns_session_updated
    ON turns(session_id, updated_at DESC, id DESC);

CREATE INDEX IF NOT EXISTS idx_turns_status
    ON turns(status, updated_at DESC);
