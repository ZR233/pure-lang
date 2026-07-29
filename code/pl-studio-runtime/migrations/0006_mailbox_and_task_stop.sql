CREATE TABLE agent_active_inputs (
    agent_id TEXT PRIMARY KEY,
    input_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(agent_id) REFERENCES agent_runtime_states(agent_id) ON DELETE CASCADE
);

ALTER TABLE task_runs
    ADD COLUMN stop_requested_origin TEXT;

ALTER TABLE task_runs
    ADD COLUMN task_generation INTEGER NOT NULL DEFAULT 0;

ALTER TABLE task_runs
    ADD COLUMN terminal_generation INTEGER;

PRAGMA user_version = 7;
