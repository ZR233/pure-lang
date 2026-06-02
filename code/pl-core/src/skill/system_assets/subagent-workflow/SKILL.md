---
name: subagent-workflow
description: Use when a task benefits from Pure subagents, multi-agent exploration, or validation. Covers spawn_agent/subagent selection, task partitioning, recoverable 429 handling, and result synthesis.
category: agents
---

# Subagent Workflow

Use this skill when the user asks for subagents, parallel exploration, multi-crate analysis, independent validation, or separate role-based investigation.

## When To Spawn

Use `spawn_agent` for managed asynchronous work when several agents can explore in parallel and the parent should coordinate, wait, and synthesize.

Use `subagent` for a simple synchronous delegation when a single child result is enough.

Avoid subagents when the task is small, strongly sequential, or requires one shared edit context.

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
2. Use `wait_agent` to collect results.
3. Use `list_agents` if state is unclear.
4. Reconcile conflicts in child findings.
5. Produce the final answer or implementation plan.

## Recoverable Capacity Failures

If a subagent tool result mentions `recoverableSubagentProvider429` or recoverable capacity failures, stop spawning or retrying child agents. Continue the remaining task in the parent agent and explain that provider capacity limited subagent execution only when relevant.

## Validation Pattern

Subagents are useful for independent checks after a draft plan or code change. Give the validator the artifact, task, and expected acceptance criteria, but avoid leaking the intended answer unless required.

## Skill Learning

If subagent coordination reveals a reusable project workflow, update or create a project skill with `skill_manage`. Do not modify system skills directly.
