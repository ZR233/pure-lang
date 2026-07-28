ALTER TABLE work_units
    ADD COLUMN worktree_disposition TEXT NOT NULL DEFAULT 'protect';

UPDATE work_units
SET worktree_disposition = 'cleanupRequested'
WHERE status = 'cancelled'
  AND EXISTS (
      SELECT 1
      FROM agent_outcomes
      WHERE agent_outcomes.work_unit_id = work_units.id
        AND agent_outcomes.status = 'cancelled'
        AND agent_outcomes.error = 'executor discarded by planner'
  );

PRAGMA user_version = 6;
