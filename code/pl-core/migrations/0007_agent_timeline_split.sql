DROP TABLE IF EXISTS agent_events;
DROP TABLE IF EXISTS agents;

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
    reason TEXT,
    budget_limit_kind TEXT,
    budget_usage_json TEXT,
    depth INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_agents_session_path
    ON agents(session_id, path);

CREATE TABLE agent_events (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    kind TEXT NOT NULL,
    agent_id TEXT,
    path TEXT,
    parent_path TEXT,
    payload_json TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_agent_events_session_sequence
    ON agent_events(session_id, sequence);

CREATE INDEX idx_agent_events_session_created_at
    ON agent_events(session_id, created_at ASC, sequence ASC);

CREATE INDEX idx_agent_events_agent_created_at
    ON agent_events(agent_id, created_at ASC, sequence ASC);
