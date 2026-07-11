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

CREATE INDEX idx_task_runs_session_updated
    ON task_runs(session_id, updated_at DESC, id DESC);
CREATE INDEX idx_task_runs_phase_updated
    ON task_runs(phase, updated_at DESC, id DESC);

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

CREATE INDEX idx_work_units_run_status
    ON work_units(task_run_id, status, created_at ASC, id ASC);

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
    FOREIGN KEY(task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE,
    FOREIGN KEY(work_unit_id) REFERENCES work_units(id) ON DELETE SET NULL
);

CREATE UNIQUE INDEX idx_agent_outcomes_run_agent_attempt
    ON agent_outcomes(task_run_id, agent_id, attempt);
CREATE INDEX idx_agent_outcomes_run_status
    ON agent_outcomes(task_run_id, status, updated_at DESC, id DESC);

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

CREATE INDEX idx_merge_records_run_updated
    ON merge_records(task_run_id, updated_at DESC, id DESC);

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

CREATE UNIQUE INDEX idx_review_rounds_run_round
    ON review_rounds(task_run_id, round);

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

CREATE UNIQUE INDEX idx_branch_leases_common_branch
    ON branch_leases(git_common_dir, branch);
