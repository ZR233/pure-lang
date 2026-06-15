CREATE TABLE IF NOT EXISTS studio_messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    role TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,
    error TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    sequence INTEGER NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_studio_messages_session_created
    ON studio_messages(session_id, created_at ASC, sequence ASC, id ASC);

CREATE INDEX IF NOT EXISTS idx_studio_messages_turn
    ON studio_messages(turn_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS message_parts (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    part_type TEXT NOT NULL,
    part_order INTEGER NOT NULL,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,
    error TEXT,
    text_channel TEXT,
    text TEXT NOT NULL DEFAULT '',
    attachments_json TEXT NOT NULL DEFAULT '[]',
    tool_json TEXT,
    agent_json TEXT,
    inference_json TEXT,
    plan_json TEXT,
    file_json TEXT,
    usage_json TEXT,
    synthetic INTEGER NOT NULL DEFAULT 0,
    ignored INTEGER NOT NULL DEFAULT 0,
    sequence INTEGER NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_message_parts_session_order
    ON message_parts(session_id, part_order ASC, sequence ASC, id ASC);

CREATE INDEX IF NOT EXISTS idx_message_parts_message_order
    ON message_parts(message_id, part_order ASC, sequence ASC, id ASC);

CREATE INDEX IF NOT EXISTS idx_message_parts_turn
    ON message_parts(turn_id, part_order ASC);

DELETE FROM message_parts;
DELETE FROM studio_messages;
DELETE FROM studio_events;
DELETE FROM timeline_events;
DELETE FROM trace_events;
DELETE FROM messages;
DELETE FROM turns;
DELETE FROM interactions;
DELETE FROM tool_approvals;
DELETE FROM agents;
DELETE FROM agent_events;
DELETE FROM agent_runtime_events;
DELETE FROM agent_runtime_snapshots;
DELETE FROM session_runtime_snapshots;
