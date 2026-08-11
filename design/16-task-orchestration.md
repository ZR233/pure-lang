# 16 - Simple / Task 与 TaskService

## 16.1 模式

root Thread 模式只有 `simple | task`。

- Simple：root 使用 executor role，可直接实现；只允许派生只读 explorer。
- Task：root 使用 planner role。planner 负责计划、设计、executor、review、merge、冲突和完成。

root 的 mode 与 role 是同一不变量，只能在没有活动 Turn、pending input 和活动 Task 时切换。
启动恢复在创建 ThreadActor 前修复旧数据库中 `simple/planner` 或 `task/executor` 的 root 记录；
child role 不参与该修复。Task root 永远不能进入 executor WorkUnit 生命周期。

每个 agent 固定对应一个 Thread。child 通过 `rootThreadId`、`parentThreadId`、`role` 和
`agentPath` 表达关系。TaskRun 只绑定 root Thread；executor 和 reviewer 直接由 WorkUnit、
ReviewRound 引用，不建立 AgentOutcome 镜像。

## 16.2 所有权

`TaskService` 位于 `pl-studio-runtime`，管理：

- TaskRun 与 phase；
- WorkUnit 与不可变 WorkCompletion；
- ReviewRound；
- MergeRecord 与 Planner Git 记账；
- BranchLease、worktree ownership 与安全清理。

Thread/Turn 的执行状态只从 Thread repository 读取。Task 状态从产品表直接组成
`TaskSnapshot`，只进入 product stream。planner 执行的 Task 工具仍是 planner Thread 自己的
toolCall Item。

## 16.3 Task 状态

Task phase 为：

```text
planning → pendingConfirmation → designUpdating → implementing → merging
         → reviewing → reworking
         → stopping → completed | blocked | failed | cancelled
```

WorkUnit 直接保存 executorThreadId、requestedByCallId、attempt、scopeHints、baseCommit、
worktree、branch、状态、summary/error 和 cleanup disposition。ReviewRound 直接保存
reviewerThreadId、scope、目标 completion/HEAD、verdict 和 findings。

WorkUnit 状态为 Pending、Running、AwaitingCompletion、ReadyForReview、Reviewing、
ChangesRequested、Approved、Merged、NoDelivery、NeedsAttention、Failed、Cancelled。
WorkUnit 额外持久化当前 tranche 的 budget slice count、typed continuation state、来源 Turn 与
continuation revision；这些字段是重启恢复和幂等续轮的 canonical owner，不能从 Timeline 文本推断。

每次 durable transition 在一个 SQLite 事务中更新所有相关产品记录。Task phase 不复制进
Thread runtime snapshot，Thread 状态也不缓存进 Task 表。

## 16.4 Planner 与等待

Task root 只允许 planner 创建 explorer；executor 通过 `task_spawn_executor` 创建，reviewer
通过 delivery/integrated review 工具创建。executor/reviewer depth 固定为 1。

`wait_agents` 订阅 Thread directory watch 后读取 snapshot，只因 progress、interaction 或
terminal 变化返回，并以 `messages` 返回本次最新增量；planner 直接消费该结果，不在 wait
之后调用 `list_agents` 重复刷新完整目录。没有轮询、自动续轮或超时中断。五分钟仅允许
planner 读取有界 child Thread 诊断，不是失败判据。

该 wait 输出协议不迁移旧历史；旧会话或 fixture 不兼容时直接重建。

review request 成功创建 reviewer 后必须结束当前 planner Turn。reviewer 提交 durable verdict 后，
Runtime 以稳定 mail ID 提交一次隐藏 continuation；root Thread 已 idle 时立即启动，仍有活动 Turn 时
只排入下一 Turn，绝不 steer 旧 Turn。新的 planner Turn 从最新 Task phase 重新解析 canonical
workspace 与 tool policy。这样 delivery pass 进入 Merging 后才授予主 workspace 写入、普通 exec
与 Git 能力，旧 Turn 的只读 snapshot 不会被阶段变化旁路。

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
Turn 必须再次调用 `plan_exit` 生成新的确认。`ImplementFreshContext` 与 `Dismiss` 保持既有 Task
启动和忽略语义。

Task planner/executor/reviewer 的 required finalization tool 只约束业务阶段完成，不约束 durable
UserInput 边界。原 Turn 因 pending Interaction 结束时必须保存为 completed，不能因为尚未调用
`plan_exit`、`report_completion` 或 review exit 工具而标成 failed；fresh Turn 恢复后仍继续执行原
finalization policy。

进程重启后不为普通 paused Task 自动启动模型；但崩溃前已经 durable 形成的 pending Planner wake
或 mailbox input 必须在资源恢复完成后继续交付。活动 Task 无 pending input 时显示 paused；用户
“继续任务”以稳定 mail ID 向 root Thread 提交一次隐藏的明确输入，要求 planner 先读取
`task_status` 和 `list_agents`。attach 只恢复已有 durable 工作，不为单纯 active Task 合成新工作。

## 16.5 Executor 与交付

`task_spawn_executor` 必须提供 taskName、message；scopeHints 可省略或为空。TaskService 在创建
child Thread 前只校验 hint 是规范仓库相对路径，再校验并发上限、phase 和 branch lease并准备
专属 worktree。hint 重叠只形成提示，不拒绝并发。

每次 executor allocation 同时生成 `TaskExecutorHandoffV1`。handoff 固定 TaskRun/WorkUnit、parent
Thread、requestedByCallId、确认计划、assignment、base/design/expected HEAD、scope、验收条件、
依赖、证据、验证契约与交付契约，并作为 `studio.task_executor_handoff` pinned section 随 fresh child session
持久化。后续 Turn 从 durable WorkUnit 与该 section 交叉校验；缺失、损坏或 HEAD/owner 不一致时
进入 NeedsAttention。相同 TaskRun 的相同 requestedByCallId 是强幂等键；Implementing 阶段中，
规范化 taskName 与 scopeHints 相同且仍为 active 的 allocation 即使来自新的 provider call ID，
也必须返回首个 WorkUnit、executor Thread 与 canonical call ID，不重复分配 worktree、BranchLease
或 Thread。只有原 WorkUnit 已 terminal，或 TaskRun 已明确进入 Reworking，才允许创建新的 attempt。

验证契约是非空的 typed command 列表，每项固定命令、仓库相对 cwd 与验证目的。
planner 在 allocation 时必须提交已核对的项目入口；executor 每轮从 durable handoff
读取，不得从 planner transcript 或短命 mailbox metadata 重建。TaskService 只校验字段、大小与
cwd 的仓库相对路径边界，不根据 assignment、scope 或命令文本推断项目知识；命令语义错误作为
普通 executor 验证失败处理，由同一 WorkUnit 修正。

executor 只能写自己的 worktree，并以以下工具结束可交付工作：

```text
report_completion {
  delivery { headCommit, verificationSummary }
  | noDelivery { verificationSummary }
}
```

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
executor 或 WorkUnit 标记为失败：runtime 先对同一 Thread 强制执行 `WallClockRollover`
compaction，再以 `workUnitId + sourceTurnId` 生成确定性 hidden continuation input，在同一
worktree 开启下一切片。一个 tranche 最多四个切片；第四次 wall-clock 耗尽进入
NeedsAttention 并保留 executor/worktree，等待 Planner 停止、拆分或用 `task_send_message`
显式开启新 tranche。非 wall-clock budget、用户停止、Task 取消和 rollover compaction 失败都不
自动续轮；pending continuation 在重启时按幂等键对账已有 active/terminal Turn，禁止重复增加切片。
rollover replacement transcript 必须先与 TurnFinished 在 repository 提交链上持久化成功，再允许
hidden continuation 入队；提交失败时 actor 不推进内存 session，也不启动下一 Turn。

UserInput 的 fresh-turn 边界不扩大上述预算续轮范围。普通 Planner、reviewer、Simple 或 child
`budgetLimited` 仍是 terminal 事实，不自动合成 continuation；只有这里定义的 executor
`WallClockRollover` 可以按 WorkUnit tranche 状态机续轮。

WorkUnit 在 ReadyForReview 之后以 `executorAgentId` 创建 fresh Delivery reviewer。ReviewRound
事务固定最新 Completion revision，reviewer canonical workspace 直接绑定同一 worktree，不接受
模型提供路径。findings 使 WorkUnit 进入 ChangesRequested；
planner 把具体 finding 发回原 executor Thread，新的 completion revision 重新审查。pass 后
WorkUnit 进入 Approved 或 NoDelivery。executor 在普通结束或失败时若没有形成新的
Completion，WorkUnit 保留可 follow-up 的 durable terminal execution 状态，并生成一次 Planner
wake；review changes-requested 后的 rework failure 也走同一路径，不能静默停在
`AwaitingCompletion/failed`。取消由既有 stop/cancel 收束处理，不额外唤醒 Planner。

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

所有 WorkUnit 均有 MergeRecord 或 NoDelivery、design 已与当前 HEAD 一致后，创建 fresh integrated
reviewer，其 canonical workspace 是 TaskRun 主 workspace。findings 进入 reworking，由新 executor
修复；pass 才允许 `task_complete`。

## 16.7 设计门禁

用户确认实施后，planner 必须先用 `task_update_design` 提交 `design/**` 的 focused patch。
解析、路径、安全检查、应用和提交是 all-or-nothing；失败恢复已触及路径和 index。

成功后以旧 HEAD 为 CAS，在同一事务推进 TaskRun 与 BranchLease expectedHead 并记录
designCommit。存在 source merge 时，完成或取消前 design 必须再次与当前实现一致。

## 16.8 Lease、停止与恢复

同一 Git common directory 与分支只有一个 BranchLease。所有设计、merge、冲突、完成和取消
共享 branch mutation lock；持锁后必须重新读取数据库和 Git 现场，不能依赖旧预检。

stop 先写 typed StopRequested 并禁止新 allocation，再 interrupt 活动 Turn。存在未报告 commit
或 dirty worktree 时返回 deferred，保留成果供 planner 处理；只有全部 completion contract 已
收束才进入 stopping、清理 Pure-owned worktree、处理 design 一致性并在事务中写 cancelled 与
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
Preview 无服务端临时状态，其 CAS token 固定 runId、task generation、phase、expectedHead、
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
StopRequested，最后投递 resume mail。Stop 只能在 paused 且 phase 为 planning、
pendingConfirmation、designUpdating、implementing 或 reworking 时撤销；merging、reviewing、
stopping 和终态继续使用现有 Retry/Reconcile。任一步失败都保留已提交事实，使用同一 recoveryId
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

不兼容 Studio 数据库直接删除并重建，不迁移或归档 ownership。数据库重建本身不扫描或删除磁盘
worktree/branch；失去 durable owner 的资源继续保留现场。

## 16.10 完成条件

`task_complete` 要求：

- design 与当前 HEAD 一致；
- 全部 WorkUnit 为 Merged 或 NoDelivery；
- 最新 integrated review 针对当前 HEAD 且 verdict 为 pass；
- 当前分支、workspace、TaskRun 和 BranchLease expectedHead 精确一致；
- 不存在 StopRequested。

工具返回 tagged `TaskCompleteOutcome`：`completed { run }` 或
`rejected { code, recoverable, message }`。所有门禁拒绝使用稳定 code（wrongPhase、stopRequested、
repositoryDrift、reviewMissing、deliveriesIncomplete）和用户可读说明。rejected 通过普通 tool
failure JSON 同时进入 Planner 上下文、SQLite Item 与 GUI；Task 保持 Reviewing，lease/review
不变，且 Planner Turn 只有成功完成时才结束。

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
