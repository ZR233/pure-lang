---
name: subagent-workflow
description: Use when a task benefits from Pure subagents, multi-agent exploration, implementation, or validation through configured Agent Profiles.
category: agents
---

# Subagent workflow

Use this skill when the user asks for subagents, parallel exploration, multi-crate analysis,
independent validation, or separate role-based investigation.

## Profile selection

Call `list_agent_profiles` before spawning when the suitable profile is not already known. Pass
the selected stable `profileId` to `spawn_agent`; the child freezes that profile's instructions,
provider, model, and effort for its lifetime. Disabled or unavailable profiles cannot be spawned.

The built-in profiles are `explorer`, `planner`, `executor`, and `reviewer`. They are immutable and
may be disabled. User profiles are loaded from one TOML file per profile. Select by capability,
not by assuming that a workflow stage requires a particular profile.

Children never receive the root Thread's `workflow_state` tool. Workflow compilation and stage
transitions remain the root Agent's responsibility.

## When to spawn

Use `spawn_agent` for bounded asynchronous work. Use `list_agents` to inspect live instances and
`wait_agents` only when the parent has no independent work. Avoid subagents when the task is small,
strongly sequential, or requires one shared edit context.

Give each child a narrow role and explicit output contract:

- what to inspect or implement;
- what must not be modified;
- which files, crates, or concerns are in scope;
- what evidence and summary the parent needs.

The root Agent owns coordination, reconciles conflicting findings, integrates changes, performs
final verification, and advances the workflow state. Agent Profiles do not imply worktrees,
branches, commits, merge records, delivery gates, or fixed review rounds.

If collaboration capacity is unavailable, continue useful work in the root Agent and report the
constraint only when it affects the result.
