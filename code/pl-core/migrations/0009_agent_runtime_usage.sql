ALTER TABLE session_runtime_snapshots
    ADD COLUMN estimated_costs_json TEXT NOT NULL DEFAULT '[]';

ALTER TABLE session_runtime_snapshots
    ADD COLUMN has_unpriced_usage INTEGER NOT NULL DEFAULT 0;

UPDATE session_runtime_snapshots
SET estimated_costs_json = '[{"currency":"' || replace(currency, '"', '\"') || '","amount":' || estimated_cost || '}]'
WHERE currency IS NOT NULL AND estimated_cost IS NOT NULL;

CREATE TABLE agent_runtime_events (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    inference_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    path TEXT NOT NULL,
    parent_path TEXT,
    role TEXT NOT NULL,
    model TEXT NOT NULL,
    context_window INTEGER,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    cached_prompt_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_costs_json TEXT NOT NULL DEFAULT '[]',
    has_unpriced_usage INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX idx_agent_runtime_events_session_inference
    ON agent_runtime_events(session_id, inference_id);

CREATE INDEX idx_agent_runtime_events_session_agent
    ON agent_runtime_events(session_id, agent_id, created_at ASC);

CREATE TABLE agent_runtime_snapshots (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    path TEXT NOT NULL,
    parent_path TEXT,
    role TEXT NOT NULL,
    model TEXT NOT NULL,
    context_window INTEGER,
    latest_context_tokens INTEGER NOT NULL DEFAULT 0,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    cached_prompt_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_costs_json TEXT NOT NULL DEFAULT '[]',
    has_unpriced_usage INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX idx_agent_runtime_snapshots_session_agent
    ON agent_runtime_snapshots(session_id, agent_id);

CREATE INDEX idx_agent_runtime_snapshots_session_updated
    ON agent_runtime_snapshots(session_id, updated_at DESC);

INSERT INTO agent_runtime_snapshots (
    id,
    session_id,
    agent_id,
    path,
    parent_path,
    role,
    model,
    context_window,
    latest_context_tokens,
    prompt_tokens,
    completion_tokens,
    cached_prompt_tokens,
    total_tokens,
    estimated_costs_json,
    has_unpriced_usage,
    updated_at
)
SELECT
    session_id || ':agent-root',
    session_id,
    'agent-root',
    '/root',
    NULL,
    'root',
    model,
    context_window,
    latest_context_tokens,
    prompt_tokens,
    completion_tokens,
    cached_prompt_tokens,
    total_tokens,
    estimated_costs_json,
    has_unpriced_usage,
    updated_at
FROM session_runtime_snapshots
WHERE total_tokens > 0;
