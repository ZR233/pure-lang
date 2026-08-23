---
name: subagent-workflow
description: Use when a task benefits from Pure subagents, multi-agent exploration, or validation. Covers spawn_agent coordination, task partitioning, structured capacity errors, and result synthesis.
category: agents
---

# Subagent Workflow

Use this skill when the user asks for subagents, parallel exploration, multi-crate analysis, independent validation, or separate role-based investigation.

## When To Spawn

Use `spawn_agent` for managed asynchronous work. Children report meaningful checkpoints with
`report_progress`; the runtime never wakes the parent, starts a continuation, or infers failure from
silence. Use `list_agents` to discover targets or inspect the full canonical directory, and use
`wait_agents` when the parent has no other work. `wait_agents` returns only the latest changed
agent messages; consume that delta directly instead of refreshing it with `list_agents`.

Avoid subagents when the task is small, strongly sequential, or requires one shared edit context.

If the active product exposes a dedicated role or workflow spawn tool, use that tool for the
managed role instead of emulating it with generic `spawn_agent` metadata. Product tools may enforce
resource ownership, worktrees, delivery contracts, review authorization, or fresh-session rules that
generic collaboration does not model. In Pure Studio Task mode, use `spawn_agent` only for
explorers and `task_spawn_executor` for executors. Request review with
`task_request_delivery_review` for an exact executor completion or
`task_transition` with action `beginIntegratedReview` for the current integrated Task HEAD.

## Partitioning

Give each child a narrow role and clear output contract:

- What to inspect.
- What not to modify.
- Which files, crates, or concerns are in scope.
- What summary format the parent needs.

For repository exploration, partition by crate, subsystem, risk area, or test surface. Do not pass hidden conclusions unless the child is explicitly validating a hypothesis.

## Parent Responsibilities

The parent owns coordination:

1. Spawn only the agents needed.
2. Continue independent parent work while children run.
3. When no executable parent work remains, call `wait_agents`; do not poll `list_agents`.
4. After a wait returns, consume its latest messages directly. Call `list_agents` only when you
   need target discovery, restart reconciliation, or a full diagnostic directory snapshot.
5. If a child has not updated its progress for five minutes, use `list_agents` for the current
   directory age and `read_agent_session` only as bounded evidence before deciding whether to
   `send_message` with a concrete alternative or
   `interrupt_agent`.
6. Reconcile conflicts in child findings.
7. Close agents only when their work is no longer needed; in Task mode, follow the product review and
   merge contract before closing an executor.

## Capacity Failures

If `spawn_agent` or `send_message` returns a structured capacity error, stop spawning or retrying child agents. Continue the remaining task in the parent agent and explain that provider or agent capacity limited subagent execution only when relevant.

## Validation Pattern

Subagents are useful for independent checks after a draft plan or code change. Give the validator the artifact, task, and expected acceptance criteria, but avoid leaking the intended answer unless required.

## Skill Learning

If subagent coordination reveals a reusable project workflow, update or create a project skill with `skill_manage`. Do not modify system skills directly.
