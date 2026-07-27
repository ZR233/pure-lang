---
name: subagent-workflow
description: Use when a task benefits from Pure subagents, multi-agent exploration, or validation. Covers spawn_agent coordination, task partitioning, structured capacity errors, and result synthesis.
category: agents
---

# Subagent Workflow

Use this skill when the user asks for subagents, parallel exploration, multi-crate analysis, independent validation, or separate role-based investigation.

## When To Spawn

Use `spawn_agent` for managed asynchronous work. The runtime subscribes the parent to direct-child
updates and starts a merged continuation when progress, attention, a usable terminal contract, or an
inactivity timeout requires coordination. Use `list_agents` only when the current state is unclear.

Avoid subagents when the task is small, strongly sequential, or requires one shared edit context.

If the active product exposes a dedicated role or workflow spawn tool, use that tool for the
managed role instead of emulating it with generic `spawn_agent` metadata. Product tools may enforce
resource ownership, worktrees, delivery contracts, review authorization, or fresh-session rules that
generic collaboration does not model. In Pure Studio Task mode, use `spawn_agent` only for
explorers, `task_spawn_executor` for executors, and `task_request_review` for reviewers.

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
3. End the current turn when no executable parent work remains; do not poll child state.
4. On a subscribed continuation, use the attached canonical snapshots and `list_agents` only if
   additional tree context is needed.
5. Reconcile conflicts in child findings.
6. Produce the final answer or implementation plan.

## Capacity Failures

If `spawn_agent` or `send_input` returns a structured capacity error, stop spawning or retrying child agents. Continue the remaining task in the parent agent and explain that provider or agent capacity limited subagent execution only when relevant.

## Validation Pattern

Subagents are useful for independent checks after a draft plan or code change. Give the validator the artifact, task, and expected acceptance criteria, but avoid leaking the intended answer unless required.

## Skill Learning

If subagent coordination reveals a reusable project workflow, update or create a project skill with `skill_manage`. Do not modify system skills directly.
