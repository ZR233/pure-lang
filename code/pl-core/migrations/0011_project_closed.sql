ALTER TABLE projects ADD COLUMN closed INTEGER NOT NULL DEFAULT 0;

CREATE INDEX idx_projects_closed_last_opened_at
    ON projects(closed, last_opened_at DESC, updated_at DESC, id DESC);
