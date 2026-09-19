use crate::mode::{
    StaticThreadModeRegistration, StaticWorkflowDefinition, StaticWorkflowState,
    StaticWorkflowTransition,
};
use pl_protocol::WorkflowStateKind;

pub const PROMPT: &str = r#"# Task Thread Mode

主代理拥有一项 canonical task，负责用户澄清、Plan 确认、设计、调度、整合和交付。子代理协作、
详细派发和统一 Turn 汇报遵循系统提示的唯一合同；本 Mode 不建立第二套交付状态或 marker。

先理解目标和现场，只有缺少实质信息才询问用户。完整方案通过 plan_current/plan_submit 确认；
plan_current 返回 approved 后才从 planning 进入 editing_documents。设计变更由 root 先更新
设计文档。平凡任务可以简短计划和直接实施，不为形式创建 DAG 或 explorer；派发说明仍须完整。

框架已注册 workflow 图，不得提交、编译或替换图。使用 workflow_current、workflow_next、
workflow_graph、workflow_history 查询；每次转换前独立只读批次重新读取 current/next，首次
和终态转换还读 graph/history。按返回的 runId、revision、当前状态和直接后继调用 solo
workflow_transition，参数为 expectedRunId、expectedRevision、expectedStateId、targetStateId
和 completion:{reason,summary,evidence}。workflow_restart 仅用于显式新尝试。

只有实质交付存在依赖时建立 DAG，记录所有权、前置条件和验证边界。派出整个可执行前沿再 wait；
等待期间 root 处理未委托工作。阅读所有对应本轮的终态报告后更新依赖，发布下一批工作。
共享接口尚未确定或写范围重叠时不并行实现。子代理不可再派生。

实现后进入 integrating，检查目录 diff、顺序整合 worktree commit，保留执行者和 worktree。
整合后派 fresh-context 只读 reviewer；独立风险面可并行审查。审查者明确批准或阻塞问题；
代码问题返回 working 交原执行者，设计问题返回 editing_documents。修复后重新整合与审查。
全部审查批准且最终验证完成后关闭本任务创建的所有子代理并清理已接受的 worktree，确认资源
回收后才进入 completed。取消或失败保留未交付现场，不能用附带免责声明代替完成要求。

最终用自然 final 或 finish_turn({message}) 交付一次结果、验证和剩余问题。报告事实来源和实际
验证记录，不使用固定口令、不机械重跑有效证据、不因已确认的相同环境故障重复派探测任务。
"#;

const STATES: &[StaticWorkflowState] = &[
    StaticWorkflowState {
        id: "planning",
        title: "Planning",
        instructions: "Inspect the task and architecture, ask only material clarification questions that block a complete plan, use a task DAG only for substantial dependent deliverables, dispatch every qualifying ready exploration before waiting, and use the fixed Plan state machine rather than request_user_input or final text to obtain implementation approval.",
        completion_criteria: &[
            "The requested outcome and non-goals are explicit.",
            "Architecture and protocol impacts are grounded in repository evidence.",
            "The plan names ownership and validation; substantial parallel work additionally names dependency waves, isolation, and why any substantial work remains serial.",
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
        instructions: "Implement the approved plan by repeatedly dispatching the cost-qualified ready frontier. Use isolated directory and worktree children for independent owned changes, start the complete wave before waiting, and collect current Turn reports and targeted tests before releasing dependent work. For findings, resume the original executor with send_message and collect fresh turn-bound evidence.",
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
        instructions: "As the sole canonical Git owner, review the combined directory diff, explicitly integrate worktree commits in dependency order, resolve conflicts without losing other owners' changes, and retain implementation agents and worktrees for possible rework until final approval and validation.",
        completion_criteria: &[
            "All accepted child deliveries are present exactly once in the canonical workspace.",
            "Worktree commits are recorded and their agents/workspaces remain available for rework.",
            "Verification records identify valid reused checks and missing integrated-tree checks without mechanically rerunning suites.",
        ],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "reviewing",
        title: "Reviewing",
        instructions: "Run one fresh-context comprehensive read-only reviewer plus cost-qualified specialized reviewers for independent risk surfaces, spawn the complete review wave before waiting, consume every canonical Turn report, then satisfy the proportional final validation matrix using valid evidence and justified missing checks before cleanup. Findings must route back through the graph to their original executor.",
        completion_criteria: &[
            "Every reviewer in the final wave reached terminal state and produced a canonical review approval for the integrated head.",
            "No unresolved design or code finding remains.",
            "All required deterministic and live acceptance gates have terminal evidence; verification records and post-approval cleanup are reported.",
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
