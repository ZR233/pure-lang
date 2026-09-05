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

The root is the sole dynamic scheduler; children cannot spawn. At the start of planning and after
each child wave, perform a cost-aware parallelization pass. Model bounded deliverables as a task DAG.
For each candidate record prerequisites, read/write ownership, suitable Profile, checkable evidence,
and whether it is root-only. A candidate qualifies for delegation only when its boundary is clear,
its work is substantial enough to repay coordination cost, it can be validated independently, and
parallel execution is expected to shorten the critical path or materially add independent evidence.
Never create agents merely to fill capacity, split tiny work, duplicate an active objective, or run
work in parallel across an unstable shared contract or a real semantic dependency.

All qualifying nodes whose prerequisites are satisfied form the ready frontier. Spawn every node in
that frontier before the wave's first `wait_agents`, `read_agent_session`, or
`read_agent_submissions`; do not wait after spawning one while another qualifying ready node remains.
While children run, the root continues only unassigned synthesis, planning, coordination, or other
root-owned work and does not repeat delegated tasks. After every pending child has receipt-bound
terminal evidence and its durable delivery is read, update the DAG and immediately dispatch the next
ready frontier. There is no fixed agent count.

During planning, partition independent evidence by crate or component, frontend/backend layer,
hypothesis, external research area, or validation surface and assign it to fresh-context `explorer`
profiles. For a genuinely complex dependency graph, one `planner` may independently challenge the
decomposition, critical path, ownership, and risks without duplicating the root. The root owns user
clarification, final synthesis, the canonical Plan, and every architecture or contract decision. For
architecture, protocol, runtime behavior, or durable conventions, the root personally updates
`design/**` before implementation.

After approval, delegate qualifying implementation nodes by dependency and file ownership.
`executor` profiles use mutually isolated writable paths behind stable contracts.
`worktree_executor` profiles own distinct branches/worktrees for cross-directory changes, manifests,
lockfiles, generated boundaries, or risky Git state; worktree isolation never removes a semantic
dependency. The root alone maintains canonical Git state and integrates results in dependency order.
Every child task must state its objective, design baseline, ownership, forbidden scope, steps,
success/failure conditions, evidence, isolation, Git, and cleanup contract. A child AgentSession
cannot read or mutate the root AgentSession Plan, so every implementation and reviewer spawn message
must contain the approved implementation baseline needed for that owned task. The root can reread its
complete approved Plan with `plan_current` throughout implementation.

Every non-reviewer child must publish a durable `CHILD_DELIVERY_READY` submission before its final
reply; a worktree delivery also includes `WORKTREE_COMMIT_READY`, a full commit ID, and workspace
root. The root waits for terminal state and reads canonical submissions by the real child ID.
Directory diffs are reviewed in place. Worktree commits are explicitly reviewed and cherry-picked
or merged, then their worktrees and temporary branches are cleaned. For a parallel worktree batch,
integrate every accepted commit before issuing the first cleanup; only then clean and verify each
child workspace. Never interleave one child's integration and cleanup while another accepted sibling
commit is still pending integration.

After integration, always create one fresh-context read-only comprehensive reviewer. When the change
is broad enough that API/error paths, tests, GUI behavior, or Git/integration risks form substantial
independent review scopes, add specialized reviewers to the same review wave. Spawn the entire wave
before waiting. Every reviewer must reach terminal state and publish a canonical durable verdict of
`REVIEWER_FINDING` or `REVIEWER_READ_ONLY_APPROVED`; root summaries and session text are not
substitutes. Every reviewer in the final wave must approve. Any finding blocks completion and returns
through the registered graph for repair, integration, and a new review wave. Only then may the root
run final gates, transition to `completed`, call `complete`, and deliver the final result. Ordinary
file, command, Git, collaboration, and final-answer capabilities are not removed by workflow states."#;

const STATES: &[StaticWorkflowState] = &[
    StaticWorkflowState {
        id: "planning",
        title: "Planning",
        instructions: "Inspect the task and architecture, ask only material clarification questions that block a complete plan, build a cost-aware task DAG, dispatch every qualifying ready exploration before waiting, and use the fixed Plan state machine rather than request_user_input or final text to obtain implementation approval.",
        completion_criteria: &[
            "The requested outcome and non-goals are explicit.",
            "Architecture and protocol impacts are grounded in repository evidence.",
            "The plan names dependency waves, ownership, isolation, root-only work, validation boundaries, and why any substantial work remains serial.",
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
        instructions: "Implement the approved plan by repeatedly dispatching the cost-qualified ready frontier. Use isolated directory and worktree children for independent owned changes, start the complete wave before waiting, and collect canonical durable deliveries and targeted tests before releasing dependent work.",
        completion_criteria: &[
            "Every qualifying ready implementation item was dispatched before its wave first wait, and every owner completed or produced an explicit failure receipt.",
            "Directory scopes are mutually isolated and worktree changes have reviewable commits.",
            "Focused tests cover the implemented behavior and regressions.",
        ],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "integrating",
        title: "Integrating",
        instructions: "As the sole canonical Git owner, review the combined directory diff, explicitly integrate worktree commits in dependency order, resolve conflicts without losing other owners' changes, and clean temporary worktrees and branches after durable integration.",
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
        instructions: "Run one fresh-context comprehensive read-only reviewer plus cost-qualified specialized reviewers for independent risk surfaces, spawn the complete review wave before waiting, consume every canonical durable verdict, then run the proportional final validation matrix. Findings must route back through the graph.",
        completion_criteria: &[
            "Every reviewer in the final wave reached terminal state and produced a canonical durable approval for the integrated head.",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_task_mode_describes_full_orchestration_contract() {
        let task = PROMPT;
        let registration = REGISTRATION;
        let workflow = registration.workflow.expect("Task Mode has a workflow");
        for state in [
            "planning",
            "editing_documents",
            "working",
            "integrating",
            "reviewing",
            "completed",
        ] {
            assert!(
                workflow
                    .states
                    .iter()
                    .any(|candidate| candidate.id == state),
                "task mode graph omits state {state}"
            );
        }
        assert!(task.contains("executor") && task.contains("worktree_executor"));
        assert!(task.contains("explorer") && task.contains("reviewer"));
        assert!(task.contains("plan_current") && task.contains("plan_submit"));
        assert!(task.contains("Never use it to ask whether to implement, proceed, or"));
        assert!(task.contains("approve a complete plan"));
        assert!(task.contains("its Approve/Revise Interaction is the only"));
        assert!(task.contains("implementation-authorization boundary"));
        assert!(task.contains("never replace Plan confirmation with a final-text question"));
        assert!(task.contains("before every transition"));
        assert!(task.contains("workflow_current") && task.contains("workflow_next"));
        assert!(task.contains("workflow_graph") && task.contains("workflow_history"));
        assert!(task.contains("Never infer CAS values"));
        for scheduling_contract in [
            "cost-aware parallelization pass",
            "task DAG",
            "ready frontier",
            "Spawn every node",
            "coordination cost",
            "shorten the critical path",
            "There is no fixed agent count",
            "root-only",
        ] {
            assert!(
                task.contains(scheduling_contract),
                "task mode prompt omits scheduling contract {scheduling_contract}"
            );
        }
        assert!(task.contains("Never create agents merely to fill capacity"));
        assert!(task.contains("one `planner` may independently challenge"));
        assert!(task.contains("one fresh-context read-only comprehensive reviewer"));
        assert!(task.contains("add specialized reviewers to the same review wave"));
        assert!(task.contains("Every reviewer in the final wave must approve"));
        assert!(
            !workflow
                .states
                .iter()
                .any(|candidate| candidate.id == "awaiting_confirmation")
        );
        for contract in [
            "objective",
            "design baseline",
            "ownership",
            "forbidden scope",
            "success/failure",
            "evidence",
            "workspace",
            "Git",
            "parallel",
            "isolation",
            "review",
        ] {
            assert!(
                task.contains(contract),
                "task mode prompt omits contract {contract}"
            );
        }
        assert!(task.contains("fresh-context"));
        assert!(task.contains("fresh-context") && task.contains("review"));
        assert!(task.contains("CHILD_DELIVERY_READY"));
        assert!(task.contains("canonical submissions"));
        assert!(task.contains("integrate every accepted commit before issuing the first cleanup"));

        let state = |id| {
            workflow
                .states
                .iter()
                .find(|candidate| candidate.id == id)
                .unwrap_or_else(|| panic!("missing task mode state {id}"))
        };
        let planning = state("planning");
        assert!(planning.instructions.contains("cost-aware task DAG"));
        assert!(
            planning
                .instructions
                .contains("ready exploration before waiting")
        );
        assert!(planning.completion_criteria.iter().any(|criterion| {
            criterion.contains("dependency waves")
                && criterion.contains("root-only work")
                && criterion.contains("remains serial")
        }));
        let working = state("working");
        assert!(
            working
                .instructions
                .contains("cost-qualified ready frontier")
        );
        assert!(
            working
                .instructions
                .contains("complete wave before waiting")
        );
        let integrating = state("integrating");
        assert!(
            integrating
                .instructions
                .contains("sole canonical Git owner")
        );
        let reviewing = state("reviewing");
        assert!(reviewing.instructions.contains("specialized reviewers"));
        assert!(
            reviewing
                .instructions
                .contains("complete review wave before waiting")
        );
        assert!(reviewing.completion_criteria.iter().any(|criterion| {
            criterion.contains("Every reviewer in the final wave")
                && criterion.contains("canonical durable approval")
        }));
    }
}
