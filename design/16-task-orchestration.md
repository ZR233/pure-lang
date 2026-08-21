# 16 - Simple / Task 与 TaskService

## 16.1 模式

root Thread 模式只有 `simple | task`。

- Simple：root 使用 executor role，可直接实现；只允许派生只读 explorer。
- Task：root 使用 planner role。planner 负责计划、设计、executor、review、merge、冲突和完成。

root Thread 的 `mode` 是唯一事实源；root role 只是运行时派生投影，不再是独立不变量。agent
注册、Turn 构建时的 instructions、模型路由、workspace 与 execution policy 都按 mode 派生 root
角色（Simple → executor，Task → planner）。切换 mode 只允许 root Thread、没有活动 Task、actor
idle 且没有 pending input；StudioRuntime 持 lifecycle 临界区后单次原子持久化 mode/role 目录记录，
再尽力把进程内 actor 角色同步为新值，失败只告警不回滚——提交 prompt 时 reconcile 会自愈残余
漂移，Turn 构建也不因角色陈旧而拒绝。不存在启动修复步骤。Task root 永远不能进入 executor
WorkUnit 生命周期。

每个 agent 固定对应一个 Thread。child 通过 `rootThreadId`、`parentThreadId`、`role` 和
`agentPath` 表达关系。TaskRun 只绑定 root Thread；executor 和 reviewer 直接由 WorkUnit、
ReviewRound 引用，不建立 AgentOutcome 镜像。

## 16.2 所有权

`TaskService` 位于 `pl-studio-runtime`，管理：

- `TaskRun` 聚合根及其领域状态机；
- WorkUnit 与不可变 WorkCompletion；
- ReviewRound；
- MergeRecord 与 Planner Git 记账；
- BranchLease、worktree ownership 与安全清理。

Thread/Turn 的执行状态只从 Thread repository 读取。Task 状态从产品表直接组成
`TaskSnapshot`，只进入 product stream。planner 执行的 Task 工具仍是 planner Thread 自己的
toolCall Item。

## 16.3 Task 状态

计划草拟和确认属于 TaskRun 创建前的 root Thread interaction 生命周期。用户确认实施后才创建
TaskRun，初始状态固定为 `DesignUpdating`；`Planning` 和 `PendingConfirmation` 不是 TaskRun 状态。
主要生命周期为：

```text
DesignUpdating --FinalizeDesign--> Implementing
Implementing/Reworking --delivery ready--> Reviewing(Delivery) --> Merging
Reviewing --changes required--> Reworking
Merging --tree changed--> Reviewing(Integration)
Merging --approved tree unchanged--> Completed
Implementing/Reworking --no delivery--> Completed

任意非终态 --request stop--> Stopping --> Cancelled | Failed
任意非终态 --recoverable conflict--> Blocked --typed recovery--> Merging | Reworking
```

`TaskRun` 是聚合根：`TaskContext` 只承载身份、确认计划、仓库身份、分支、base/expected HEAD；
唯一状态事实源是带数据的 `TaskRunState` enum。每个状态 payload 是独立的 crate-private struct，
分别位于 `task_run/state/<state>.rs`；`task_run/state/mod.rs` 只声明模块、定义 enum 和导出内部稳定
入口。设计结果、审查目标、停止请求、阻塞恢复和失败信息只存在于适用状态中。设计后的状态组合
持有 `FinalizedDesign { head, commit, summary, fingerprint }`；`commit` 可空，因此“设计已经
finalize”和“设计阶段产生了提交”是两个独立事实。`ReviewingState` 使用
`ReviewTarget::Delivery | Integration`，`BlockedState` 只接受
`BlockedRecovery::RetryMerge | ResumeRework | ManualOnly`。`Blocked` 不是终态；只有
`Completed`、`Failed`、`Cancelled` 是终态。

状态变化只由穷尽的 `TaskCommand` 驱动，并返回
`TransitionDecision { next_state, durable_effects, external_effects }`。状态模块只判断领域规则并
声明效果，不访问 SQLite、Git、Thread 或网络；adapter 在对应事务和 lifecycle 边界解释效果。
不存在通用 `can_transition_to`、任意同态 set-state API、`dyn State` 或泛型 typestate。

WorkUnit 把 lifecycle 与 executor execution 合并为唯一的 `WorkUnitState`，不能分别写入
`status + executionStatus` 形成非法组合。ReviewRound 同理使用唯一 `ReviewRoundState`，reviewer
执行进度包含在 pending/failed payload 中，不再独立持久化 `status + reviewerStatus`。WorkUnit
的身份列直接保存 executorThreadId、requestedByCallId、attempt、scopeHints、baseCommit、worktree
和 branch；ReviewRound 的关系列保存 reviewerThreadId、scope 和目标 completion/HEAD，冻结的
changed-files、逐文件覆盖与 findings 保持独立审计数据。Delivery round 的文件清单直接复制不可变
WorkCompletion；Integrated round 在 branch mutation lock 内按 Task baseCommit 到 expectedHead
计算，并与审查 diff 使用同一 pathspec、排除 `design/**`。Rename/Copy 的旧、新路径都属于审查
目标；删除、二进制、生成文件、lockfile 和 migration 不因文件类型被过滤。

每条 `ReviewFinding` 必须给出可执行的 `recommendation`：写清改成什么、为什么，必要时附内联
片段或精确到函数/行号的最小改法，让 executor 据此直接 rework；`review_exit` 校验会拒绝
`changesRequired/blocked` 下缺失 `recommendation` 的 finding。`task_status` 的 reviews 只投影
概览（verdict/summary/findings_count/has_recommendations 和文件覆盖计数），省略 findings 与路径
明细；planner 用
`read_review_round(roundId, offset, limit)` 分页读取单轮完整 findings（含 recommendation），
用 `read_review_file_coverage(roundId, diagnosticsRevision, category, offset, limit)` 分页读取冻结
文件、覆盖状态和最近一次拒绝诊断，保证不被默认输出预算截断。

WorkUnit 状态为 Pending、Running、AwaitingCompletion、ReadyForReview、Reviewing、
ChangesRequested、Approved、Merged、NoDelivery、NeedsAttention、Failed、Cancelled。
各状态 payload 持有适用的 worktree disposition、execution summary/error、budget、typed
continuation、来源 Turn 与 continuation revision；这些事实是重启恢复和幂等续轮的 canonical
owner，不能从 Timeline 文本推断。

`task_runs`、`work_units` 和 `review_rounds` 各自只保存一份完整 `state_json` 和单调 `revision`。
`state_kind` 是从 `state_json.kind` 生成的 SQLite stored column，只用于 CHECK、索引和 partial unique
constraint，不能由应用写入。TaskRun 的更新使用 `id + revision` CAS；状态内的有效进度变化也递增
revision。每个 root Thread 只能拥有一个非终态 TaskRun，`Blocked` 也计入该唯一约束。数据库更新、
关联产品记录与 BranchLease 变更在同一事务提交；Git 操作留在事务外，通过 operation ID、expected
HEAD 与内容指纹校验。当前 schema 为不兼容版本；旧 fingerprint 直接重建运行时数据库，不解析旧
状态字符串、不迁移旧列。

Task 产品 wire 使用独立的 `StudioTaskState` tagged enum，只投影产品所需 payload，不直接暴露领域
state struct。HTTP snapshot、SSE、recovery、FRB DTO 和 Dart domain 一次性使用 typed union；Flutter
对 variant 穷尽 switch，不保留 `phase: String` 或未知字符串 fallback。Task phase 不复制进 Thread
runtime snapshot，Thread 状态也不缓存进 Task 表。

Task 生命周期工具只在 Turn 准备时已经解析到 active TaskRun 时安装。规划确认前的 Task root
没有 TaskRun，因此不暴露 `task_status`、executor/review/merge/complete 等 TaskService 工具，只保留
通用能力和规划阶段的 `plan_exit`。`plan_exit` 只属于 Task root planner；所有 child Agent 无论角色、
阶段或模型判断都不得看见或调用该工具。root planner 与 child 使用角色隔离的 execution profile：
explorer 只读探索并向父 Agent 汇报，不创建、提交或确认 Task 计划，也不调用 Task 生命周期工具。
创建 TaskRun 后启动的 fresh Turn 根据 active run 与最新 phase 获得相应工具集合；已经开始的 Turn
持有固定工具 lease，不在运行中动态增删工具。

Studio 的 Simple root、explorer、reviewer 和 executor 允许 `ToolEffect::Process`。Task root planner
在规划与待确认阶段不暴露 `exec` / `write_stdin`，只能使用受路径边界约束的只读文件工具，或把
明确问题交给只读 explorer，防止用户确认前直接实施。进入 `DesignUpdating` 后，planner 恢复
Process 与 WorkspaceWrite，可继续探索、运行命令，并用普通文件工具修改仓库内任意文件；
`Merging` 同样拥有这两种 effect。`Implementing`、`Reviewing`、`Reworking` 等其他阶段仍通过
Task 生命周期工具与 child Agent 推进。现有角色权限、路径边界与 Git mutation 工具策略不因阶段
放宽而绕过；executor 仍只拥有自己的 canonical worktree。

`DesignUpdating` Turn 必须以成功的 `task_finalize_design` 作为 required finalization tool。该工具
只收束设计阶段，不要求文件变化：无变化时直接记录完成，有变化时把当前 Task 拥有的 workspace
变化提交为阶段基线。普通 final 文本、`exec` 或其他工具结果不能把该 Turn 标成成功；模型未完成
设计阶段时形成 typed validation failure，TaskRun 保持可恢复的 `DesignUpdating`。

## 16.4 Planner 与等待

Task root 只允许 planner 创建 explorer；executor 通过 `task_spawn_executor` 创建，reviewer
通过 delivery/integrated review 工具创建。executor/reviewer depth 固定为 1。只有 root Thread 中
role、agent identity 与持久化 Thread 记录一致的 planner Plan trace 才能投影为 PlanConfirmation；
child 或身份不匹配的 Plan trace 仅保留为 trace，不创建 Interaction。PlanConfirmation 的 scope
必须持久化产生它的 agent identity，恢复扫描复用同一资格校验，不能把历史 child trace 补投为确认。
启动恢复以 `plan-confirmation-{planId}` 为稳定身份，在访问短命 runtime actor 前先读取持久化
Interaction 与活动 TaskRun：任何状态的既有 Interaction 都表示投影已发生，活动 TaskRun 也表示该
计划已进入实施，两者都直接跳过。只有确实缺少 Interaction 且没有活动 TaskRun 的最新 root plan
才需要检查 idle agent 并补投 pending confirmation；已 resolved 的历史计划或 terminal Task 不得因
agent 不驻留而产生伪恢复 warning。

`wait_agents` 订阅 Thread directory watch 后读取 snapshot，只因 progress、interaction 或
terminal 变化返回，并以 `messages` 返回本次最新增量；terminal 消息直接携带 canonical
`lastTurnOutcome`，包含 budget kind/usage、reason、rollover 结果和 Turn identity，不复制只含枚举
的 outcome 字段。planner 直接消费该结果，不在 wait 之后调用 `list_agents` 重复刷新完整目录。
child 已有新的 `activeTurnId` 时，即使瞬态 activity 尚为 idle，也必须视为正在运行并隐藏上一 Turn
的 terminal outcome；否则恢复消息后的首次 wait 会把旧 BudgetLimited 误当成新 Turn 已结束。
没有轮询、自动续轮或超时中断。五分钟仅允许 planner 读取有界 child Thread 诊断，不是失败判据。

该 wait 输出协议不迁移旧历史；旧会话或 fixture 不兼容时直接重建。

review request 成功创建 reviewer 后必须结束当前 planner Turn。reviewer 提交 durable verdict 后，
Runtime 以稳定 mail ID 提交一次隐藏 continuation；root Thread 已 idle 时立即启动，仍有活动 Turn 时
只排入下一 Turn，绝不 steer 旧 Turn。新的 planner Turn 从最新 Task state 重新解析 canonical
workspace 与 tool policy。活动 Task 的 Process 与 WorkspaceWrite effect 由状态机 capability 统一
决定，只在 `DesignUpdating` 与 `Merging` 可用；主 workspace 的 Git 合并和记账职责只在 Merging
执行。旧 Turn 的固定 lease 不能取得新 state 才安装的 Task 生命周期工具。

review verdict、reviewer terminal failure 和 executor completed/failed terminal outcome 都先提交到 Task
repository，再派生稳定 ID 的 Planner wake。executor failure 只唤醒 planner 读取 `task_status` 并
决定是否向原 WorkUnit/Thread follow-up，不自动重跑模型、不创建第二个 WorkUnit 或 agent。wake
投递失败不会把已提交的 Task 事实改成 Blocked；启动恢复按 durable source 与 `thread_inputs`
对账，只补投完全缺失的 mail，queued、claimed、active 或 consumed 均视为已交付。因此可重试的
是幂等 mailbox materialization，而不是业务执行。重复事件、恢复扫描和并发提交共享同一 mail ID，
不产生第二次 Planner Turn。

Task Planner wake 是 level-triggered 通知：同一 TaskRun 在 root Thread 中排队的 wake 共享通用
queue coalescing key。root idle 时首个 wake 立即启动；root 活跃时，队首连续的 executor/review
wake 在下一 Turn 启动前合并，最新 wake 决定 Turn identity，较早 mail 作为 durable leading input
由同一 checkpoint 消费。planner 始终读取 canonical `task_status`，不按 wake 文本重放旧事实。
已经 claimed/active 的 wake 不与后到事实合并，保证 Turn snapshot 之后发生的新变化仍能排入下一
Turn；用户输入、交互回复和其他 mailbox 类型不参与该 key。这样 provider 重试、review verdict 与
恢复扫描不会为已被同一 Planner Turn 覆盖的事实创建多个模型 Turn。

Studio 的 `request_user_input` 对 Simple、Task root 和可用 child 一律形成 durable fresh-turn
边界：pending Interaction 先经 ThreadActor/ThreadRepository 提交，原 Turn 再结束；用户答复以
`interaction-resolution:{interactionId}` 作为稳定 mail ID，把 resolved Interaction 与 hidden input
在同一 Thread repository 事务提交。idle root/child 立即启动新 Turn，活动中的任何 Turn 都只令
该 input 排队，绝不 steer；它没有 Planner wake coalescing key。重复答复按 Interaction 状态和
稳定 mail ID 幂等，进程重启后也只能 materialize 一次。Interaction resolution 只更新 Interaction
并提交 fresh durable input，不能修改或复活 terminal origin Turn，也不能覆盖无关活动 Turn。
ToolApproval 继续使用原有审批流程。PlanConfirmation 的 `ContinuePlanning` 仍属于 Planner phase，
但回答中的调整
要求必须与 resolved Interaction 在同一个 Thread repository 事务写入 hidden durable input，复用
`interaction-resolution:{interactionId}`、StartOrQueue、no-steer、无 coalescing 的规则；fresh Planner
Turn 必须再次调用 `plan_exit` 生成新的确认。`ImplementFreshContext` 同样必须把 resolved
Interaction 与 hidden implementation input 作为 durable fresh-turn 边界提交；先建立 TaskRun，再由
fresh Planner Turn 读取 active Task 并获得 `task_finalize_design` 等 Task 工具，绝不把实施输入 steer
回提出计划的 origin Turn。`Dismiss` 保持既有忽略语义。

Task planner/executor/reviewer 的 required finalization tool 只约束业务阶段完成，不约束 durable
UserInput 边界。原 Turn 因 pending Interaction 结束时必须保存为 completed，不能因为尚未调用
`plan_exit`、`report_completion` 或 review exit 工具而标成 failed；fresh Turn 恢复后仍继续执行原
finalization policy。

进程重启后不为普通 paused Task 自动启动模型；但崩溃前已经 durable 形成的 pending Planner wake
或 mailbox input 必须在资源恢复完成后继续交付。活动 Task 无 pending input 时显示 paused；用户
“继续任务”以稳定 mail ID 向 root Thread 提交一次隐藏的明确输入，要求 planner 先读取
`task_status` 和 `list_agents`。attach 只恢复已有 durable 工作，不为单纯 active Task 合成新工作。

## 16.5 Executor 与交付

`task_spawn_executor` 接收一份自包含的实施蓝图：`taskName`、`objective`、分组的 `scope`、
`implementationSteps`、`acceptanceCriteria`、`dependencies`、`evidence` 和 `verification`。
每个 WorkUnit 只承担一个可独立验证的成果。planner 必须先完成足以形成蓝图的探索；关键方案仍
未知时继续探索或自行处理，不能用空泛说明把方案发现转嫁给 executor。下一步立即依赖的关键工作
不应派发后原地等待；可并行工作尽量使用不重叠的预计修改面，存在依赖或明显重叠时串行派发。
scope path 用于拆分、审查和冲突提示，不是 executor worktree 的硬性写入权限。

`scope.inScope`、`scope.scopeHints`、实施步骤、验收条件和至少一条命令验证均非空；
`scope.outOfScope` 必须显式出现但可为空。每个步骤包含唯一 ID、具体指令、至少一个仓库相对
目标路径（可带 symbol）、预期结果和验收条件引用；每条验收条件必须同时被至少一个实施步骤和
至少一项命令或检查覆盖。命令验证固定唯一 ID、命令、仓库相对 cwd、目的、预期结果和验收条件
引用；只读检查同样固定 ID、指令、目标、预期结果和引用。所有标识唯一、引用有效且不得重复，
路径与 cwd 必须是规范仓库相对路径，整个 handoff 受固定上下文预算约束，输入拒绝未知字段。
这些校验以及 pinned section 大小验证必须在分配 worktree、BranchLease、WorkUnit 或 child Thread
之前完成，失败不得留下任何资源或子对话。

每次 executor allocation 同时生成第二版且唯一命名的 `TaskExecutorHandoff`。handoff 按运行归属、
仓库事实、确认计划、实施蓝图和交付规则分组，固定 TaskRun/WorkUnit、parent Thread、
requestedByCallId、确认计划、base/design-finalized/expected HEAD、完整蓝图和交付契约，并作为
`studio.task_executor_handoff` pinned section 随 fresh child session 持久化。运行时对规范化蓝图
计算稳定内容指纹；同一 provider call 或新重试只有完整指纹一致时才复用既有 WorkUnit。taskName、
scope 相同但步骤、验收或验证不同是稳定冲突，不能复用或重新分配。后续 Turn 从 durable WorkUnit
与该 section 交叉校验；缺失、损坏、旧版 handoff 或 HEAD/owner 不一致时进入 NeedsAttention，
不迁移或兼容第一版。只有原 WorkUnit 已 terminal，或 TaskRun 已明确进入 Reworking，才允许创建
新的 attempt。

executor 使用全新上下文，初始消息由 runtime 固定生成，只要求读取 pinned handoff 并按顺序开始；
不得复制一份可能漂移的自由文本 assignment。executor 可调整不改变目标与验收语义的低层实现；
若现场事实与蓝图冲突或必须扩大任务语义，必须保留证据并通知 planner。planner、恢复流程和
delivery reviewer 均通过只读 `read_work_unit_handoff` 从持久化 working state 取得同一份 handoff，
不能依赖活动 actor 或对话历史。通用只读 explorer 仍使用通用派发协议，但必须获得明确问题、
检索范围、期望证据和返回格式，不扩展为 Task 实施蓝图协议。

executor 只能写自己的 worktree，并以以下工具结束可交付工作：

```text
report_completion {
  kind: delivery,
  headCommit,
  verificationResults: [{ checkId, summary }]
}
| report_completion {
  kind: noDelivery,
  verificationResults: [{ checkId, summary }]
}
```

完成结果使用顶层 tagged object；`kind`、`headCommit` 与 `verificationResults` 都是工具的
顶层字段，不再包在 `result` 对象中。对 provider 暴露的 JSON Schema 保持单一 object +
properties 形状，不在根节点使用 `oneOf`；`headCommit` 在 schema 中可选，运行时再按 `kind`
穷尽执行条件校验：delivery 必须提供，noDelivery 必须省略。这样既避免嵌套 union 被编码成
JSON 字符串，也兼容不能稳定生成根 `oneOf` 参数的 provider，并继续拒绝未知字段。

`verificationResults` 必须恰好覆盖 handoff 中全部 command 与 inspection ID；缺失、重复或未知 ID
均拒绝。summary 非空，结果按 handoff 的稳定顺序生成现有 WorkCompletion 人类可读验证摘要，
不修改数据库格式。任何检查失败时 executor 只能继续修复或报告阻塞，不能提交 completion。

delivery 要求 worktree clean、HEAD 相对固定 base 推进、commit 身份一致，并记录完整
base-to-HEAD changed files；worktree 内变更不受 scopeHints 限制。成功事务创建不可变
WorkCompletion 并将 WorkUnit 置为 ReadyForReview。普通文本
结束、工具错误或预算中止不会伪造交付，WorkUnit 保持 AwaitingCompletion，可由 planner 向同一
Thread 发送明确 follow-up。follow-up 或 changes-requested rework 开启新的 executor Turn 时，
WorkUnit 与 Thread execution 必须在同一事务中恢复为 `Running/Running`，清除旧 execution error
并推进 continuation revision；重复的 TurnStarted 对已处于 `Running/Running` 的组合保持幂等。
不得持久化 `AwaitingCompletion/Running` 或 `ChangesRequested/Running` 这类重启校验无法接受的
中间组合。

executor Turn 被取消后可能形成 `AwaitingCompletion/Cancelled` 的 durable 组合。planner 首次关闭
该 executor 时必须在同一事务中归一为 `Cancelled/Cancelled` 并请求清理；后续重复关闭返回同一
discard disposition，不再次推进 revision。ReadyForReview、Reviewing 或 ChangesRequested 的
completion review 仍禁止关闭。

executor 的单个 Turn 保持 30 分钟 wall-clock 上限。前三个 `WallClock` budget terminal 不把
executor 或 WorkUnit 标记为失败：runtime 通过唯一 compaction controller 对同一 Thread 强制执行
`WallClockRollover`。attached Turn 复用原 CancellationToken，provider-backed compaction 受 120 秒
硬超时约束；取消、超时或错误不得阻止当前 Turn 提交 terminal。成功后以
`workUnitId + sourceTurnId` 生成确定性 hidden continuation input，在同一 worktree 开启下一切片。
一个自动 tranche 最多四个切片；第四次 wall-clock 耗尽、非 wall-clock budget 或 rollover 失败
进入 NeedsAttention，保留 executor/worktree，并形成稳定 Planner wake。pending continuation 与
Planner wake 在重启时分别按幂等键对账已有 active/terminal Turn 和 queued/claimed/active/consumed
mail，禁止重复增加切片或启动 Turn。rollover replacement transcript 必须先与 TurnFinished 在
repository 提交链上持久化成功，再允许 hidden continuation 入队；提交失败时 actor 不推进内存
session，也不启动下一 Turn。

planner 用统一的 `send_message`（parent→direct-child）向子代理下发调度或恢复消息；不增加 Task
专用恢复工具。每次成功接受的消息都刷新 child budget。活动 Turn 不被中断，但 wall-clock 和
本 tranche 的 model/tool/wait 计数从消息接受时重新开始；idle child 开启 fresh Turn。对应 WorkUnit
的 budget tranche 重置为第一片，清除上一 tranche 的 budget/error/source。预算型 NeedsAttention
可由该消息恢复为同一 executor/Thread/WorkUnit/worktree 的 `Running/Running`；handoff、ownership
或其他非预算 NeedsAttention 继续拒绝恢复。自动 `PendingStart` continuation 不刷新 tranche。

UserInput 的 fresh-turn 边界不扩大上述预算续轮范围。普通 Planner、reviewer、Simple 或 child
`budgetLimited` 仍是 terminal 事实，不自动合成 continuation；只有这里定义的 executor
`WallClockRollover` 可以按 WorkUnit tranche 状态机续轮。

WorkUnit 在 ReadyForReview 之后以 `executorAgentId` 创建 fresh Delivery reviewer。ReviewRound
事务固定最新 Completion revision，reviewer canonical workspace 直接绑定同一 worktree，不接受
模型提供路径。Reviewer 必须在 `review_exit.fileReviews` 中为冻结清单的每个规范仓库相对路径提交
`reviewed: true`；服务端精确拒绝缺失、false、重复、额外、绝对或非规范路径。该标记声明 Reviewer
已经结合 prompt 中完整 diff 审查该文件，不要求每个文件都有独立 `read_file` trace。findings 使
WorkUnit 进入 ChangesRequested；
planner 把具体 finding 发回原 executor Thread，新的 completion revision 重新审查。pass 后
WorkUnit 进入 Approved 或 NoDelivery。executor 在普通结束或失败时若没有形成新的
Completion，WorkUnit 保留可 follow-up 的 durable terminal execution 状态，并生成一次 Planner
wake；review changes-requested 后的 rework failure 也走同一路径，不能静默停在
`AwaitingCompletion/failed`。取消由既有 stop/cancel 收束处理，不额外唤醒 Planner。

Reviewer 在产品语义上仍是只读角色，不得通过 shell 修改 workspace、Git 或其他现场。审查前可以
使用 `list_files`，或通过 `exec` 运行 `rg` / `rg --files` 定位设计和代码；定位之后仍必须用
`read_file` 阅读至少一个相关 `design/**` 文档，才能提交 `review_exit`。Reviewer 的中文审查规则
保存在独立 Markdown prompt 模板中；模板要求检查所有 changed files、调用点、测试、错误路径和
跨文件交互，发现第一个问题后继续并一次提交所有确定、离散、可执行的 finding，同时排除推测、
既有问题、刻意变更和纯风格 nit。

`review_exit` 返回 tagged outcome。文件覆盖或其他可修正输入门禁失败时返回
`rejected { code, recoverable, message, diagnosticsRevision, coverage, violations }`，工具失败但不结束
Reviewer Turn；一次诊断聚合 missing、unreviewed、duplicate、extra、invalid path 及其他输入问题，
小集合直接完整返回，大集合提供总数、稳定预览和分页 revision。拒绝只更新 ReviewRound 的最近
覆盖尝试与诊断，保持 Pending，不推进 WorkUnit/Task phase，也不产生 Planner wake。只有覆盖完整且
其他门禁通过时返回 `accepted`，在同一事务提交全部文件标记、verdict、summary 与 findings，随后
物化一次幂等 Planner wake 并结束 Turn。旧数据库中的历史 round 若没有覆盖字段，明确投影为
coverage unknown，不推断为已审查。

每个 Agent Turn 结束时，Studio 从结构化 `TurnFailure` 派生独立的
`TaskFailureDisposition`。capacity、transport、408/409/425/429、5xx 和普通验证失败为
Recoverable；authentication、authorization、configuration、provider protocol、fatal tool runtime、
internal invariant 以及未知永久 provider failure 为 Fatal。
该判断只使用 typed category/kind/retry，不解析 message。

Recoverable child failure 保留 WorkUnit/Review 可 follow-up 状态并产生一次 Planner wake；
Recoverable root failure 保持当前 Task phase，等待用户修复配置后继续，不自动重放 Turn。Fatal
failure 以来源 Turn ID 幂等写入 `task_failures`，首个 fatal 在同一 SQLite immediate 事务中把
TaskRun 置为 Failed、固定 terminal failure、收束未完成 WorkUnit/Review 并删除 BranchLease。
事务提交后才中断其余 Task agent。现有 worktree disposition 保持 Protect，branch 和物理成果不
删除；迟到 completion、review、wake 或第二个 fatal 不能覆盖已提交终态。

## 16.6 Planner 自主 Git、合并记账与综合审查

Approved 且 executor 已关闭的 delivery 由 `task_status` 投影为 `MergeCandidate`，包含 executor、
completion revision、相对 worktree locator、branch、base/head commit 与 Task expected HEAD。
TaskService 不执行 merge，也不提供专用 conflict 文件工具。Planner 在 Task 主 workspace 使用普通
exec/file/Git，自行选择 merge、cherry-pick、squash、rebase 或 manual，并自行解决或 abort 冲突。
冲突期间 Task phase 仍是 Merging，不创建独立 conflict state 或持久化 conflict tool session。

Git 收束后 Planner 调用 `task_record_merge`，提交 executor、completion revision、previous/resulting
HEAD、typed method 与 summary。该工具只在 branch mutation lock 内重读并验证 caller、Approved
completion、已关闭 executor、BranchLease/Git identity、当前 HEAD、previous 是 resulting 的祖先、
clean workspace 且没有未结束的 Git operation；它不运行或补偿 Git，也不判断 patch 等价形状。
成功事务写 MergeRecord、推进 WorkUnit/TaskRun/BranchLease，并授权幂等清理源 worktree。
Git 已变化但记账失败时保留现场并 scoped block，不 reset。

delivery reviewer 的 prompt 必须直接包含完整实施蓝图、验收条件和 executor 的全部验证结果，
按验收 ID 逐项核对，并继续满足完整 changed-files 覆盖门禁。reviewer 与 executor 消费同一份
持久化契约，不从 planner transcript 重述或猜测目标。

所有 WorkUnit 均有 MergeRecord 或 NoDelivery 后，TaskService 计算统一、
transport-neutral 的综合审查门禁：`Required`、`SatisfiedByReview { reviewRoundId, reviewedHead }`、
`NotRequiredNoDelivery` 或
`NotRequiredSingleExecutorEquivalent { workUnitId, completionRevision, mergeRecordId }`。同一结果用于
`task_status`、Studio 产品任务状态、模型工具、桌面桥接、HTTP `/state` 和最终完成事务；适配器不得
重复推断。

“始终只有一个 executor”要求整个 TaskRun 只有一个 WorkUnit 和一个 executor identity；允许该
executor 多次返修并产生多个不可变 completion revision，但最终只能有一个获准 delivery、对应的
pass delivery review 和一个 MergeRecord。只要曾创建第二个 WorkUnit/executor，或者已存在任何
integrated review round，就不能事后走免审。单 executor 复用 delivery review 还必须在 branch
mutation lock 内证明：获准 completion base 等于 merge 前 Task HEAD；MergeRecord delivery head
等于被 delivery review 通过的 head；delivery head、merge resulting head 与当前 Task HEAD 的完整
Git tree object 完全相同；
主 workspace clean、没有未结束 Git operation；所有 Task agent 已 terminal。该证明只比较内容，
因此 merge、cherry-pick、squash、rebase 或 manual 都可免审；任一提交不可读、tree 不同、冲突解决
改变实现、planner 额外修改实现或其他证明失败都保守返回 `Required`。

`NotRequiredNoDelivery` 与 `NotRequiredSingleExecutorEquivalent` 可在 Implementing/Reworking 阶段
直接调用 `task_complete`。`Required` 必须创建 fresh integrated reviewer，其 canonical
workspace 是 TaskRun 主 workspace；findings 进入 reworking，pass 后门禁为 `SatisfiedByReview`。
相同不可变 Task HEAD 仍受 pending review 与 provider call 幂等键约束，不重复创建 round。

## 16.7 设计阶段门禁

用户确认实施后，TaskRun 必经 `DesignUpdating`。planner 可继续探索，也可使用普通文件和命令工具
修改任意仓库文件；不再存在专用 `task_update_design`，也不强制修改 `design/**`。

planner 完成该阶段时调用 `task_finalize_design { summary }`。工具在 branch mutation lock 内重读
TaskRun、BranchLease、Git identity、HEAD 与未完成 Git operation。workspace 无变化时不创建提交，
只记录 `designFinalizedHead`、摘要并推进到 `Implementing`；有变化时精确暂存当前 Task 变化，创建
`chore(task): 完成设计阶段` 提交，以旧 HEAD 为 CAS 原子推进 TaskRun、BranchLease expectedHead，
并记录 `designPhaseCommit`。事务失败时只在 exact repository scope 仍成立时撤销提交并恢复为未提交
草稿；不安全时保留现场并 block。

进入 `DesignUpdating` 时状态 payload 保存完整 Git 基线。core 在每个模型工具调用写入唯一终态
Item 后调用宿主提供的统一 completion callback；Studio 对仍处于同一 Task 设计状态的 root planner
重新计算完整 Git 状态和内容指纹，并以递增 observation sequence、Turn ID、tool call ID 持久化为
最近观察。该观察覆盖成功、失败、拒绝和取消的普通工具，不识别 `apply_patch`、`write_file`、
`exec` 或工具来源；`task_finalize_design` 自身不更新观察，避免失败 finalize 把未确认外部变化变成
Task 所有。

finalize 前现场指纹必须精确等于最近一次 durable observation。HEAD 漂移、未完成 Git operation、
外部修改或 observation CAS 漂移都保留文件并返回具体冲突，不提交、不回滚。存在确认变化时只提交
基线到最近观察之间覆盖的路径；提交后 SQLite CAS 失败，只在 HEAD 仍精确等于新提交且没有后续
merge 时把提交安全恢复为未提交草稿，否则保留现场并进入 typed Blocked recovery。

`task_spawn_executor` 在所有活动 Task planner Turn 中保持可见，但只允许 `Implementing` 或
`Reworking`。其他 phase 调用时返回 recoverable `task_phase_mismatch`，包含当前 phase、允许 phase
以及下一步工具；在 `DesignUpdating` 中明确要求先调用 `task_finalize_design`。阶段错误发生在任何
WorkUnit、worktree 或 child Thread 分配之前。

## 16.8 Lease、停止与恢复

同一 Git common directory 与分支只有一个 BranchLease。所有设计、merge、冲突、完成和取消
共享 branch mutation lock；持锁后必须重新读取数据库和 Git 现场，不能依赖旧预检。

stop 先写 typed StopRequested 并禁止新 allocation，再 interrupt 活动 Turn。存在未报告 commit
或 dirty worktree 时返回 deferred，保留成果供 planner 处理；只有全部 completion contract 已
收束才进入 stopping、清理 Pure-owned worktree，并在事务中写 cancelled 与
删除 lease。

启动恢复把遗留 inProgress Turn/Item 标记为 interrupted(runtimeRestarted)，把 pending reviewer
round 收束为失败，并按精确 completion/HEAD 恢复 WorkUnit。Merging 重启时先验证 canonical
workspace、Git common directory 和 branch；HEAD 仍等于 expectedHead、workspace clean 且没有未结束
Git operation 时保持 Merging 并进入 paused，等待用户继续 Planner。若 HEAD 已变化、workspace dirty
或仍有 merge/rebase/cherry-pick 等状态，则保留现场、将精确 Task scoped block，并提供非破坏性的
Retry/Reconcile，不自动 reset、abort 或 cleanup。Retry 重新验证 canonical Git identity，原子重建
BranchLease 和进程 lease、清除该恢复终态后回到 Merging；失败时继续保留 issue 和磁盘现场。
Task phase、delivery、review、merge record、worktree 和 lease 均从产品表恢复；没有 pending input 时
保持 paused。

任何 run、Thread、WorkUnit、review、merge、lease 或 Git 身份配对失败都只 block 精确作用域，
不击穿其他 Project。恢复不重建物理模型连接、不启动 continuation、不删除外部资源。

### 16.8.1 可续跑 Task 与对话恢复

Studio 提供 `previewTaskRecovery(rootThreadId)` 与 `applyTaskRecovery(request)` 两步产品接口。
Preview 无服务端临时状态，其 CAS token 固定 runId、TaskRun revision、typed state、expectedHead、
StopRequested、目标 Thread/runtime revision、候选 Turn/input、continuation revision、BranchLease
与 Git/worktree fingerprint。Apply 持 branch mutation lock 重读全部事实；任何 identity、revision、
Completion、Review、Merge 或 Git 指纹漂移都返回 stale，要求重新 Preview 或 Reconcile。

系统依次建议最近 failed/interrupted 且仍可 follow-up 的 executor、最近 failed/interrupted planner、
最近更新的 eligible executor/planner；reviewer 不进入通用对话回退。默认选择从最近失败 Turn 到
有效尾部，用户只能选择连续末尾一至八个完整 Turn。精确 transcript 匹配失败时 Preview 显式提供
`rebuildThread`，不得自动降级。

对话恢复不回退 TaskRun、WorkUnit、attempt、budget slice、continuation、Completion、Review 或
Merge。executor 仍由 planner 通过既有 follow-up 恢复；executor 新 `TurnStarted` 后，WorkUnit 与
Thread execution 在同一事务恢复为 `Running/Running`。root resume input 使用稳定 mail ID
`task-recovery:{runId}:{recoveryRevision}`，重复 recoveryId 和 mail materialization 必须幂等。

恢复是可重试 saga：先提交 Thread transcript/working state/recovery marker，再在满足门禁时清除
StopRequested，最后投递 resume mail。Stop 只能在 paused 且 state 为 `DesignUpdating`、
`Implementing` 或 `Reworking` 时撤销；`Merging`、`Reviewing`、`Stopping` 和终态继续使用现有
Retry/Reconcile。任一步失败都保留已提交事实，使用同一 recoveryId
重试时从 durable 状态继续，不能重复增加恢复 revision。

Git fingerprint 包含 canonical worktree、Git common directory、branch、HEAD/base/expectedHead、
未结束 Git operation、index diff、working-tree binary diff 与 untracked 内容 hash。dirty worktree
允许恢复，但 Apply 时必须与 Preview 完全一致；恢复不得执行 reset、clean、abort、cherry-pick、
checkout 或删除。失去 durable owner、路径缺失或 Git identity 无法 reconcile 时才允许全 Task 重跑。

## 16.9 清理安全

worktree 路径和分支包含 run 与 executor Thread 身份：

```text
.pure/worktrees/<taskRunId>/<threadId>
pure-task-<runId>-<threadId>
```

恢复清理和项目关闭遵循 preview → confirmation → execution-time revalidation。预览列出 path、
branch、存在状态、dirty、ahead、changed files 和 expected revision。执行只处理数据库证明归属
且仍与预览一致的 `pure-task-*` 分支与 `.pure/worktrees/**`；绝不删除用户主工作区。

项目清理成功后必须推进并发布 project/thread directory revision，再发布最新 recovery state；GUI
只根据这些 canonical snapshot/event 移除项目、Thread 和失效选择，不维护本地删除镜像。由恢复问题
触发的 `RemoveProject` 与直接项目清理共享同一 lifecycle 临界区；已经持锁的恢复路径不得重入该锁。

不兼容 Studio 数据库直接删除并重建，不迁移或归档 ownership。数据库重建本身不扫描或删除磁盘
worktree/branch；失去 durable owner 的资源继续保留现场。

## 16.10 完成条件

`task_complete` 要求：

- 设计阶段已经 finalize；
- 全部 WorkUnit 为 Merged 或 NoDelivery；
- 综合审查门禁为 `SatisfiedByReview`、`NotRequiredNoDelivery` 或
  `NotRequiredSingleExecutorEquivalent`；`Required` 以 `reviewRequired` 拒绝并说明无法复用 delivery
  review 的稳定原因；
- 当前分支、workspace、TaskRun 和 BranchLease expectedHead 精确一致；
- 不存在 StopRequested；
- Task root Thread 自身不存在 pending Interaction。已结算子 Thread 的残留 Interaction
  不阻塞完成——planner 没有取消它们的工具，树级门禁会把任务死锁到用户介入。

pending Interaction 是可恢复的用户或工具边界，`task_complete` 不自动取消；它以
`pendingInteraction` 拒绝完成，并返回总数及稳定 Interaction 预览。用户或 Agent 解决这些
Interaction 后可在同一 Reviewing phase 重试。完成事务必须在写入 terminal fact 与删除 lease 前
原子重验 root 自身的该不变量，不能只依赖事务外 preflight，否则并发创建的 Interaction 可能穿透
终态门禁。

planner 当前 todo list 存在未完成条目时，`task_complete` 以 `todoIncomplete` 拒绝，message
列出每条未完成条目的状态与步骤，planner 需先用 `update_todo_list` 把条目标记完成（或按事实
修订清单）后重试。没有 todo list 或全部条目 completed 时不拦截，不强制 planner 必须使用 todo。

工具返回 tagged `TaskCompleteOutcome`：`completed { run }` 或
`rejected { code, recoverable, message }`。所有门禁拒绝使用稳定 code（wrongPhase、stopRequested、
repositoryDrift、reviewRequired、deliveriesIncomplete、pendingInteraction、todoIncomplete）和用户可读说明。rejected 通过普通 tool
failure JSON 同时进入 Planner 上下文、SQLite Item 与 GUI；Task 保持调用前的非终态，lease/review
不变，且 Planner Turn 只有成功完成时才结束。

完成事务接收上述强类型门禁依据，并在同一 SQLite immediate 事务重新校验 WorkUnit 数量与
executor identity、completion revision、delivery review、MergeRecord、设计阶段完成事实、BranchLease
和 pending Interaction。Git tree、路径 diff、workspace 与未结束 operation 的证明在同一 branch
mutation lock 内紧邻事务完成；任何 durable 事实漂移使事务拒绝。任务状态同时发布门禁和原因，
WorkUnit 概览仅发布蓝图指纹、目标及步骤/验收/验证数量；完整 handoff 只由
`read_work_unit_handoff` 按需读取，避免挤占默认状态上下文。

`task_complete` 只提交通用 Task 生命周期事实，不选择或执行任何项目命令。项目验证由 executor
按照 durable handoff 中的 typed command 契约完成，并通过 WorkCompletion 保存验证摘要；reviewer
负责审查这些证据和实现结果。Task harness 不根据 changed files、目录名或语言推断额外验证。

完成事务写 completed 并删除 BranchLease。任何迟到 child completion、旧 generation 或旧 Turn
通知都不能改变已提交的 Task 终态。

Flutter Driver 验收的 stall 判据只观察 durable Task/WorkUnit 进度：phase、generation、expected
HEAD、WorkUnit/continuation/budget slice、executor Thread 已提交的 `runtimeRevision`、Completion、
Merge 与 Review revision。Task 产品投影把 executor 的 durable revision 发布为
`executorProgressRevision`，因此 root Timeline 不变时，child checkpoint 和 tool result 仍可推进验收
进度；该字段不写入 WorkUnit 表，也不改变 continuation revision 的幂等语义。Thread 的
`thinking/runningTool/responding` 属于瞬态活动，不得刷新 stall 计时；否则重复探索会伪装成任务
进展。总超时继续约束长任务，单次长编译是否允许超过 stall 窗口由 fixture 显式配置。
Driver harness 只驱动与观测这些通用生命周期字段，不解析用户 prompt、项目目录、构建命令或
review finding 的项目语义。计划、验证命令或项目判断写错属于普通 planner/executor 行为，由同一
WorkUnit 的状态机与 follow-up 处理，不得在 harness、工具 schema、skill 或系统提示词中硬编码
项目知识来规避。

真实验收使用 `new | observe | resume` 三种模式。New 要求空 DriverHome、写版本化 manifest 且只
提交一次原始 prompt；Observe 只读取已有 run；Resume 复用 DriverHome、Studio DB、配置、manifest
和全局 deadline，重新取得 VM URL，但不再次提交 prompt 或确认计划。每次 attempt 使用独立日志
目录；只有成功的 Task recovery 才重置 stall 窗口。一次验收最多应用三次 conversation recovery，
第四次直接判定恢复循环；stale preview 的重新生成不计数。

预算恢复验收只允许 debug scripted fixture 注入短 wall-clock 与短 compaction timeout；生产默认
仍固定为 30 分钟和 120 秒。fixture 必须先观察 budget `NeedsAttention`，让当前 Planner Turn 结束
并由稳定 wake 开启 fresh Turn，再向原 executor 执行 `send_message`。Driver 必须记录恢复后的
`budgetSliceCount == 1`，并证明 WorkUnit、agent、worktree 与 branch identity 均未变化；同一隔离
Studio 数据目录重启后还必须验证 wake、恢复消息和 continuation 没有重复物化。
