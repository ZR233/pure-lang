CREATE TABLE agents (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    path TEXT NOT NULL,
    parent_path TEXT,
    role TEXT NOT NULL,
    task TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT,
    error TEXT,
    depth INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_agents_session_path
    ON agents(session_id, path);

CREATE TABLE agent_events (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    path TEXT NOT NULL,
    parent_path TEXT,
    role TEXT NOT NULL,
    task TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT,
    error TEXT,
    depth INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_agent_events_session_created_at
    ON agent_events(session_id, created_at ASC, id ASC);

CREATE INDEX idx_agent_events_agent_created_at
    ON agent_events(agent_id, created_at ASC, id ASC);

CREATE TABLE agent_messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    sender_path TEXT NOT NULL,
    message TEXT NOT NULL,
    trigger_turn INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX idx_agent_messages_agent_created_at
    ON agent_messages(agent_id, created_at ASC, id ASC);

CREATE TABLE agent_turns (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    status TEXT NOT NULL,
    summary TEXT,
    error TEXT,
    started_at INTEGER NOT NULL,
    completed_at INTEGER
);

CREATE INDEX idx_agent_turns_agent_started_at
    ON agent_turns(agent_id, started_at ASC, id ASC);
