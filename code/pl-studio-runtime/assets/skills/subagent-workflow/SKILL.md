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
the selected stable `profileId` to `spawn_agent`; the child freezes its creation instructions and
physical workspace. The host may apply current provider/model/effort configuration at the next Turn
boundary. Disabled or unavailable profiles cannot be spawned.

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

Children never receive the root Thread Mode's workflow tools or runtime state. The root Agent may
only query and advance the graph already registered by the host; neither root nor children compile
workflow definitions.

## When to spawn

Use `spawn_agent` for bounded asynchronous work. Independent explorers use fresh context with
`forkTurns:none` and run in parallel; root synthesizes their evidence. Use `list_agents` to inspect
live instances and `wait` when the parent has no independent work. Preserve real semantic
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

The parent stores each successful spawn receipt's `agentId`, `profileId`, `messageAccepted`, and
`messageSequence`. `messageAccepted:true` proves input admission, not completion. `send_message`
returns `target`, `messageId`, and `sequence`; neither receipt contains a `turnId`. Keep at most one
outstanding assignment per child. Before reusing a child, record its previous `lastTurn.turnId` from
`list_agents` and the latest observed child notification `commitSequence`; do not accept an old Turn
or an old delivery as the new assignment's result. Message sequence and journal commit sequence are
different counters and must not be compared to each other.

When the parent has no independent work, call `wait({"taskIds":[],"timeoutMs":300000})` to wait for
Thread messages. This is a maximum wait: a message arriving earlier wakes the parent immediately.
Choose a shorter limit only when a concrete independent action has an earlier deadline. Do not
mechanically call `list_agents` or reread full session history after every timeout. Continue waiting
unless a new notification, overdue expected milestone, missing completion evidence, or explicit
error warrants a targeted query. Consume every received notification and keep all outstanding
children accounted for; waiting longer does not excuse ignoring a failure or accepting stale evidence.
The wait result contains `tasks`, `messagesReady`, and `timedOut`, not `batch.events`.
A ready-message wake or local tool-task completion does not prove child completion. Child notifications
arrive as model-visible Thread messages with `childId`, `commitSequence`, `turn`, `lifecycle`, and
`progress`. Bind the `childId` to the actual spawn receipt. Successful Turn evidence is
`turn.state.kind:"finished"` with `turn.state.value:"completed"` or `"toolCompleted"`; record that
notification's `turn.turnId`. A `progress` payload without such a Turn only proves publication.
In particular, `report_progress.stage:"readyForCompletion"` and `CHILD_DELIVERY_READY` do not mean
that the child Turn has ended. Keep that child pending and wait for its matching successful terminal
notification to enter the parent's model context. Do not request a workflow checkpoint, advance the
graph, integrate the delivery, or announce completion while that notification is still unconsumed.
A terminal notification arriving after a checkpoint request cannot retroactively justify the request.
`list_agents` can corroborate the matching row's `lastTurn.turnId` and `lastTurn.state`, together with
`pendingInputs` and `runningTasks`; absence from a wait response proves nothing.

Maintain the outstanding child set until every target has its own current successful Turn evidence.
If a child is reused, require a new Turn identity after the recorded previous Turn; diagnose ambiguous
or missing notification evidence with `read_agent_session({"target":"<agentId>","detail":"full"})`
instead of inventing an ID from a receipt. Only then call
`read_agent_submissions({"target":"<agentId>"})`. The canonical page must be nonempty and contain the
required durable marker. The page also reports `targetState` from the same frozen history as its
cursor: `agentId`, `throughSequence`, `turn`, `completion`, and `guidance`. Check this state before
using any page item. If `targetState.completion` is `running` or `notStarted`, keep the child pending;
a nonempty page or ready marker does not authorize integration or a checkpoint. Follow its guidance,
wait for and consume the matching successful child Turn notification, then read a fresh page without
the old cursor. Paging a frozen Running view cannot make it become a terminal view. `completed` and
`toolCompleted` corroborate success but do not replace the requirement to consume the matching
terminal notification. `waitingInteraction`, `stepLimit`, `cancelled`, `interrupted`, and `failed`
require the corresponding interaction, continuation, or failure diagnosis; none is successful delivery.
This is a safeguard for an early/diagnostic read, not a reason to poll submissions before completion.
Follow `nextCursor`/`hasMore` as supplied to obtain complete evidence; a
`fragment` is part of a large immutable submission, not a separate delivery. For a reused child,
exclude already consumed submissions. Session text is diagnostic evidence, not the normal delivery
channel; an empty or missing final submission requires a narrowed re-dispatch.

`turn.state.value:"stepLimit"` is a paused budget boundary, not successful completion.
`"waitingInteraction"` requires the outstanding interaction to be resolved; `cancelled`, `interrupted`,
or `failed` are not success. Read the child's latest durable Timeline first. If the child is healthy
and work remains, send a concrete continuation with `send_message`, retain its admission receipt,
and wait for the newly observed Turn. Do not resume blindly, and do not substitute session history
for the required durable submission.

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
For one parallel worktree batch, inspect every branch/base/commit and integrate every accepted commit
with ordinary Git before the first cleanup. Only after the final accepted sibling commit is integrated,
call `close_agent({"target":"<agentId>","workspaceDisposition":"cleanup"})` for each child and
require the successful close result's `lifecycle.kind:"closed"` and selected `workspaceDisposition`.
A child lifecycle notification can precede physical cleanup and cannot replace this successful receipt.
A pending tool-task acknowledgement
is not completed cleanup: wait for that exact task's final result when needed. A failed closure
retains recoverable resources and must be retried explicitly. Verify that each Pure-owned worktree
and branch is gone. Omitted `workspaceDisposition` means `preserve`, not cleanup; no
`agentChanged` event is part of this tool contract. Never interleave one child's integration and cleanup
while another accepted sibling commit is still pending integration.

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
