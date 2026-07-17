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

CREATE TABLE agent_framework_events (
    agent_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (agent_id, sequence)
);

CREATE TABLE agent_outcomes (
    id TEXT PRIMARY KEY,
    task_run_id TEXT NOT NULL,
    work_unit_id TEXT,
    agent_id TEXT NOT NULL,
    owner_path TEXT NOT NULL,
    initiated_by TEXT NOT NULL,
    requested_by_call_id TEXT NOT NULL,
    role TEXT NOT NULL,
    status TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    summary TEXT,
    error TEXT,
    delivery_json TEXT,
    review_json TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    terminal_observed INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY(task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE,
    FOREIGN KEY(work_unit_id) REFERENCES work_units(id) ON DELETE SET NULL
);

CREATE TABLE agent_pending_inputs (
    agent_id TEXT NOT NULL,
    queue_position INTEGER NOT NULL,
    input_json TEXT NOT NULL,
    PRIMARY KEY (agent_id, queue_position)
);

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

CREATE TABLE agent_runtime_sessions (
    agent_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    context_json TEXT NOT NULL,
    usage_json TEXT NOT NULL,
    last_context_tokens INTEGER,
    trace_sequence INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (agent_id, session_id)
);

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

CREATE TABLE agent_runtime_states (
    agent_id TEXT PRIMARY KEY,
    revision INTEGER NOT NULL,
    snapshot_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE agent_runtime_traces (
    agent_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (agent_id, session_id, sequence)
);

CREATE TABLE agent_turns (
    agent_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    status TEXT NOT NULL,
    reason TEXT,
    usage_json TEXT NOT NULL,
    metadata_json TEXT,
    started_at INTEGER,
    finished_at INTEGER,
    PRIMARY KEY (agent_id, turn_id)
);

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

CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE attachments (
    id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT NOT NULL,
    message_id TEXT,
    media_type TEXT NOT NULL,
    filename TEXT,
    storage_path TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    width INTEGER,
    height INTEGER,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE,
    FOREIGN KEY(message_id) REFERENCES messages(id) ON DELETE SET NULL
);

CREATE TABLE branch_leases (
    id TEXT PRIMARY KEY,
    task_run_id TEXT NOT NULL UNIQUE,
    git_common_dir TEXT NOT NULL,
    branch TEXT NOT NULL,
    expected_head TEXT NOT NULL,
    acquired_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE
);

CREATE TABLE interactions (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    item_id TEXT,
    tool_id TEXT,
    agent_path TEXT,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    resolution_json TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    resolved_at INTEGER,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE merge_records (
    id TEXT PRIMARY KEY,
    task_run_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    status TEXT NOT NULL,
    expected_head TEXT NOT NULL,
    source_commit TEXT NOT NULL,
    conflict_files_json TEXT NOT NULL,
    resolution_summary TEXT,
    verification_json TEXT,
    attempt INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE
);

CREATE TABLE message_parts (
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
    revision INTEGER NOT NULL DEFAULT 0,
    activity_group_id TEXT,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    reasoning_content TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    item_type TEXT NOT NULL DEFAULT 'message',
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_opened_at INTEGER,
    closed INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE review_rounds (
    id TEXT PRIMARY KEY,
    task_run_id TEXT NOT NULL,
    round INTEGER NOT NULL,
    head_commit TEXT NOT NULL,
    status TEXT NOT NULL,
    reviewer_agent_id TEXT,
    summary TEXT,
    design_references_json TEXT NOT NULL,
    findings_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE
);

CREATE TABLE session_runtime_snapshots (
    session_id TEXT PRIMARY KEY,
    model TEXT NOT NULL,
    context_window INTEGER,
    latest_context_tokens INTEGER NOT NULL DEFAULT 0,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    cached_prompt_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    currency TEXT,
    estimated_cost REAL,
    updated_at INTEGER NOT NULL,
    estimated_costs_json TEXT NOT NULL DEFAULT '[]',
    has_unpriced_usage INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE session_skills (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    skill_name TEXT NOT NULL,
    skill_name_key TEXT NOT NULL,
    source TEXT NOT NULL,
    path TEXT NOT NULL,
    first_turn_id TEXT NOT NULL,
    last_turn_id TEXT NOT NULL,
    last_tool_call_id TEXT NOT NULL,
    activated_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(session_id, skill_name_key),
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    title TEXT NOT NULL,
    mode TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    archived INTEGER NOT NULL DEFAULT 0,
    instruction_snapshot_json TEXT,
    visibility TEXT NOT NULL DEFAULT 'active',
    parent_session_id TEXT,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE TABLE studio_events (
    id TEXT PRIMARY KEY,
    project_id TEXT,
    session_id TEXT,
    turn_id TEXT,
    sequence INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL
);

CREATE TABLE studio_messages (
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

CREATE TABLE task_runs (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    phase TEXT NOT NULL,
    plan TEXT NOT NULL,
    workspace_root TEXT NOT NULL,
    git_common_dir TEXT NOT NULL,
    branch TEXT NOT NULL,
    base_commit TEXT NOT NULL,
    expected_head TEXT NOT NULL,
    design_commit TEXT,
    status_message TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE tool_approvals (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    tool_call_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    arguments_json TEXT NOT NULL,
    working_directory TEXT,
    decision TEXT NOT NULL,
    reason TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE trace_events (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE turns (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    status TEXT NOT NULL,
    reason TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    completed_at INTEGER,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE work_units (
    id TEXT PRIMARY KEY,
    task_run_id TEXT NOT NULL,
    title TEXT NOT NULL,
    status TEXT NOT NULL,
    owned_paths_json TEXT NOT NULL,
    base_commit TEXT NOT NULL,
    worktree_path TEXT NOT NULL,
    branch TEXT NOT NULL,
    attempt INTEGER NOT NULL,
    agent_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY(task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE
);

CREATE INDEX idx_agent_events_agent_created_at
    ON agent_events(agent_id, created_at ASC, sequence ASC);

CREATE INDEX idx_agent_events_session_created_at
    ON agent_events(session_id, created_at ASC, sequence ASC);

CREATE UNIQUE INDEX idx_agent_events_session_sequence
    ON agent_events(session_id, sequence);

CREATE UNIQUE INDEX idx_agent_outcomes_run_agent_attempt
    ON agent_outcomes(task_run_id, agent_id, attempt);

CREATE INDEX idx_agent_outcomes_run_status
    ON agent_outcomes(task_run_id, status, updated_at DESC, id DESC);

CREATE INDEX idx_agent_runtime_events_session_agent
    ON agent_runtime_events(session_id, agent_id, created_at ASC);

CREATE UNIQUE INDEX idx_agent_runtime_events_session_inference
    ON agent_runtime_events(session_id, inference_id);

CREATE INDEX idx_agent_runtime_sessions_session
    ON agent_runtime_sessions(session_id, updated_at);

CREATE UNIQUE INDEX idx_agent_runtime_snapshots_session_agent
    ON agent_runtime_snapshots(session_id, agent_id);

CREATE INDEX idx_agent_runtime_snapshots_session_updated
    ON agent_runtime_snapshots(session_id, updated_at DESC);

CREATE INDEX idx_agent_runtime_traces_session
    ON agent_runtime_traces(session_id, sequence);

CREATE UNIQUE INDEX idx_agents_session_path
    ON agents(session_id, path);

CREATE INDEX idx_attachments_message_id ON attachments(message_id);

CREATE INDEX idx_attachments_session_id ON attachments(session_id);

CREATE UNIQUE INDEX idx_branch_leases_common_branch
    ON branch_leases(git_common_dir, branch);

CREATE INDEX idx_interactions_session_status_updated
    ON interactions(session_id, status, updated_at DESC);

CREATE INDEX idx_interactions_session_turn
    ON interactions(session_id, turn_id);

CREATE INDEX idx_merge_records_run_updated
    ON merge_records(task_run_id, updated_at DESC, id DESC);

CREATE INDEX idx_message_parts_message_order
    ON message_parts(message_id, part_order ASC, sequence ASC, id ASC);

CREATE INDEX idx_message_parts_session_order
    ON message_parts(session_id, part_order ASC, sequence ASC, id ASC);

CREATE INDEX idx_message_parts_turn
    ON message_parts(turn_id, part_order ASC);

CREATE INDEX idx_messages_session_created_at
    ON messages(session_id, created_at ASC, id ASC);

CREATE INDEX idx_messages_session_item_type_created_at
    ON messages(session_id, item_type, created_at ASC, id ASC);

CREATE INDEX idx_projects_closed_last_opened_at
    ON projects(closed, last_opened_at DESC, updated_at DESC, id DESC);

CREATE INDEX idx_projects_last_opened_at ON projects(last_opened_at DESC, id DESC);

CREATE INDEX idx_projects_updated_at ON projects(updated_at DESC, id DESC);

CREATE UNIQUE INDEX idx_review_rounds_run_round
    ON review_rounds(task_run_id, round);

CREATE INDEX idx_session_skills_session_updated_at
    ON session_skills(session_id, updated_at DESC, skill_name_key ASC);

CREATE INDEX idx_sessions_parent_session
    ON sessions(parent_session_id);

CREATE INDEX idx_sessions_project_updated_at
    ON sessions(project_id, archived, updated_at DESC, id DESC);

CREATE INDEX idx_studio_events_created
    ON studio_events(created_at ASC, id ASC);

CREATE INDEX idx_studio_events_project_sequence
    ON studio_events(project_id, sequence)
    WHERE project_id IS NOT NULL;

CREATE UNIQUE INDEX idx_studio_events_session_sequence
    ON studio_events(session_id, sequence)
    WHERE session_id IS NOT NULL;

CREATE INDEX idx_studio_messages_session_created
    ON studio_messages(session_id, created_at ASC, sequence ASC, id ASC);

CREATE INDEX idx_studio_messages_turn
    ON studio_messages(turn_id, updated_at DESC);

CREATE INDEX idx_task_runs_phase_updated
    ON task_runs(phase, updated_at DESC, id DESC);

CREATE INDEX idx_task_runs_session_updated
    ON task_runs(session_id, updated_at DESC, id DESC);

CREATE INDEX idx_tool_approvals_session_created_at
    ON tool_approvals(session_id, created_at ASC, id ASC);

CREATE INDEX idx_trace_events_session_sequence
    ON trace_events(session_id, sequence ASC);

CREATE INDEX idx_trace_events_session_timestamp
    ON trace_events(session_id, timestamp ASC);

CREATE INDEX idx_turns_session_updated
    ON turns(session_id, updated_at DESC, id DESC);

CREATE INDEX idx_turns_status
    ON turns(status, updated_at DESC);

CREATE INDEX idx_work_units_run_status
    ON work_units(task_run_id, status, created_at ASC, id ASC);

PRAGMA user_version = 1;
