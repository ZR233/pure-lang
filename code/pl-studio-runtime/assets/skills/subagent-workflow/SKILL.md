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

Use the tool schema's camelCase names on the first call. Canonical shapes are:

- unrestricted explorer/planner/reviewer: `{"profileId":"explorer","forkTurns":"none","message":"..."}`
  （按角色替换 `profileId`）；
- directory executor: `{"profileId":"executor","forkTurns":"none","writablePaths":["src/module"],"message":"..."}`;
- worktree executor: `{"profileId":"worktree_executor","forkTurns":"none","message":"..."}`.

Never send `profile_id`, `fork_turns`, or `writable_paths`. Only a directory Profile accepts
`writablePaths`; do not send it to unrestricted or worktree Profiles. Validate the intended Profile,
mode, and narrow paths before the first invocation. If a call returns a typed argument error, correct
the schema once instead of repeating the same arguments. Treat an intentional directory-boundary
denial as `expected_rejection`: do not retry it or bypass it through shell, Git, or MCP.

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

Every non-reviewer child must publish one final durable delivery after completing its work and before
its final reply. It calls `report_progress` with `stage:"readyForCompletion"`, a summary containing
`CHILD_DELIVERY_READY`, a concrete `nextStep`, and substantive `detail` containing the same evidence
as the final reply. A worktree executor additionally includes `WORKTREE_COMMIT_READY`, its verified
40-character commit, and workspace root. Failure to publish is a delivery failure, not permission to
claim success in free text.

The parent stores each successful spawn receipt's `agentId`, calls `wait_agents` until that specific
child is terminal (a progress wake is not terminal), and only then calls
`read_agent_submissions({"target":"<agentId>"})`. The canonical page must be nonempty and contain the
durable marker. An empty page may trigger `read_agent_session` for diagnosis, but session text is not
normal delivery and requires a narrowed re-dispatch. Do not poll submissions before terminal.

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
For a worktree child, the safe order is inspect branch/base/commit, integrate with ordinary Git, then
`close_agent({"target":"<agentId>","workspaceDisposition":"cleanup"})`, and finally verify that the
Pure-owned worktree and branch are gone. Never request cleanup before integration.

The reviewer remains read-only for workspace, Git, shell, and external state. After finishing its
review and before its final reply it must call `report_progress` to append the final durable
collaboration verdict. Earlier markerless intermediate progress submissions are permitted but do not
replace that final verdict. The final submission uses `REVIEWER_FINDING` for a blocking verdict or
`REVIEWER_READ_ONLY_APPROVED` for approval. Root must take the reviewer agentId from the bound spawn
receipt and call `read_agent_submissions` with the reviewer agentId; only a canonical nonempty page
carrying that marker authorizes the next step. A root retelling or `read_agent_session` does not count,
and an unbound tool result does not count. A finding returns to implementation/design repair and a
fresh reviewer; only a durable approval allows root to run the final verification gates.
Reviewers never read `.git/**`, the index, or object storage with file tools because those are internal
binary Git data; they use `git_status`, `git_diff`, and `git_workspace_info` for Git evidence and only
open known text source files.

If collaboration capacity is unavailable, continue useful work in the root Agent and report the
constraint only when it affects the result.
