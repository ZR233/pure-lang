ALTER TABLE agent_events ADD COLUMN reason TEXT;
ALTER TABLE agent_events ADD COLUMN budget_limit_kind TEXT;
ALTER TABLE agent_events ADD COLUMN budget_usage_json TEXT;

ALTER TABLE agent_turns ADD COLUMN reason TEXT;
ALTER TABLE agent_turns ADD COLUMN budget_limit_kind TEXT;
ALTER TABLE agent_turns ADD COLUMN budget_usage_json TEXT;
