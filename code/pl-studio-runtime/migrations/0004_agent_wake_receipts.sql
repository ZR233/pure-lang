CREATE TABLE IF NOT EXISTS agent_wake_receipts (
    agent_id TEXT NOT NULL,
    wake_id TEXT NOT NULL,
    receipt_json TEXT NOT NULL,
    accepted_at INTEGER NOT NULL,
    PRIMARY KEY (agent_id, wake_id),
    FOREIGN KEY(agent_id) REFERENCES agent_runtime_states(agent_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_agent_wake_receipts_agent_accepted
    ON agent_wake_receipts(agent_id, accepted_at, wake_id);

PRAGMA user_version = 5;
