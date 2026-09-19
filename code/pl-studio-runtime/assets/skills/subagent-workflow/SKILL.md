---
name: subagent-workflow
description: Use when a task benefits from 糊来帮 subagents, multi-agent exploration, implementation, or validation through configured Agent Profiles.
category: agents
---

# Subagent workflow

Use this skill when the user asks for subagents, parallel exploration, multi-crate analysis,
independent validation, or separate role-based investigation.

## Profile selection

Call `list_agent_profiles` before spawning when the suitable profile is not already known. Pass
the selected stable `profileId` to `spawn_agent`; the child freezes its creation instructions and
physical workspace. The host may apply current provider/model/effort configuration at the next Turn
boundary. Disabled or unavailable profiles cannot be spawned.

Use the tool schema's camelCase names on the first call. Canonical shapes are:

- unrestricted explorer/planner/reviewer: `{"profileId":"explorer","forkTurns":"none","taskSummary":"Inspect the assigned component","message":"..."}`
  （按角色替换 `profileId`）；
- directory executor: `{"profileId":"executor","forkTurns":"none","writablePaths":["src/module"],"taskSummary":"Implement the assigned module","message":"..."}`;
- worktree executor: `{"profileId":"worktree_executor","forkTurns":"none","taskSummary":"Inspect the assigned component","message":"..."}`.

Every spawn requires a concise `taskSummary` (1–80 Unicode characters after whitespace normalization) for the GUI child list; `message` contains the full instructions. Root and children provide 1–3 sentence commentary at meaningful checkpoints, including before the first tool, important findings, transitions, waits and blockers.

Never send `profile_id`, `fork_turns`, or `writable_paths`. Only a directory Profile accepts
`writablePaths`; do not send it to unrestricted or worktree Profiles. Validate the intended Profile,
mode, and narrow paths before the first invocation. If a call returns a typed argument error, correct
the schema once instead of repeating the same arguments. Treat an intentional directory-boundary
denial as `expected_rejection`: do not retry it or bypass it through shell, Git, or MCP.

The built-in profiles are `explorer`, `planner`, `executor`, `worktree_executor`, and `reviewer`. They are immutable and
may be disabled. User profiles are loaded from one TOML file per profile. Select by capability,
not by assuming that a workflow stage requires a particular profile.

Children never receive the root Thread Mode's workflow tools or runtime state. The root Agent may
only query and advance the graph already registered by the host; neither root nor children compile
workflow definitions.

## When to spawn

Use `spawn_agent` for bounded asynchronous work. Independent explorers use fresh context with
`forkTurns:none` and run in parallel; root synthesizes their evidence. Use `list_agents` to inspect
live instances and `wait` when the parent has no independent work. Preserve real semantic
dependencies in order; do not parallelize overlapping ownership. In Task `editing_documents`, only
root writes `design/**`.

For delegation content, reporting, waiting and continuation, follow the system prompt's canonical
collaboration contract. This skill only supplies profile selection and invocation examples; it does
not add another completion protocol or confirmation step. Dispatch bodies and Turn reports have no
application length limit; taskSummary alone is the short list title.
