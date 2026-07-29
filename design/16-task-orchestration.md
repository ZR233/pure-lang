# 16. Simple / Task 模式与任务编排

## 模式语义

Studio 会话模式固定为 `simple | task`。新会话默认 `simple`；数据库中旧
`auto | plan` 会话保留但不进入列表、直接读取或恢复流程。

- `simple`：根 turn 使用 executor 角色，直接对话和实施；只能创建只读 explorer。
- `task`：根 turn 始终使用 planner 角色。planner 是唯一协调者，负责澄清意图、
  提交计划、更新设计、发起代理、消费结果、掌管当前分支、解决 merge 冲突、启动
  reviewer 和完成汇报。

explorer、executor、reviewer 的 agent depth 固定为 1，不得派生后代。Task 根的通用
`spawn_agent` 只允许创建 explorer；executor 必须由 planner 调用
`task_spawn_executor { taskName, message, ownedPaths }` 创建，reviewer 必须由 planner
调用 `task_request_review` 间接创建。所有终态结果必须回流 planner。

## 执行边界

核心层使用 `TurnExecutionProfile` 与工具 effect 强制角色边界。effect 分为
`Read`、`WorkspaceWrite`、`Process`、`AgentControl`、`BranchControl` 和
`ConflictWrite`；未知 effect 对 planner、explorer、reviewer 默认拒绝。

- planner 平时只允许读取、交互、agent control、任务 harness 和受限的
  `task_update_design`。
- executor 只写自己的 worktree，并用 `submit_delivery` 显式提交交付。
- reviewer 只读 plan、diff、代码和按需定位的 design 文档，通过 `review_exit`
  返回结构化审查结果。
- planner 在 `resolvingConflict` 阶段临时获得 `ConflictWrite`，且只能修改当前
  `MergeRecord` 列出的冲突文件。

## 持久化 coordinator

任务事实通过 SQLite 持久化为 `TaskRun`、`WorkUnit`、`AgentOutcome`、
`MergeRecord`、`ReviewRound` 和 `BranchLease`。生命周期为：

```text
planning -> pendingConfirmation -> designUpdating -> implementing -> merging
         -> resolvingConflict -> reviewing -> reworking
         -> stopping -> cancelled
         -> completed | blocked | failed | cancelled
```

coordinator 在 durable 事实提交后向 `pl-core::AgentEventHub` 发布稳定 product signal。
Planner 活跃时更新只被合并，不抢占当前 turn；当前 turn 收尾后由 core 排入一个
ephemeral wake continuation。没有更新但仍有 direct child 时，Planner 进入
`WaitingAgents`，由订阅消息或每个 child 独立的 30 秒 inactivity timer 唤醒，不暴露
模型层等待工具。应用重启后从持久事实、agent snapshots 与 pending input FIFO 恢复；
Git 状态与 `expectedHead` 不一致时进入 `blocked`。

等待注册、status watch 和 deadline 只由 Core `WaitingAgents` 拥有。commentary、tool
activity 与普通 progress 只更新有界进展摘要并重置 child deadline，不能直接唤醒 Planner；
`NeedsAttention`、真实终态和 ProductGated durable phase 才是 actionable wake。每个 child
连续 30 秒无活动只产生一次 `InactivityDiagnostic`，其中必须包含当前 canonical status、
最后活动时间和最近进展，但该诊断不能单独授权 interrupt、`task_stop` 或把 Running executor
判定为失败。只有新活动或 Planner 明确重新进入等待后才能重新计时，不形成周期查询。
Core 必须把 typed wake context 编译为本轮确定性的 execution policy：对仍为
`Queued | Running | WaitingTool | WaitingInteraction` 且拥有 active turn 或可启动 pending
turn 的超时 child，协作工具 schema 与实际 dispatch 都不得允许 `send_input` 或
`close_agent`。纯诊断轮只保留读取 canonical 状态所需的 `list_agents` 和只读能力，不得派生
agent、停止任务或执行其他任务控制动作；混合批次仍可处理真实 actionable fact，但不得借此
控制受保护的健康 child。诊断轮结束且仍有 live child 时重新进入 `WaitingAgents`，从进入时刻
重新获得完整 30 秒 deadline。诊断期间到达的终态、`NeedsAttention` 或 durable product phase
继续进入下一 continuation；显式用户输入会取代诊断轮并恢复普通策略，用户明确请求的
`interrupt=true` 仍按正常取消语义执行。

completion watcher 只观察终态，并把结果作为 `trigger=false` notification 写入 durable
mailbox；父级确有等待注册时，再由 `WaitingAgents` 通过 accepted wake receipt 原子创建
`trigger=true` continuation。channel lag 必须从 durable agent/task snapshot 对账，不能因
缺失中间事件推导终态。Planner continuation 固定消费 typed `WakeContext`：当前 agent 状态、
wake reason、最后活动时间、最近最多 8 个重要阶段事件、一份最新 commentary 摘要、终态
事实、是否存在用户停止请求、signal revision 与 lag reconciliation 结果。

Studio 为每个 Task session 持有一个私有 `AgentRuntime<StudioHost>`（actor coordinator、repository identity、
task generation 与 lifecycle epoch）。同一 session 的用户 root turn 与 continuation
复用该 runtime，因此后续 planner 能继续 list、send 和 close 先前 turn 创建的
agent；不同 session 完全隔离。Simple mode 仍使用 turn-local runtime。planning
generation 在该 session 首次创建 `TaskRun` 时绑定 run id；run 终态且旧 turn 已静止后，
下一个 root turn 才安全轮换 generation，避免旧 agent path 泄漏到新任务。
同一 session 的 runtime 获取与 generation 轮换必须在稳定的 per-session cell 内单航班
执行；停止旧 generation 成功后原子替换该 cell，失败则保留原 entry。全局 registry 锁
不得跨越停止 runtime 的异步等待，不同 session 应能并行；shutdown 与获取/轮换通过
registry 生命周期门禁互斥，避免清空期间漏掉或覆盖 runtime。

已分配 worktree 的 agent 在后续 `submit(InputDelivery::Start)` turn 中
必须继续使用 agent entry 持有的 worktree path；父 planner 当前 workspace 只能提供模型与
turn 配置，不能覆盖 child 的工作区。worktree 句柄缺失或与 durable assignment 不一致时
拒绝启动 follow-up，不得回退到主工作区执行。

Task lifecycle adapter 在安装时绑定 Studio session，并通过该 per-session runtime 边界选择
持久 TaskRun；通用 `AgentSpawnLifecycleRequest.sessionId` 保持工具执行 turn scope 语义，
不得被误作 Studio session 身份或与 hook 绑定值比较。

root turn 结束或 UI 仅切换所选 session 不销毁 Task agent runtime。进程 shutdown 先停止
root turn，再复制 runtime 列表、释放 registry 锁，并逐个
cancel-and-wait/quiesce；该路径保留 durable worktree，不调用会 discard 且吞错的通用
`shutdown_descendants`。旧 epoch 的 agent 事件不得跨越 runtime restart 产生 UI 或
continuation 副作用，也不得写入新 epoch 的 durable agent outcome 或终结观察事实。

真实进程重启不能恢复内存 task handle、物理模型连接或 actor mailbox。取得 process lease
后，store 以 run-scoped 单事务把 `Pending | Running | WaitingForDelivery` WorkUnit 及其
精确配对 Outcome 原子收束为 `Cancelled`；explorer 的 `Queued | Running` Outcome 同样
收束。`Delivered | Merged | Failed | Cancelled` 及 delivery 保持不变，所有旧 terminal
事实标记为已观察，保留 worktree/path/branch 供审计。run、workUnitId、agentId、attempt 或
状态配对错位时事务整体回滚并 block 精确 run，禁止伪恢复 agent 或产生第二次 continuation。

启动恢复顺序固定为 process lease、agent 事务收束、durable-aware worktree 对账、主仓库
校验，最后才允许把 recovery durable signal 发布到 AgentEventHub。对账未完成或出现部分
缺失资源的 run 不得进入 continuation。runtime attach 会从 active Task 的 outcome、delivery、
review round、merge evidence 和 recovery snapshot 重放稳定 product signal；accepted wake
receipt 负责在 publish 前崩溃、乱序重放和重复 attach 下维持单次续轮。

`recover_active_tasks` 返回
`TaskRecoveryReport { recoveredRuns, issues }`，不再用任一局部错误击穿 Runtime 初始化。
Git/worktree/agent/merge/active-run 配对等可归属失败转为 typed `StudioRecoveryIssue`，
scope 为 project 或 session；无明确 owner 的恢复/孤儿失败转为 application degradation。
对应 run 不清理、不续轮，其他通过完整 ownership 识别和 group preflight 的 run 继续恢复。
仅 SQLite、schema、Bridge 或完整 ownership snapshot 无法读取属于应用致命错误。曾经
`affected.isEmpty()` 时直接返回初始化错误的 terminal-only/blocked group，也必须保留为
精确 issue，而不是让 Flutter bootstrap 失败。

Runtime Ready snapshot 携带稳定 issue id、application/project/session scope、project/session/
task ids、typed category、detail 和 available action。bootstrap 选择必须跳过带阻断 issue 的
项目/会话；Bridge selector 与 Runtime controller 同样拒绝直接选择故障目标。局部 issue
清理成功后以 canonical snapshot 原子移除 issue 并重新选择健康目标，失败则保留 issue 并
更新诊断。

同一 Git common directory 与分支只允许一个写入任务。`BranchLease` 是进程内所有权，
`expectedHead` CAS 和工作区清洁检查负责检测用户或外部进程的变化。
用户确认实施时，任务启动边界先准备项目 Git 基线：有效仓库继续要求 named branch、
有效 HEAD 和 clean working tree；完全不属于 Git 仓库的项目在项目根初始化 `main`，
已初始化但尚无 HEAD 的仓库保留当前 named branch。两种无 HEAD 情况都按现有
`.gitignore` 暂存全部项目文件，并创建 `chore: initialize Pure Studio workspace`
首提交；空项目允许空提交。提交优先使用用户已有 Git identity，缺失时仅对该次提交
临时使用 `Pure Studio <pure-studio@local>`，不得写入 local 或 global 配置。初始化和
首提交是独立、持久的项目准备操作；其后 TaskRun、lease 或 continuation 启动失败不得
回滚 `.git` 或改写该提交，重试必须幂等复用已经建立的 clean HEAD。只有任务启动入口
允许执行该准备流程，恢复、交付、设计、合并和审查阶段的 repository 检查始终只读。
已有仓库的 dirty、detached、merge/rebase 现场或损坏状态不得触发自动初始化。
Task 模式从规划 turn 开始即保持 coordinator 的工作区写入独占：skills 可以只读发现、读取和
激活，但 `skill_view` 不得更新项目使用统计，主 turn 完成后也不得启动 skills 自学习
reviewer。这样用户确认实施时的 clean working tree 检查只反映用户或外部进程修改，不会被
规划阶段的后台副作用污染。
仓库准备阶段还必须幂等确保 Git 私有 `info/exclude` 包含 `.pure/worktrees/` 和
`target/pure/`：前者避免 coordinator 创建的内部 worktree 污染主工作区 clean 门禁，
后者避免 `exec` / `write_stdin` 的完整命令输出污染 executor 的 clean delivery 门禁；
不得为此修改或提交用户的 `.gitignore`。该规则同时适用于已有仓库与自动初始化仓库。
计划确认 resolution 只有在任务启动边界完整成功后才可从 pending projection 移除。若 clean
working tree 或其他启动预检失败，Flutter 必须捕获并展示错误，保持同一确认交互可见且可
重试，不得让 bridge 异常成为未捕获的 UI 异步异常。
任务进入 `blocked` 时必须在同一 SQLite 事务中更新 `TaskRun` 并删除 durable
`BranchLease`，随后释放进程 lease；诊断事实保留，但不得永久阻塞同一分支的新任务。

所有会改变任务分支的操作（设计提交、交付合并、冲突继续、完成和取消）共享同一
branch mutation lock；该锁不与 scheduler 或 supervisor 锁嵌套。持锁后必须重新读取
精确的 `TaskRun` 与 `BranchLease`，并重新验证进程 lease、named 当前分支、Git common
directory、干净工作区及两条持久事实的 `expectedHead` 都与当前 HEAD 一致。

通用 agent supervisor 通过生命周期 hook 与精确 worktree spec 接入 Task 编排，
不依赖 Studio store 或 coordinator 类型。Task coordinator 在 agent id 分配后先以事务
创建 Pending `WorkUnit` 与 `AgentOutcome`，并返回固定的 repository、path、branch
和 base commit；supervisor 按该 spec 创建 worktree，coordinator 再以事务把两条记录
激活为 Running，之后才允许子 turn 启动。只有 `implementing | reworking` 阶段允许
分配 executor。并行 allocation 在进程内串行化检查 attempt、并发数和 ownedPaths 后再
事务写入；prepare 事务未提交时不留记录，随后 worktree 创建、持久化激活或 turn
启动失败时删除 agent registry entry、worktree 与分支，并将 WorkUnit/Outcome
事务性标记为 Failed，由 Outcome 保存错误供 planner 与重启恢复审计。
同一工作单元的重试身份由排序后的规范 `ownedPaths` 集合确定，不依赖 planner 可改写的
标题；同一任务内该身份最多分配三次。

executor 分配使用专用、严格类型化的模型工具：

```text
task_spawn_executor {
  taskName: string,
  message: string,
  ownedPaths: string[]
}
```

三个字段全部必填，`ownedPaths` 至少一项。工具在调用通用 `AgentRuntime::spawn` 前用
唯一的 `OwnedPath` 解析模型完成规范化、非法路径和重叠检查；静态校验失败不得创建
Outcome、WorkUnit、worktree、Studio session 或 agent registry 条目。Studio 将通过校验的
输入转换为内部 `StudioSpawnIntent`，lifecycle 只解析一次该类型；模型不能再用通用
`spawn_agent.metadata` 构造 executor 分配。lifecycle 仍保留 harness kind、角色、
session、路径范围和 call id 的最终不变量校验。

executor 固定使用 fresh session，不暴露父历史继承参数。planner 必须在 `message` 中提供
完整任务说明、设计提交和验收要求；Studio 另行生成有界 developer constraint，列出规范
`ownedPaths`，要求只修改所属 worktree、提交并调用 `submit_delivery`，并禁止派生、合并
或操作用户分支。成功结果只返回 `{ agentId, sessionId, turnId, ownedPaths }`，并立即结束
当前 planner turn。planner 不在派生前快照中轮询、追派 executor 或读取子 worktree；
delivery、失败合同或 inactivity timeout 会产生 typed wake batch；Studio Planner wake
renderer 在 turn 准备时重新读取最新 durable Task snapshot，synthetic 输入不进入用户历史。

## Executor 交付

executor 必须先自行 commit，并调用：

```text
submit_delivery { headCommit, verificationSummary }
```

每个 task executor outcome 持久化 `CompletionContract::DeliveryRequired`，包含 task run、
work unit 与一次 recovery 上限。合法 `submit_delivery` 在同一事务中写入 Completed outcome
和 Delivered work unit，提交成功后以 outcome id 发布 `ProductPhaseChanged`；它不得依赖
稍后到达的 terminal projector 间接唤醒，也不得清除已经持久化的 terminal observation。
executor turn
进入 terminal 时 coordinator 仍必须检查合同，不能只按 actor Idle 视为任务完成：
worktree 无改动且 HEAD 未推进时以 `noDelivery` 失败；HEAD 已推进或仍有未提交修改时进入
`WaitingForDelivery`，保留 session、worktree 和 lease，并向同一 executor 投递一次受控
recovery，要求实际验证、提交并调用 `submit_delivery`。recovery 后仍未交付则明确失败并保留
worktree 诊断，不自动合并、不伪造 verification summary。Task executor 始终注册持久
completion watcher，并使用 `AgentWakePolicy::ProductGated`：底层 `TurnFinished` 只更新
runtime 状态，不能越过 delivery contract 唤醒 Planner；合同终结后才发布以 durable record
id 为幂等键的产品信号。单个 turn 因 `InterruptThenStart` 结束时，如果 agent 快照仍有
active turn 或 pending input，或者 activity 尚未回到 `Idle`，它只是同一 executor 的中间
turn，不得结算 completion contract、关闭 Outcome/WorkUnit 的 delivery scope 或发布产品
信号。异步 terminal projector 还必须在写入产品终态前重新读取 latest agent snapshot，并
确认 latest last turn 与待投影 turn 一致；只有 queue-drained canonical terminal 才进入上述
交付检查与恢复。

core 在 pending-input FIFO 之外持久化 accepted wake receipt，记录 typed wake id 与组成批次
的 durable signal ids；因此同一产品事实即使在 wake 已出队后重启重放、或被合并进不同批次，
也不会产生第二个续轮。continuation 去重同时识别 queued turn 和正在执行的 ephemeral 受管
续轮，普通 root turn 不参与 running 去重，以免吞掉 child terminal 的后续输入。recovery
claim 与 runtime FIFO 之间使用由
outcome id 和 recovery count 生成的稳定 dispatch id；claim 提交后若进程退出，重启必须复用
同一 claim。runtime durable turn metadata 已存在该 dispatch id 时不得重复提交；该 turn
已终态但产品投影尚未完成时，恢复流程先重放 terminal 投影再发布产品信号。这样 claim
提交、runtime submit 和产品 terminal 投影三个边界都可恢复，同时一次 recovery 上限不会被
重复消费。

runtime 只接受 HEAD 已推进且 worktree 干净的交付，返回 `baseCommit`、`headCommit`、
`changedFiles`、验证摘要和 `{ path, branch }`。runtime 不隐式执行 `git add -A`。
work unit 声明 `ownedPaths`；并行 executor 的写入范围不得重叠，超出范围的交付必须
返回 planner 决策。work unit 创建时固定记录 `baseCommit`、预期 worktree path 和
branch；交付校验不得改用随后可能因其他 executor 合并而推进的 task `expectedHead`，
且 caller workspace 和 branch 必须与固定记录精确匹配。rename/copy 同时校验 source
与 destination，delete 校验被删除的原路径。单个 work unit 最多尝试三次，同时运行
的 executor 最多四个。

`ownedPaths` 只接受相对规范路径或唯一的目录后缀 `/**`；裸尾随 `/`、`\` 与其他
通配符均拒绝。持久化和展示保留规范原文大小写，比较键遵循平台文件系统语义：
Windows 转为小写后进行 overlap 与交付匹配，Unix 保持大小写敏感。allocation 与
delivery 必须复用同一个解析模型，避免两条路径产生不同边界判断。

Task executor 的通用 `close_agent merge=true` 必须在进入 worktree merge 路径前拒绝；
关闭或取消只允许丢弃 worktree。Task worktree 不得调用通用的隐式 `commit_all`。
对 Pending、Running 或 WaitingForDelivery executor 执行 discard 时，Task lifecycle hook
必须使用精确 lifecycle token，在一个事务中把配对 WorkUnit 与 AgentOutcome 幂等收束为
Cancelled，并标记终态已观察；只有 durable 处置成功后 supervisor 才能释放 worktree。
worktree 清理失败不回滚 Cancelled 事实，资源信息保留给恢复清理，但该 WorkUnit 不再占用
ownedPaths。重复 discard 不得产生第二次状态迁移或 continuation。
discard 在调用 supervisor 释放 worktree 前必须把对应 WorkUnit 的
`worktreeDisposition` 持久化为 `cleanupRequested`；这项授权独立于 WorkUnit 当前状态，
因此即使 WorkUnit 已经 Failed/Cancelled 也必须幂等写入。未带精确 discard 证据的 legacy
Cancelled 记录保持 `protect`，不能因终态自动删除可能仍含未合并提交的 worktree。

worktree 路径和分支包含 task run id：

```text
.pure/worktrees/<taskRunId>/<agentId>
pure-task-<runId>-<agentId>
```

恢复 issue 与项目关闭的用户确认清理都先返回 typed `RecoveryCleanupPreview`，包含 path、
branch、missing/partial/complete、dirty、ahead commit、changed-file 及
`expectedRevision`。项目级预览必须聚合该项目全部 session 的全部 Task run，不能只展示触发
issue 的单个 run。执行时以 revision 做 CAS，先递归关闭该项目的 root agent tree，再在事务中
终结全部关联 Task、删除 BranchLease、将相关 disposition 写为 `cleanupRequested`；之后才
幂等释放预览中经 durable ownership 验证的 Pure-owned leaf/branch。该确认明确授权放弃这些
Pure worktree 中的未提交修改和未合并提交，但不授权触碰用户主工作区、非 `pure-task-*`
分支或 `.pure/worktrees/<taskRunId>/<agentId>` 之外的路径。session scope 保留聊天历史；
project scope 归档会话、删除损坏 Task/runtime 元数据并移除 Studio 项目登记，但绝不删除或
修改用户项目目录。

项目清理关闭 agent tree 时由 durable cleanup 临时接管内存资源 ownership；普通 agent close
不得抢先释放 worktree 或移除 ownership 映射。关闭或后续清理失败时保留该接管和映射供重试，
仅在 durable worktree 清理及项目 quarantine 都完成后最终 detach。实际删除前必须再次核对
确认时的项目版本、完整 Task run 集合（包括零 work-unit run）、work-unit identity 集合和
worktree HEAD/dirty/存在性事实；agent close 合法产生的 Task 状态变化不作为失效条件。任一
资源事实漂移都终止执行并要求刷新预览。中断后恢复只续做已有 durable cleanup 授权，不扩大
清理范围。

## Planner 合并与冲突

planner 调用 `task_merge_agent { agentId, expectedHeadCommit }`。runtime 校验当前分支
和交付 commit 后执行 `git merge --no-ff --no-commit`。无冲突时先运行相关集成检查，
通过后才创建 merge commit。成功后更新 `expectedHead`、关闭 agent 并释放 worktree；
executor 完成一个即可合并。

`task_merge_agent` 只对 Task 根 planner 开放，并绑定工具安装时的 Studio session；工具
执行上下文中的 turn id 或 workspace 不能替代该身份。开始合并前，coordinator 在一个
事务中固定来源 phase、WorkUnit、AgentOutcome、delivery commit、主分支旧 HEAD、index
tree 与 executor worktree 身份，创建唯一 active `MergeRecord` 并进入 `merging`。随后在
共享 branch mutation lock 内重新验证进程 lease、`BranchLease`、named branch、Git
common directory、clean workspace、caller `expectedHeadCommit`，以及 executor worktree
的 path、branch、HEAD、base ancestry 和 changed-file scope。任何不一致都不得产生 Git
合并副作用。同一 delivery 只允许产生一个 merge；planner 可查询已持久化结果，但不能
重复消费。

coordinator 使用有界、非交互、结构化参数启动 Git，并以实际 changed-file class 选择
相关验证：Rust 变更至少执行 workspace formatting check，Flutter 变更至少执行 Flutter
analyze。executor 的验证摘要只作为审计输入，不能替代 coordinator 验证。无冲突且验证
通过后创建 focused merge commit；commit message 记录 task run、agent、旧 HEAD、来源
commit 和 coordinator 验证结果。`TaskRun.expectedHead`、`BranchLease.expectedHead`、
`MergeRecord`、`WorkUnit` 与恢复到来源 phase 必须在一个事务中以旧 HEAD、精确 delivery
身份和 active merge 为 CAS 原子推进。若验证失败，runtime abort merge 并证明 HEAD、
index 和 worktree 恢复 prestate，然后把 merge 标记为失败、任务标记为 blocked，且不
消费 delivery。

Git merge commit 已创建而 durable CAS 失败时，只有在当前 named branch、HEAD、parent、
commit diff、index 和 worktree 仍精确属于本次 clean merge 时才允许补偿回旧 HEAD；无法
证明安全时保留现场并 block 精确任务。durable 接受后不再回滚 merge。此时先释放 branch
mutation lock，再通过持久 Task supervisor 以 discard 语义关闭 executor；若内存 registry
已丢失，则使用精确 durable worktree owner 做幂等清理。清理失败写入 merge evidence 供
后续重试，不改变 `MergeRecord=Merged`、`WorkUnit=Merged` 或 completed delivery。迟到的
agent terminal 事件也不得把已合并 WorkUnit 降级。

Git 返回冲突时不得 abort。runtime 从 unmerged index 固定 `MERGE_HEAD`、merge base、
pre-merge index tree、每个冲突 path 的 stage 1/2/3 mode 与 object id、rename source /
destination、binary 标记，以及 Git 已自动合并的 index entries。冲突按 text、add/add、
rename/delete、modify/delete、binary 等 typed kind 持久化；路径必须是规范仓库相对路径，
worktree/index 不得包含与该 merge 无关的修改。随后 `MergeRecord=Conflicted`、任务进入
`resolvingConflict`，保留 MERGE_HEAD、index 与 worktree，只 claim 一次 merge record 并以
该 record id 发布 `MergeConflict` product signal；executor worktree 继续受 durable owner
保护。

重启恢复按 merge phase 判断 Git 状态：`Pending | Verifying` 必须依据持久 prestate、
当前 HEAD、MERGE_HEAD 和 index 判断可安全继续、补偿还是 block；`Conflicted` 加
`resolvingConflict` 且现场与 conflict manifest 一致时是合法恢复状态，不得被普通 dirty
workspace 检查误判为外部漂移。`MergeRecord.verification_json` 承载版本化 `MergeEvidence`，
包含来源 phase、prestate、delivery identity、验证、commit、冲突 manifest、补偿与 cleanup
状态；保持现有六张 coordinator 表，不为 transient tool trace 新增协议或数据表。

每次 merge durable 接受后记录一次尚未消费的完成通知。runtime 在事务中 claim 同一任务
尚未通知的成功 merge，并以每个 merge record id 分别发布稳定 `MergeCompleted` product
signal；AgentEventHub 在 Planner 活跃时把多个 record 更新合并为一个 wake batch，重复扫描
不得再次续跑。

冲突时持久化 `MergeRecord`，暂停其他 merge，由 planner 使用以下受限工具亲自解决：

- `merge_list_conflicts`
- `merge_read_conflict`
- `merge_resolve_file`
- `merge_verify`
- `merge_continue`
- `merge_abort`

planner 只能修改冲突清单中的文件。continue 前必须没有 unmerged index、冲突标记或
未解释修改，并通过格式化与相关测试。三次解决仍失败时必须 abort 并进入 blocked。

## 设计与审查门禁

用户确认实施后，planner 先调用 `task_update_design` 修改并提交 `design/**`；成功前
不得创建 executor。任务取消或部分失败时，design 必须回退或更新到与当前实现一致。
`task_update_design` 的 patch 必须是从 `*** Begin Patch` 到 `*** End Patch` 的完整
Codex patch；新增文件的每一行内容都必须带 `+` 前缀。工具说明必须给出完整示例，
执行失败时必须向模型保留解析、路径或 Git 操作的具体根因，不能只返回通用重试提示，
否则模型无法根据失败类型修正下一次调用。
处理“实施”确认时必须先完成 plan、session、repository 与 branch lease 校验并创建
`TaskRun`，再把 confirmation 标为 resolved 和写入 accepted/implementing lifecycle；
创建失败时原 confirmation 保持 pending，不得留下虚假的 implementing 状态。
该工具只对 Task 根 planner 可见，先完整解析并验证 patch 的所有 source 和 move
destination 都是规范、非 ignored、且不会经 symlink 逃逸的 workspace-relative
`design/**` 路径，再进行首次写入。应用与提交是 all-or-nothing：失败时精确恢复所有
已触及的 design 路径和暂存区，不影响其他路径。

focused design commit 成功后，SQLite 在一个事务中以旧 HEAD 为 CAS，同时推进
`TaskRun.expectedHead` 与 `BranchLease.expectedHead`、记录 `designCommit`，并将初始
`designUpdating` 推进到 `implementing`。后续一致性更新只允许在没有进行中 merge 且
可继续实施或返工的 `implementing | reworking`。若事务失败，仅当 HEAD 仍是刚创建的提交且
工作区干净时补偿回旧 HEAD；无法证明安全时将该精确 run 标记为 `blocked` 并保留诊断，
不得覆盖外部变化。durable CAS 成功前 allocation phase gate 始终关闭，工具成功本身
不启动 continuation。

durable CAS 成功后、工具返回成功前必须最后一次复验 clean workspace、workspace root、
Git common dir、named branch 与 exact HEAD。若该窗口出现外部 commit、切分支或 dirty
workspace，必须 block 精确 run 并保留外部现场；已经原子推进的 TaskRun/BranchLease HEAD
继续记录本次 exact commit，不回退为旧 durable HEAD，也不得补偿或覆盖外部 Git 状态。

`git commit`（包括受控 revert 的 focused commit）返回成功即进入 post-commit 边界；此后任何 HEAD、branch、
status 或 diff 检查失败都不得再调用 pre-commit path rollback。coordinator 只接受能证明
“当前 named branch HEAD 是本次提交”的 commit：它必须以旧 `expectedHead` 为唯一父提交，
commit diff 必须精确等于预先验证的 design paths，commit tree 必须等于 hook 执行前锁定的
staged tree，且 index/worktree 没有 hook 或外部注入。
不能完成该证明，或发现外部 clean commit、branch 变化、dirty workspace 时，必须 block
精确 run 并保留现场；只有 exact commit 已证明且 HEAD/工作区仍安全时才能补偿。
pre-commit 阶段失败时，design path rollback 必须在每个写/删动作前重新拒绝 symlink
ancestor 并证明 canonical path 仍位于 workspace 内；rollback 后还必须证明整个 repository
恢复 clean。hook 或外部并发留下 source residue、或安全路径证明失败时，必须 block 精确
run、保留残留现场并报告诊断，不能把它当作普通工具失败。

尚无 accepted source merge 且 base 之后只有本任务 design commit 时，取消操作通过
创建受控 `git revert` commit 撤销设计（不 hard reset、不改写历史），再以同一事务推进
任务与 lease 的 `expectedHead`，之后才可 terminalize。若已经接受 source merge，只有
`designCommit == expectedHead` 才表示 planner 已完成最后一次设计一致性更新；部分实施
失败也必须以最后一次 design consistency commit 收束后才能进入终态。
取消 revert 先把 inverse patch 写入 index/worktree，再通过 focused commit 完成，以便
`pre-commit`、`commit-msg` 等仓库策略真实参与；任一步失败都必须 block 精确 run 并保留
`REVERT_HEAD`、index 和 worktree 现场。成功 commit 使用同一 exact-commit 证明与 post-commit
补偿规则。安全补偿必须再次证明 workspace root、git common dir、named branch、clean 状态
和 exact HEAD 全部仍属于该 run；即使 HEAD 相同，切换到另一分支也禁止 reset/restore。
`task_stop` 先订阅 Task terminal durable fact channel，再按 branch mutation、allocation 的
固定顺序短暂取得两把 guard，完成预检并写入
持久 `StopRequested`，但不立即进入不可逆 `stopping`，delivery scope 继续开放。随后在不持
branch mutation lock 的情况下 interrupt active turn；每次 terminal fact 通知后重查
durable completion predicate，保留 10 秒总上限且不使用短轮询，再统一检查
全部 completion contract、HEAD 与 worktree。存在已推进 HEAD 或 dirty worktree 时停止返回
`deferred`，executor 进入上述一次 delivery recovery；只有不存在可恢复成果且所有合同已
终结时，才进入 `stopping` 并拒绝新的 allocation/delivery，最后重新取得 branch guard，
复验 repository 后完成 revert、durable HEAD 推进和 terminalization。
显式传入的 branch mutation guard 必须绑定创建它的 coordinator；其他 coordinator 的
guard 不得授权 locked mutation API。

停止意图使用服务端确定的
`TaskStopOrigin::{UserRequest, PlannerDecision, RuntimeFailure, ApplicationShutdown}`，并将
“谁发起停止”与独立的 `TaskStopReason` 持久化。模型工具 `task_stop` 只能形成
`PlannerDecision`，用户点击停止只能形成 `UserRequest`，模型输入不得伪造 origin。UI、
timeline 和 continuation prompt 都按 durable origin 渲染，不得把任意 `StopRequested`
固定描述成“用户请求停止”。

Task quiesce 在写入 `StopRequested` 的同一原子边界递增 task generation，阻止新的
executor/reviewer turn，并使旧 wake、defer 与 start receipt 失效；随后取消活动 turn。所有
完成、失败和取消路径汇入单一 durable terminal-fact barrier，同一 generation 只能提交一个
task/session/queue/event 终态。`StopRequested` 后只允许一个带 receipt 的
`DeliveryRecovery` turn，用于 executor 已产生可恢复结果但 durable delivery 尚未完成的
情况；不得重新启动通用 Planner 或 executor。

当前编码轮全部交付合并后，planner 必须先用 `task_update_design` 把 `designCommit`
推进到当前 `expectedHead`，再调用 `task_request_review` 间接创建只读 reviewer。
review harness 在创建 round 和派生 reviewer 前重新校验该不变量，使设计不一致在
仍可更新设计的 `implementing/reworking` 阶段失败，不能进入只读 `reviewing` 后才暴露。
reviewer 成功启动后同样结束当前 planner turn；reviewer 使用
`AgentWakePolicy::ProductGated`，`review_exit` durable transaction 成功后以 review round id
发布 product signal。未满足审查合同时，runtime terminal 本身不得唤醒可执行 Planner 续轮。
reviewer 使用 fresh session，不复制 planner 完整历史；harness 生成的自包含初始 prompt
包含 plan、任务 diff、代理结果、验证摘要和 design 文件索引，不预载 design 正文。系统
提示词要求 reviewer 根据改动主动搜索并读取相关 design，再对照设计审查正确性、回归、
安全和测试缺口。

审查创建以 `task_request_review` 的 provider call id 作为一次性持久授权。harness 消费
该授权后，`ReviewRound`、reviewer `AgentOutcome`、`ownerPath=/root` 和
`requestedByCallId` 必须精确配对；直接派生 reviewer 或重复消费授权均拒绝。审查 diff
固定为任务 base 到当前 `expectedHead` 的非 design 综合 diff，design 只提供排序后的
文件索引，避免模型把预载正文误当作已经主动核验的设计依据。

`review_exit` 返回 `verdict`、`summary`、`designReferences` 和 `findings`。runtime 根据
tool trace 校验 reviewer 先成功定位文档，再以规范的 workspace-relative 路径读取
`design/**` 正文；该校验必须读取完整 reviewer `AgentSession`，不得因 Responses
continuation 只发送增量上下文而丢失先前的 locator 或 read 证据。路径、章节和 finding
引用都必须能在实际读取结果中验证。未调用
`review_exit` 便终结的 reviewer 会把本轮与 outcome 标记为失败，并恢复到可实施阶段，
不得伪造通过或重复触发 continuation。`changesRequired` 返回 planner 派发修复
executor；修复合并后必须创建新的 reviewer。最多三轮审查修复。

所有 provider 工具调用使用同一个稳定调用身份：优先采用非空 provider `call_id`，缺失时
回退到通用 ToolCall `id`。消息历史、tool context、review/merge/conflict authorization 与
agent requestedByCallId 必须消费同一身份，使 Chat Completions 与 Responses provider
具有一致的 harness 关联语义。

只有 design 一致、所有交付已处理、当前分支干净、最新 reviewer 对当前 HEAD 返回
`pass` 且验证通过时，planner 才能调用 `task_complete`。

`task_complete` 在共享 branch mutation lock 内重新校验 TaskRun、BranchLease、named
branch、clean workspace 与 exact HEAD；最新 review 必须针对该 HEAD 返回 `pass`，所有
work unit、outcome 和 merge 都必须已收束。存在已接受 source merge 时，最后一次
`task_update_design` 必须已经把 `designCommit` 推进到当前 HEAD。runtime 按任务综合变更
运行必要的最终检查后，以单事务写入 `completed` 并删除 BranchLease，再释放进程 lease。
一旦 `StopRequested` 已持久化，`task_complete` 在 coordinator 预检和 store 事务两层都必须
拒绝，不能绕过停止合同完成任务或释放 lease。

`task_stop` 先以 typed origin 持久化 `StopRequested` 并递增 task generation，再终止并等待
当前任务的内存代理到 canonical terminal，
随后检查 delivery-required 合同和精确 worktree。可恢复提交或修改使停止返回 `deferred`，
不得关闭 delivery scope、取消 outcome 或释放 worktree；完成最多一次 recovery 后重新评估。
只有无可恢复成果时才在短锁事务中进入 `stopping`，将剩余 durable agent/work unit 收束为
`cancelled`，最后重新进入 branch mutation lock。尚无 source merge 时，如已接受 design commit，
必须先创建受控 revert commit；已有 source merge 时，必须先由 planner 更新 design 到当前
实现。该设计一致性检查必须在写入 `StopRequested` 和进入不可逆 `stopping` 之前完成；
检查失败时 run 保持原 phase 且仍可调用 `task_update_design`。存在尚未安全 abort 的
merge/conflict 时停止操作拒绝终态写入，保留现场供 planner 使用冲突工具处理。取消终态与
BranchLease 删除同样在一个事务中完成。
