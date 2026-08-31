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

The built-in profiles are `explorer`, `planner`, `executor`, `worktree_executor`, and `reviewer`. They are immutable and
may be disabled. User profiles are loaded from one TOML file per profile. Select by capability,
not by assuming that a workflow stage requires a particular profile.

Children never receive the root Thread's `workflow_state` tool. Workflow compilation and stage
transitions remain the root Agent's responsibility.

## When to spawn

Use `spawn_agent` for bounded asynchronous work. Independent explorers use fresh context with
`forkTurns:none` and run in parallel; root synthesizes their evidence. Use `list_agents` to inspect
live instances and `wait_agents` when the parent has no independent work. Preserve real semantic
dependencies in order; do not parallelize overlapping ownership. In Task `editing_documents`, only
root writes `design/**`.

Give each child a self-contained message with eight sections: purpose and user value; design baseline
and prerequisite facts; owned files/modules and invariants; forbidden scope; ordered
exploration/implementation/test/submit steps; checkable completion and failure conditions;
diff/commit/test/risk evidence; workspace, `writablePaths`, Git, and cleanup contract.

Give each child a narrow role and explicit output contract:

- what to inspect or implement;
- what must not be modified;
- which files, crates, or concerns are in scope;
- what evidence and summary the parent needs.

The root Agent owns coordination, reconciles conflicting findings, integrates changes, performs
final verification, and advances the workflow state. For a single bounded implementation or mutually
exclusive directories use `executor` with the narrowest non-overlapping `writablePaths`; directory
restrictions apply only to Pure built-in mutation and shell/Git/MCP can bypass them, so children must
not use that to cross scope or stage/commit/reset. Shared interfaces, manifests, lockfiles, generated
files, whole-tree formatting, or high-risk Git state use `worktree_executor`; it must commit in its
isolated worktree and root explicitly adopts then cleans up. Worktrees isolate the scene, not semantic
dependencies.

After working, root enters `integrating`: inspect directory diffs, explicitly cherry-pick/merge
worktree commits, resolve only adjacent necessary conflicts, and request cleanup. If a child fails,
wait for capacity and narrow/re-dispatch once; only a second failure permits minimal
`ROOT_IMPLEMENTATION_FALLBACK`, recording reason and directly modified files. After integration always
spawn a new fresh-context read-only `reviewer`; it never fixes. Route code findings to `working` and
design findings to `editing_documents`; every repair must be re-integrated and receive a new reviewer.

If collaboration capacity is unavailable, continue useful work in the root Agent and report the
constraint only when it affects the result.
