use crate::mode::{
    StaticThreadModeRegistration, StaticWorkflowDefinition, StaticWorkflowState,
    StaticWorkflowTransition,
};
use pl_protocol::WorkflowStateKind;

pub const PROMPT: &str = r#"# Task 任务模式

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

只有实质交付存在依赖时建立 DAG，记录所有权、前置条件和局部验证边界。按可用容量派发就绪任务；
每收到一个执行者本轮完成报告即安排 fresh-context 局部静态审查，不等待整批。审查消息包含完整
最新批准计划和该任务范围、工作区、版本及验证证据。问题交原执行者修复，再派新 reviewer 复审。
各任务在 working 内独立循环，依赖满足即发布后续工作；共享接口未定或写范围重叠时不并行实现。
子代理不可再派生。执行者只做自身范围的定向验证，reviewer 不运行 shell 或测试。

本地 directory 任务局部验证和审查通过且依赖满足后直接完成，代码已在当前工作空间，无合入步骤。
worktree 任务通过后由 root 立即将已批准提交合入当前工作空间，成功后完成，不等其他执行者。
Git 操作串行，不能顺带提交其他本地任务；影响已审范围的冲突或变更需定向验证与复审。
全部任务完成后进入 integrating，核对本地任务及 worktree 合入完整性；没有工作树就不制造合入。
reviewing 由 root 对照完整计划进行跨任务审查、适用全量检查及集成功能验收，不强制再派全量 reviewer。
最终验收代码问题返回 working 交原执行者，设计问题返回 editing_documents；局部修复和复审后，
本地任务直接完成，worktree 新增修复再次合入，重新经过 integrating 与 reviewing。
执行者及 worktree 保留到最终验收通过后再关闭、清理；确认资源回收后才进入 completed。
取消或失败保留未交付现场，不能用附带免责声明代替完成要求。

最终用自然 final 或 finish_turn({message}) 交付一次结果、验证和剩余问题。报告事实来源和实际
验证记录，不使用固定口令、不机械重跑有效证据、不因已确认的相同环境故障重复派探测任务。
"#;

const STATES: &[StaticWorkflowState] = &[
    StaticWorkflowState {
        id: "planning",
        title: "规划",
        instructions: "核对任务与架构，只澄清影响完整计划的实质问题。仅为有实际依赖的交付建立任务 DAG，按容量派发就绪探索；通过固定 Plan 状态机确认完整计划，不用 request_user_input 或最终回复代替。",
        completion_criteria: &[
            "预期成果和非目标明确。",
            "架构及协议影响有仓库事实依据。",
            "计划明确所有权、局部验证与主代理验收；实质并行任务还明确依赖、隔离及保持串行的原因。",
            "plan_current 返回当前完整计划为 approved。",
        ],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "editing_documents",
        title: "更新设计文档",
        instructions: "架构、协议、运行时或持久化契约变化由主代理先更新权威设计文档，并核对其与已批准计划一致。",
        completion_criteria: &[
            "实施前已更新所有受影响设计契约。",
            "权威文档不保留过时兼容契约或将 Mode 作为 Skill 的约定。",
        ],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "working",
        title: "实施与局部审查",
        instructions: "按容量派发所有权独立的任务。单个执行者交付后立即安排 fresh-context 局部静态审查，提供完整批准计划和对应版本的证据。问题交原执行者修复，各任务独立复审。本地 directory 任务通过后原地完成；worktree 提交通过且依赖满足后串行合入，不等其他执行者。执行者只做局部验证，审查者不运行测试。",
        completion_criteria: &[
            "每项计划实施任务都有当前版本的局部审查批准和验证证据，未解决失败不算完成。",
            "directory 任务在当前工作空间直接完成，无合入操作；worktree 成果已获批准、依赖满足且完成合入。",
            "适用的定向验证覆盖实现行为和回归；不适用项说明原因。",
        ],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "integrating",
        title: "核对交付与合入",
        instructions: "对照计划核对全部本地任务和增量合入的工作树成果，不为 directory 改动制造合入操作。核对冲突或依赖变化后的审查证据仍有效，受影响任务返回局部修复和审查。执行者及 worktree 保留至主代理最终验收通过。",
        completion_criteria: &[
            "全部计划本地任务已原地完成，所有已批准工作树成果在当前工作空间中恰好合入一次。",
            "工作树提交有记录，执行者及其工作区仍可用于返工。",
            "验证记录区分有效复用证据与待执行集成检查，不机械重跑测试套件。",
        ],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "reviewing",
        title: "整体集成验收",
        instructions: "主代理亲自审查跨任务交互，对照完整计划检查最终工作空间，并执行适用的全量检查、集成及功能验收。可复用有效局部证据，但不能替代必要最终门禁。不强制额外派发全量审查子代理。代码缺陷经 working 交原执行者修复，设计缺陷回 editing_documents；验收通过前保留资源。",
        completion_criteria: &[
            "所有任务范围均有当前有效审查批准，主代理逐项对应计划要求、实现和实际验收证据。",
            "不存在未解决的设计或代码问题。",
            "所有必要的确定性检查及明确要求的 live 验收均有终态证据，已报告验证记录和验收通过后的资源清理。",
        ],
        kind: WorkflowStateKind::Atomic,
    },
    StaticWorkflowState {
        id: "completed",
        title: "已完成",
        instructions: "",
        completion_criteria: &[],
        kind: WorkflowStateKind::Final,
    },
    StaticWorkflowState {
        id: "stopped",
        title: "已停止",
        instructions: "",
        completion_criteria: &[],
        kind: WorkflowStateKind::Final,
    },
];

const TRANSITIONS: &[StaticWorkflowTransition] = &[
    edge(
        "planning",
        "editing_documents",
        "固定 Plan 状态机确认有事实依据的完整计划为 approved。",
    ),
    edge("planning", "stopped", "任务已取消或无法安全继续。"),
    edge(
        "editing_documents",
        "working",
        "所需权威设计文档均与已批准计划一致。",
    ),
    edge(
        "editing_documents",
        "stopped",
        "任务已取消或无法形成一致设计。",
    ),
    edge(
        "working",
        "integrating",
        "全部本地任务通过局部审查与验证，所有工作树成果已获批准、依赖满足且完成合入。",
    ),
    edge("working", "stopped", "任务已取消或实施无法安全继续。"),
    edge("integrating", "working", "交付核对发现实现缺陷或缺失成果。"),
    edge(
        "integrating",
        "reviewing",
        "全部计划本地任务和工作树合入均已核对，具有有效局部审查和验证证据。",
    ),
    edge(
        "integrating",
        "stopped",
        "任务已取消或无法安全完成交付核对与合入。",
    ),
    edge(
        "reviewing",
        "working",
        "主代理验收发现实现缺陷，需要局部修复和新的审查。",
    ),
    edge(
        "reviewing",
        "editing_documents",
        "验收发现架构或契约缺陷，需要修改设计。",
    ),
    edge(
        "reviewing",
        "completed",
        "主代理已验证全部计划要求，必要最终门禁通过，无遗留问题，且已确认子代理及工作树清理。",
    ),
    edge("reviewing", "stopped", "任务已取消或无法安全完成最终验收。"),
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
    title: "任务",
    goal: "通过计划确认、设计、独立实施与审查、工作树增量合入和主代理整体验收完成复杂任务。",
    initial_state_id: "planning",
    states: STATES,
    transitions: TRANSITIONS,
};

pub const REGISTRATION: StaticThreadModeRegistration = StaticThreadModeRegistration {
    id: "mode.task",
    display_name: "任务",
    description: "通过独立实施与审查、工作树增量合入和主代理整体验收完成复杂任务",
    order: 20,
    prompt: PROMPT,
    workflow: Some(WORKFLOW),
};
