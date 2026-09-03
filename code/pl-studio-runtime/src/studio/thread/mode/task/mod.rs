use pl_core::{
    StaticThreadModeRegistration, StaticWorkflowDefinition, StaticWorkflowState,
    StaticWorkflowTransition,
};
use pl_protocol::WorkflowStateKind;

pub const PROMPT: &str = r#"# Task Thread Mode

You own one canonical root task. The framework has already registered and compiled the complete
workflow graph and starts its initial state before the first provider request. Never submit, patch,
compile, or supersede a workflow definition. Use `workflow_current`, `workflow_next`,
`workflow_graph`, and `workflow_history` for canonical reads. Use `workflow_transition` only after
the current state's completion criteria and the selected edge guard are satisfied. Immediately
before every transition, issue a separate read-only tool response that calls `workflow_current` and
`workflow_next` together; before the first transition and before entering a terminal state, also
call `workflow_graph` and `workflow_history` in that read-only response. Pass the exact run ID,
revision, current state, and direct successor returned by those fresh queries. Never infer CAS values
from the injected context or an earlier mutation receipt. Use `workflow_restart` only for an explicit
new attempt. Read-only queries may run together; a mutation must be the only tool call in its provider
response. For `workflow_transition`, keep `expectedRunId`, `expectedRevision`, `expectedStateId`, and
`targetStateId` at the top level, and put all three completion fields inside one `completion` object:
`{"reason":"...","summary":"...","evidence":["..."]}`. There is no top-level `reason` field.

Planning and confirmation are real user boundaries managed by the independent fixed Plan state
machine, not by the workflow graph. Ask `request_user_input` only when a missing material fact or
user preference prevents a complete plan. Never use it to ask whether to implement, proceed, or
approve a complete plan, and never replace Plan confirmation with a final-text question. Call
`plan_current` before a Plan mutation and use `plan_next` or `plan_history` when its transitions or
audit history are needed. When a complete plan is ready, directly call the solo `plan_submit` with
the exact Plan revision and complete Markdown; its Approve/Revise Interaction is the only
implementation-authorization boundary. If the user requests additions or changes, read the
resulting `revisionRequested` state, incorporate every requested change, and submit the complete
replacement. The workflow remains in `planning` throughout clarification and confirmation. Only
after `plan_current` returns `approved` may you use the solo `workflow_transition` from `planning`
to `editing_documents`. Do not start implementation before that approval.

For architecture, protocol, runtime behavior, or durable conventions, the root agent personally
updates `design/**` before implementation. Delegate independent read-only exploration to fresh
context `explorer` profiles. After approval, delegate implementation by dependency and file
ownership: `executor` profiles must have mutually isolated writable paths, and `worktree_executor`
profiles own distinct branches/worktrees. Spawn independent children before waiting so they run in parallel. Every child
task must state its objective, design baseline, ownership, forbidden scope, steps, success/failure
conditions, evidence, isolation, Git, and cleanup contract. A child AgentSession cannot read or
mutate the root AgentSession Plan, so every implementation and reviewer spawn message must contain
the approved implementation baseline needed for that owned task. The root can reread its complete
approved Plan with `plan_current` throughout implementation.

Every non-reviewer child must publish a durable `CHILD_DELIVERY_READY` submission before its final
reply; a worktree delivery also includes `WORKTREE_COMMIT_READY`, a full commit ID, and workspace
root. The root waits for terminal state and reads canonical submissions by the real child ID.
Directory diffs are reviewed in place. Worktree commits are explicitly reviewed and cherry-picked
or merged, then their worktrees and temporary branches are cleaned. For a parallel worktree batch,
integrate every accepted commit before issuing the first cleanup; only then clean and verify each
child workspace. Never interleave one child's integration and cleanup while another accepted sibling
commit is still pending integration.

After integration, create a fresh-context read-only reviewer. Its final durable verdict must be
`REVIEWER_FINDING` or `REVIEWER_READ_ONLY_APPROVED`; root summaries and session text are not
substitutes. Findings return through the registered graph for repair and another integration/review
cycle. Only after approval may the root run final gates, transition to `completed`, call `complete`,
and deliver the final result. Ordinary file, command, Git, collaboration, and final-answer
capabilities are not removed by workflow states."#;

const STATES: &[StaticWorkflowState] = &[
    StaticWorkflowState {
        id: "planning",
        title: "Planning",
        instructions: "Inspect the task and architecture, ask only material clarification questions that block a complete plan, run independent read-only exploration in parallel, and use the fixed Plan state machine rather than request_user_input or final text to obtain implementation approval for a complete implementation and validation plan with explicit ownership and isolation.",
        completion_criteria: &[
            "The requested outcome and non-goals are explicit.",
            "Architecture and protocol impacts are grounded in repository evidence.",
            "The plan names dependencies, ownership, isolation, and validation boundaries.",
            "plan_current reports approved for the complete current Plan.",
        ],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "editing_documents",
        title: "Editing design documents",
        instructions: "The root agent updates authoritative design documents for every architecture, protocol, runtime, or durable-contract change and verifies that the documents agree with the approved plan.",
        completion_criteria: &[
            "All affected design contracts are updated before implementation.",
            "No stale compatibility or Mode-as-Skill contract remains in authoritative docs.",
        ],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "working",
        title: "Working",
        instructions: "Implement the approved plan. Use isolated directory and worktree children for independent owned changes, start parallel work before waiting, and collect canonical durable deliveries and targeted tests.",
        completion_criteria: &[
            "Every implementation owner has completed or produced an explicit failure receipt.",
            "Directory scopes are mutually isolated and worktree changes have reviewable commits.",
            "Focused tests cover the implemented behavior and regressions.",
        ],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "integrating",
        title: "Integrating",
        instructions: "Review the combined directory diff, explicitly integrate worktree commits, resolve conflicts without losing other owners' changes, and clean temporary worktrees and branches after durable integration.",
        completion_criteria: &[
            "All accepted child deliveries are present exactly once in the canonical workspace.",
            "Worktree commits and cleanup are recorded.",
            "The integrated tree passes focused formatting, compile, and test checks.",
        ],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "reviewing",
        title: "Reviewing",
        instructions: "Run a fresh-context read-only review of the integrated workspace, consume its canonical durable verdict, then run the proportional final validation matrix. Findings must route back through the graph.",
        completion_criteria: &[
            "A fresh reviewer produced a canonical durable verdict for the integrated head.",
            "No unresolved design or code finding remains.",
            "All required deterministic and live acceptance gates have terminal evidence.",
        ],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "completed",
        title: "Completed",
        instructions: "",
        completion_criteria: &[],
        kind: WorkflowStateKind::Final,
    },
    StaticWorkflowState {
        id: "stopped",
        title: "Stopped",
        instructions: "",
        completion_criteria: &[],
        kind: WorkflowStateKind::Final,
    },
];

const TRANSITIONS: &[StaticWorkflowTransition] = &[
    edge(
        "planning",
        "editing_documents",
        "The fixed Plan state machine reports approved for the complete evidence-based plan.",
    ),
    edge(
        "planning",
        "stopped",
        "The task was cancelled or cannot proceed safely.",
    ),
    edge(
        "editing_documents",
        "working",
        "All required authoritative design documents match the approved plan.",
    ),
    edge(
        "editing_documents",
        "stopped",
        "The task was cancelled or the design cannot be made coherent.",
    ),
    edge(
        "working",
        "integrating",
        "Implementation owners have delivered reviewable changes and focused evidence.",
    ),
    edge(
        "working",
        "stopped",
        "The task was cancelled or implementation cannot proceed safely.",
    ),
    edge(
        "integrating",
        "working",
        "Integration exposed an implementation defect or missing delivery.",
    ),
    edge(
        "integrating",
        "reviewing",
        "All accepted deliveries are integrated and focused checks pass.",
    ),
    edge(
        "integrating",
        "stopped",
        "The task was cancelled or integration cannot complete safely.",
    ),
    edge(
        "reviewing",
        "working",
        "Review found an implementation defect that requires code changes.",
    ),
    edge(
        "reviewing",
        "editing_documents",
        "Review found an architecture or contract defect that requires design changes.",
    ),
    edge(
        "reviewing",
        "completed",
        "The reviewer approved the integrated head and all required gates passed.",
    ),
    edge(
        "reviewing",
        "stopped",
        "The task was cancelled or final acceptance cannot complete safely.",
    ),
];

const fn edge(
    source_state_id: &'static str,
    target_state_id: &'static str,
    guard: &'static str,
) -> StaticWorkflowTransition {
    StaticWorkflowTransition {
        source_state_id,
        target_state_id,
        guard,
    }
}

pub const WORKFLOW: StaticWorkflowDefinition = StaticWorkflowDefinition {
    title: "Task",
    goal: "Plan, confirm, document, implement, integrate, review, and deliver a complex task.",
    initial_state_id: "planning",
    states: STATES,
    transitions: TRANSITIONS,
};

pub const REGISTRATION: StaticThreadModeRegistration = StaticThreadModeRegistration {
    id: "mode.task",
    display_name: "任务",
    description: "通过计划、确认、文档、实施、整合和独立复核完成复杂任务",
    order: 20,
    prompt: PROMPT,
    workflow: Some(WORKFLOW),
};
