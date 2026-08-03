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
调用 `task_request_delivery_review` 或 `task_request_integrated_review` 间接创建。所有
完成和审查事实都由 planner 通过 Agent Directory 与 `task_status` 主动读取。

## 执行边界

核心层使用 `TurnExecutionProfile` 与工具 effect 强制角色边界。effect 分为
`Read`、`WorkspaceWrite`、`Process`、`AgentControl`、`BranchControl` 和
`ConflictWrite`；未知 effect 对 planner、explorer、reviewer 默认拒绝。

- planner 平时只允许读取、交互、agent control、任务 harness 和受限的
  `task_update_design`。
- executor 只写自己的 worktree，并用 `report_completion` 显式报告可审查结果。
- reviewer 只读 plan、diff、代码和按需定位的 design 文档，通过 `review_exit`
  返回结构化审查结果。
- planner 在 `resolvingConflict` 阶段临时获得 `ConflictWrite`，且只能修改当前
  `MergeRecord` 列出的冲突文件。

## 持久化 coordinator

任务事实通过 SQLite 持久化为 `TaskRun`、`WorkUnit`、`WorkCompletion`、
`AgentOutcome`、`MergeRecord`、`ReviewRound` 和 `BranchLease`。生命周期为：

```text
planning -> pendingConfirmation -> designUpdating -> implementing -> merging
         -> resolvingConflict -> reviewing -> reworking
         -> stopping -> cancelled
         -> completed | blocked | failed | cancelled
```

coordinator 只提交 durable Task 事实和 agent progress/snapshot，不向 Planner mailbox
写入 product signal，不创建 synthetic continuation。每次 durable agent commit 后，
`AgentDirectory` 更新 canonical snapshot 并推进单一 watch revision；普通 tool activity
不推进 directory revision，只有显式 `report_progress`、等待 interaction 和 terminal
变化会推进。Planner 没有其他工作时调用 `wait_agents`，该工具先订阅 watch 再读取 snapshot，
并在任一目标出现新 progress、interaction 或 terminal 时返回。等待没有 timer、deadline、
周期轮询或后台模型 turn；用户输入、中断和关闭会取消等待。

五分钟只是 Planner 的判断线索与 `read_agent_session` 查询权限阈值，不是 runtime timer
或失败事实。Planner 可先用 `list_agents` 查看 checkpoint 与调用时计算的
`summaryAgeSeconds`；仍有 active work 的 agent 超过五分钟没有摘要时，才可调用
`read_agent_session`。目标 turn 已 terminal 并回到 `Idle` 时可立即读取，即使 agent
lifecycle 仍为 `Active`。该工具只获取有界 user/assistant 文本和工具名称。Planner 必须
根据证据选择继续等待、
用 `send_message` 给出具体替代方向，或用 `interrupt_agent` 终止不安全/重复失败的 turn；
不得仅根据运行时间、普通工具活动或缺少 final 文本判定失败。

应用重启后从持久事实、agent snapshots 与 pending explicit input 恢复；遗留 Running turn
收束为 `Cancelled(runtime_restarted)`。没有 pending explicit input 的活动 Task 显示 paused，
由用户点击继续；attach 只对账 projection，不启动 Planner。Git 状态与 `expectedHead`
不一致时进入 `blocked`。

“继续任务”是唯一的无待处理输入恢复入口。GUI 只在 root Task session、活动 Task、Planner
无 Running turn 且 durable session 状态为 `interrupted` 时显示该操作。用户点击后，runtime
使用 `task-resume:{taskRunId}:{agentUpdatedAt}` 作为稳定 durable input id，把一条
`MailboxPresentation::Hidden` 输入提交给 Planner 的唯一 canonical session；该输入要求先
读取 `task_status` 与 `list_agents`，不会投影为 timeline user message，也不重建 reviewer、
executor 或旧 turn。重复点击、已有 pending input 或 agent 已恢复运行时必须拒绝或复用同一
durable receipt，不能并发创建第二个 Planner turn。

Studio 为每个 Task session 持有一个私有 `AgentRuntime<StudioHost>`（agent registry、
repository identity、task generation 与 lifecycle epoch）。同一 session 的用户 root turn
复用该 runtime，因此后续 planner 能继续 list、send 和 close 先前 turn 创建的
agent；不同 session 完全隔离。Simple mode 仍使用 turn-local runtime。planning
generation 在该 session 首次创建 `TaskRun` 时绑定 run id；run 终态且旧 turn 已静止后，
下一个 root turn 才安全轮换 generation，避免旧 agent path 泄漏到新任务。
同一 session 的 runtime 获取与 generation 轮换必须在稳定的 per-session cell 内单航班
执行；停止旧 generation 成功后原子替换该 cell，失败则保留原 entry。全局 registry 锁
不得跨越停止 runtime 的异步等待，不同 session 应能并行；shutdown 与获取/轮换通过
registry 生命周期门禁互斥，避免清空期间漏掉或覆盖 runtime。

已分配 worktree 的 agent 在后续显式 `send_message` turn 中
必须继续使用 agent entry 持有的 worktree path；父 planner 当前 workspace 只能提供模型与
turn 配置，不能覆盖 child 的工作区。worktree 句柄缺失或与 durable assignment 不一致时
拒绝启动 follow-up，不得回退到主工作区执行。

Task lifecycle adapter 在安装时绑定 Studio session，并通过该 per-session runtime 边界选择
持久 TaskRun；通用 `AgentSpawnLifecycleRequest.sessionId` 保持工具执行 turn scope 语义，
不得被误作 Studio session 身份或与 hook 绑定值比较。

root turn 结束或 UI 仅切换所选 session 不销毁 Task agent runtime。进程 shutdown 先停止
root turn，再复制 runtime 列表、释放 registry 锁，并逐个
cancel-and-wait/quiesce；该路径保留 durable worktree，不调用会 discard 且吞错的通用
`shutdown_descendants`。旧 epoch 的 agent 事件不得跨越 runtime restart 产生 UI 副作用，
也不得写入新 epoch 的 durable agent outcome 或终结观察事实。

真实进程重启不能恢复内存 task handle 或物理模型连接。取得 process lease 后，store 以
run-scoped 单事务把遗留 Running turn 收束为 `Cancelled(runtime_restarted)`，但保留 agent
canonical session、pending explicit input、WorkUnit、Outcome、delivery 和 worktree。Task
executor 的 WorkUnit/Outcome 与单次 turn 解耦；没有 pending explicit input 的活动 run
保持原 Task phase，并在 projection 中标为 paused。run、workUnitId、agentId、attempt 或
session owner 配对错位时事务整体回滚并 block 精确 run，禁止伪恢复 agent 或输入。

启动恢复顺序固定为 process lease、agent 事务收束、durable-aware worktree 对账、主仓库
校验，最后更新 canonical Agent Directory/Task projection。对账未完成或出现部分缺失资源
的 run 保持 paused 或进入阻断 issue；runtime attach 不重放 product signal、不提交输入，
也不启动模型。

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
首提交是独立、持久的项目准备操作；其后 TaskRun、lease 或 Planner 启动失败不得
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
任何 merge、conflict、design 或恢复路径写入 `blocked` 后，都必须汇入 coordinator 的同一
terminal barrier，不得只在 store 内结束 durable phase。barrier 以 task run id 与 generation
为身份提交终态事实，并终止该 generation 的内存工作：interrupt 当前 Planner turn、关闭
direct children、丢弃属于旧 generation 的未 claim 内部输入，再把可复用的 root agent
收束到 `Idle`。终态之后迟到 completion 由 RunningTurn identity 与 task generation 拒绝，
不得创建模型 turn；下一条明确的用户输入才允许开启新一轮 root 工作。runtime attach 必须
在放行 restored explicit input 前用最新 durable TaskRun 对账，因此进程在 terminal commit
与内存收束之间退出也不会恢复旧 turn。

Composer 对每个 session 维护一个 typed state，同时拥有 draft 与
`idle | submitting | pendingStart` 提交阶段；不得用 UI 控件本地文本、全局
`AsyncError` 或多个无关联 bool 表示同一次提交。点击发送先冻结完整 draft 并进入
`submitting`，Bridge 返回已经进入 canonical runtime queue 的 typed turn receipt 后才清空
draft，并以该 turn id 保持 `pendingStart`；session event 或 snapshot 观察到同一 turn 后才
解除 pending gate。提交失败时恢复完整 draft、回到 `idle` 并在 Composer 内展示可重试错误，
用户继续编辑会清除旧错误。`submitting | pendingStart` 期间键盘提交和发送按钮共享同一
single-flight gate，避免重复 turn；回调必须显式持有 Future，不能因 Widget callback 丢弃
尚未完成的 Bridge 调用。每次提交还必须递增 session-local submission revision；Bridge
成功或失败回调只能修改仍处于同一 revision 的 `submitting` 状态，过期 Future、错误
session receipt 或已经被 canonical turn 收束的状态一律丢弃。snapshot、event 与 canonical
state adoption 后统一对全部 session 的 Composer 与 `turnsBySession` 对账，不能只修正当前
选中 session。错误与 pending 状态都提供稳定 Driver key，供真实 GUI 验收。

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
标题；attempt 只提供单调审计序号，不设置固定轮数上限。只有真实 blocked、用户停止或
Task 终态才能结束 executor/reviewer 修复闭环。

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
`ownedPaths`，要求只修改所属 worktree、提交并调用 `report_completion`，并禁止派生、合并
或操作用户分支。developer prompt 还必须要求 executor 在完成定位、开始实现、开始验证、
遇到阻塞和准备交付时调用 `report_progress`，工具失败后先读取现状并修复根因或更换方案，
不得重复完全相同的失败调用。成功结果只返回
`{ agentId, sessionId, turnId, ownedPaths }`，不结束当前 Planner turn。Planner 可继续派发
独立工作；没有其他工作时调用 `wait_agents`，不轮询或主动催促。

## Executor 完成与交付审查

executor 的 required ending tool 是：

```text
report_completion {
  result:
    | { kind: delivery, headCommit, verificationSummary }
    | { kind: noDelivery, verificationSummary }
}
```

调用者必须是 WorkUnit 当前 executor。身份校验使用 runtime `parentId` 对应当前 Task
root agent id，并独立校验持久化逻辑 owner path 为 `/root`；agent id 与逻辑 path
属于不同命名空间，不得互相比较或把 `/root` 伪装成 runtime parent id。delivery 要求非空验证摘要、clean worktree、
HEAD 相对固定 base 推进、`headCommit` 与 HEAD 一致且全部改动位于 `ownedPaths`；
noDelivery 要求非空验证摘要、clean worktree 且 HEAD 未推进。成功调用在同一事务中创建
不可变 `WorkCompletion`，递增 completion revision，把 WorkUnit 置为 `ReadyForReview`，
把 progress 置为 `readyForReview`，并以 Completed 结束当前 executor turn。历史
completion 永不覆盖；每个 review 精确绑定 completion id、revision 和 full HEAD。

executor 普通文本结束而未成功调用 `report_completion` 时，WorkUnit 进入
`AwaitingCompletion`，保留 agent、session、worktree、branch 和 lease。runtime 不创建
recovery turn；Planner 读取 canonical Task/agent snapshot 后，只有显式调用 `send_message`
才让同一 executor 继续。关闭、重启或 terminal projector 不得静默 discard 可恢复成果、
自动合并或伪造 verification summary。工具错误或预算终止形成的
`AwaitingCompletion + AgentOutcome::Failed` 是合法、可恢复的 durable 配对；重启只能暂停
它，不能把该配对误判为损坏并阻塞整个 Task。

executor 的模型生命周期在 completion 审查期间保持可复用，但新的模型 turn 受 Studio
产品状态门禁约束。`Running` executor 可接收 steer，`AwaitingCompletion |
ChangesRequested` 可由 Planner 的显式 `send_message` 启动 follow-up；`ReadyForReview |
Reviewing | Approved | Merging | Merged | NoDelivery | Failed | Cancelled` 必须在消息提交前
拒绝新输入，并在 turn factory 准备模型前再次复核。该双重门禁防止并发边界或恢复输入绕过
WorkUnit 状态，同时不把 Task 状态泄漏进通用 AgentLoop。

delivery completion 只接受 HEAD 已推进且 worktree 干净的交付。`headCommit` 可使用完整 commit id 或
至少 7 位、在当前仓库中无歧义的十六进制缩写；runtime 必须通过 Git 解析并确认它就是
worktree HEAD，持久化和返回时统一规范化为完整 commit id。交付同时返回 `baseCommit`、
`changedFiles`、验证摘要和 `{ path, branch }`。runtime 不隐式执行 `git add -A`。
work unit 声明 `ownedPaths`；并行 executor 的写入范围不得重叠，超出范围的交付必须
返回 planner 决策。work unit 创建时固定记录 `baseCommit`、预期 worktree path 和
branch；交付校验不得改用随后可能因其他 executor 合并而推进的 task `expectedHead`，
且 caller workspace 和 branch 必须与固定记录精确匹配。rename/copy 同时校验 source
与 destination，delete 校验被删除的原路径。同时运行的 executor 最多四个。

`ownedPaths` 只接受相对规范路径或唯一的目录后缀 `/**`；裸尾随 `/`、`\` 与其他
通配符均拒绝。持久化和展示保留规范原文大小写，比较键遵循平台文件系统语义：
Windows 转为小写后进行 overlap 与交付匹配，Unix 保持大小写敏感。allocation 与
delivery 必须复用同一个解析模型，避免两条路径产生不同边界判断。

WorkUnit 状态固定为
`Pending | Running | AwaitingCompletion | ReadyForReview | Reviewing | ChangesRequested |
Approved | Merging | Merged | NoDelivery | Failed | Cancelled`。`ReadyForReview` 之后，
planner 调用 `task_request_delivery_review { executorAgentId }` 创建 fresh reviewer。
reviewer 只读精确 completion commit、base diff、ownedPaths、验证摘要与相关 design，并用
`review_exit` 写入 `scope=delivery` 的不可变 ReviewRound。review pass 将 delivery
WorkUnit 置为 `Approved`，noDelivery WorkUnit 置为 `NoDelivery`；findings 将 completion
与 round 标为 `ChangesRequired`，WorkUnit 置为 `ChangesRequested`。

Planner 在 reviewer terminal 后调用 `list_agents` 与 `task_status`。有 findings 时，必须用
`send_message` 把具体 finding 发给原 executor；同一 executor 在原 worktree 修复后创建新
completion revision，并由新的 reviewer 审查。旧 completion、reviewer 与 ReviewRound
保持不可变；同一 completion revision 可在 reviewer 启动失败、turn 失败或应用重启后由新的
provider call 创建新的审查轮次，但同一 WorkUnit 同时只允许一个 pending delivery review。
数据库唯一约束只保护 pending 审查与 provider call 的一次性授权，不能禁止同一不可变
completion 的后续 fresh review。旧 revision 或旧 HEAD 结果以 CAS 拒绝。循环没有次数上限，
只由 pass、真实 blocked、用户停止或 Task 终态结束。

delivery review pass 后，Planner 显式 `close_agent` 关闭 executor 的模型生命周期，但
`Approved` worktree、branch 和 completion 仍由 Task coordinator 持有；随后
`task_merge_agent` 合并，成功后才清理 Pure-owned worktree。`NoDelivery` 跳过 merge，但
仍参与最终整体完整性审查。Task worktree 不得调用通用的隐式 `commit_all`。
普通 `close_agent` 必须先在 Studio lifecycle 的只读 prepare 阶段校验 durable WorkUnit /
Outcome；`ReadyForReview | Reviewing | ChangesRequested | Merging` 在中断 turn、写入
framework `Closing` 或释放任何资源之前直接拒绝。commit 阶段以同一 durable identity
重新 CAS 校验：`Approved + Completed` 只关闭模型并保留资源供 merge，其余获准 discard
先提交 `cleanupRequested` 和终态，再释放 worktree。只有 durable discard 已提交后才属于
不可回滚边界；普通前置拒绝不得把 agent 错误置为 Faulted。
对 Pending、Running、AwaitingCompletion 或 ChangesRequested executor 执行 discard 时，
Task lifecycle hook
必须使用精确 lifecycle token，在一个事务中把配对 WorkUnit 与 AgentOutcome 幂等收束为
Cancelled，并标记终态已观察；只有 durable 处置成功后 supervisor 才能释放 worktree。
worktree 清理失败不回滚 Cancelled 事实，资源信息保留给恢复清理，但该 WorkUnit 不再占用
ownedPaths。重复 discard 不得产生第二次状态迁移或输入。
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

planner 调用 `task_merge_agent { agentId, expectedHeadCommit }`。runtime 只接受已关闭
且 WorkUnit 为 `Approved` 的 executor，校验当前分支和 completion commit 后执行
`git merge --no-ff --no-commit`。无冲突时先运行相关集成检查，通过后才创建 merge commit。
成功后更新 `expectedHead` 并释放 worktree；executor 通过审查一个即可合并。

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
`resolvingConflict`，保留 MERGE_HEAD、index 与 worktree，只 claim 一次 merge record，
更新 Task projection；executor worktree 继续受 durable owner 保护。

重启恢复按 merge phase 判断 Git 状态：`Pending | Verifying` 必须依据持久 prestate、
当前 HEAD、MERGE_HEAD 和 index 判断可安全继续、补偿还是 block；`Conflicted` 加
`resolvingConflict` 且现场与 conflict manifest 一致时是合法恢复状态，不得被普通 dirty
workspace 检查误判为外部漂移。`MergeRecord.verification_json` 承载版本化 `MergeEvidence`，
包含来源 phase、prestate、delivery identity、验证、commit、冲突 manifest、补偿与 cleanup
状态；保持现有六张 coordinator 表，不为 transient tool trace 新增协议或数据表。

每次 merge durable 接受后更新 Task projection。Planner 在当前 turn 中继续处理，或在
`wait_agents` 返回后重读 canonical Task snapshot；runtime 不记录或重放 merge completion
notification，也不创建 Planner 续轮。

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
不得创建 executor。计划应指出需要核验的设计领域和文档，但这些自然语言引用只提供
planner/reviewer 阅读上下文，不构成机器可执行的文件集合；coordinator 禁止从 Markdown、
反引号路径或 prompt 文本推导必须修改的目标。`task_update_design` 提交的完整 patch 是本次
设计修改范围的唯一声明，harness 只校验 patch 中所有 source/move destination 均位于
`design/**`、提交 diff 精确等于已验证路径、focused commit 与 branch HEAD/lease CAS
一致。创建 executor 前必须存在指向当前 `expectedHead` 的 durable design commit；任务取消
或部分失败时，design 必须回退或更新到与当前实现一致。
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
已触及的 design 路径和暂存区，不影响其他路径。失败结果必须明确说明 coordinator 没有
记录 design commit、已经完成回滚，并要求下一次调用重新提交完整的逻辑
patch；patch engine 列出的 applied changes 只描述失败前短暂写入的 hunk，不能暗示它们
已经成为 Git commit 或仍保留在工作区。

focused design commit 成功后，SQLite 在一个事务中以旧 HEAD 为 CAS，同时推进
`TaskRun.expectedHead` 与 `BranchLease.expectedHead`、记录 `designCommit`，并将初始
`designUpdating` 推进到 `implementing`。后续一致性更新只允许在没有进行中 merge 且
可继续实施或返工的 `implementing | reworking`。若事务失败，仅当 HEAD 仍是刚创建的提交且
工作区干净时补偿回旧 HEAD；无法证明安全时将该精确 run 标记为 `blocked` 并保留诊断，
不得覆盖外部变化。durable CAS 成功前 allocation phase gate 始终关闭，工具成功本身
不启动新 turn。

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
持久 `StopRequested`，但不立即进入不可逆 `stopping`，completion scope 继续开放。随后在不持
branch mutation lock 的情况下 interrupt active turn；每次 terminal fact 通知后重查
durable completion predicate，保留 10 秒总上限且不使用短轮询，再统一检查
全部 completion contract、HEAD 与 worktree。存在已推进 HEAD 或 dirty worktree 时停止返回
`deferred`，executor 保持 `AwaitingCompletion`，等待 Planner 显式 follow-up；只有不存在可恢复成果且所有合同已
终结时，才进入 `stopping` 并拒绝新的 allocation/completion。Task 专用事务先把所有未合并
WorkUnit、Outcome 和活动 ReviewRound 收束为取消/失败并授权对应 worktree cleanup，再关闭
内存 agent；不得让普通 delivery close 守卫承担 Task 停止语义。最后重新取得 branch guard，
复验 repository 后完成 revert、durable HEAD 推进和 terminalization。
应用在 `stopping` 中退出时，恢复流程只重放这条确定性停止 saga：先对账并续做已经
durable 授权的 worktree cleanup，再完成设计回退、Task `Cancelled` 与 BranchLease 释放；
不启动模型、不创建 continuation，也不把任务永久留在不可继续的 paused stopping 状态。
显式传入的 branch mutation guard 必须绑定创建它的 coordinator；其他 coordinator 的
guard 不得授权 locked mutation API。

应用在 reviewer turn 运行期间退出时，恢复事务不会重建 reviewer 或启动 Planner。它把
仍为 pending 的 ReviewRound 以 `failed(runtime restarted)` 收束，并把 reviewer Outcome
收束为 `Cancelled`；delivery review 精确复验 completion revision 与 reviewed HEAD 后把
WorkUnit 从 `Reviewing` 恢复为 `ReadyForReview`，integrated review 精确复验 Task HEAD 后
把 Task 从 `Reviewing` 恢复为 `Reworking`。旧 round 保持不可变审计记录，用户继续后
Planner 只能显式创建新的 reviewer。

停止意图使用服务端确定的
`TaskStopOrigin::{UserRequest, PlannerDecision, RuntimeFailure, ApplicationShutdown}`，并将
“谁发起停止”与独立的 `TaskStopReason` 持久化。模型工具 `task_stop` 只能形成
`PlannerDecision`，用户点击停止只能形成 `UserRequest`，模型输入不得伪造 origin。UI、
timeline 和 Task projection 都按 durable origin 渲染，不得把任意 `StopRequested`
固定描述成“用户请求停止”。

Task quiesce 在写入 `StopRequested` 的同一原子边界递增 task generation，阻止新的
executor/reviewer turn；随后取消活动 turn。所有完成、失败和取消路径汇入单一 durable
terminal-fact barrier，同一 generation 只能提交一个 task/session/queue/event 终态。
`StopRequested` 后不自动启动 Planner 或 executor；存在可恢复成果时保持
`AwaitingCompletion`，由 Planner 在停止前显式处理。

所有 WorkUnit 均为 `Merged | NoDelivery` 后，planner 必须先用 `task_update_design` 把
`designCommit` 推进到当前 `expectedHead`，再调用 `task_request_integrated_review {}`
创建 fresh integrated reviewer。review harness 在创建 round 和派生 reviewer 前重新校验
该不变量。reviewer 成功启动后不结束当前 Planner turn。`review_exit` durable transaction
更新 ReviewRound、Outcome、Agent Directory 与 Task projection，但不提交输入；Planner
没有其他工作时通过 `wait_agents` 等待 reviewer progress/terminal，再用 `task_status`
读取 canonical review。

integrated reviewer 绑定当前 Task HEAD，审查 plan、任务综合 diff、跨模块交互、合并结果、
测试缺口与 design 一致性。若有 findings，Task 进入 `reworking`，Planner 不重新打开已关闭
agent，而是基于 findings 创建新的 Integration Executor，声明受影响 `ownedPaths`，并让它
走同一 `report_completion -> delivery review -> 修复循环 -> close -> merge`。修复合并并
更新 design 后必须创建新的 integrated reviewer；循环同样没有次数上限。integrated pass
后才允许 `task_complete`。

审查创建以 delivery/integrated request 工具的 provider call id 作为一次性持久授权。
harness 消费授权后，`ReviewRound`、reviewer `AgentOutcome`、`ownerPath=/root` 和
`requestedByCallId` 必须精确配对；直接派生 reviewer 或重复消费授权均拒绝。

`ReviewRound` 带 `scope=delivery|integrated`。delivery scope 还带 completion id、
completion revision 与 reviewed HEAD；integrated scope 绑定 Task HEAD。`review_exit`
返回 `verdict`、`summary`、`designReferences` 和 `findings`。runtime 根据 tool trace 校验
reviewer 先成功定位文档，再以规范的 workspace-relative 路径读取 `design/**` 正文；该
校验必须读取完整 reviewer `AgentSession`。reviewer developer prompt 必须明确：design
读取使用 `path=design/...` 且 `cwd` 省略或为 `.`；completion worktree 的 `cwd` 只能用于
读取目标 source。路径、章节和 finding 引用都必须能在实际读取结果中验证。未调用
`review_exit` 便终结的 reviewer 会把本轮与 outcome 标记为失败并保留 paused 状态，不得
伪造通过或触发 Planner 输入。reviewer 成功退出后由生命周期清理自动关闭该临时 reviewer；
该关闭不通知父代理、不启动 turn。

所有 provider 工具调用使用同一个稳定调用身份：优先采用非空 provider `call_id`，缺失时
回退到通用 ToolCall `id`。消息历史、tool context、review/merge/conflict authorization 与
agent requestedByCallId 必须消费同一身份，使 Chat Completions 与 Responses provider
具有一致的 harness 关联语义。

只有 design 一致、所有 WorkUnit 均为 `Merged | NoDelivery`、当前分支干净、最新
integrated reviewer 对当前 HEAD 返回 `pass` 且验证通过时，planner 才能调用
`task_complete`。

`task_complete` 在共享 branch mutation lock 内重新校验 TaskRun、BranchLease、named
branch、clean workspace 与 exact HEAD；最新 integrated review 必须针对该 HEAD 返回
`pass`，所有 work unit、completion、outcome、delivery review 和 merge 都必须已收束。
存在已接受 source merge 时，最后一次
`task_update_design` 必须已经把 `designCommit` 推进到当前 HEAD。runtime 按任务综合变更
运行必要的最终检查后，以单事务写入 `completed` 并删除 BranchLease，再释放进程 lease。
一旦 `StopRequested` 已持久化，`task_complete` 在 coordinator 预检和 store 事务两层都必须
拒绝，不能绕过停止合同完成任务或释放 lease。

`task_stop` 先以 typed origin 持久化 `StopRequested` 并递增 task generation，再终止并等待
当前任务的内存代理到 canonical terminal，
随后检查 completion-required 合同和精确 worktree。可恢复提交或修改使停止返回 `deferred`，
不得关闭 completion scope、取消 outcome 或释放 worktree；由 Planner 显式 `send_message`
让 executor 完成报告后重新评估。
只有无可恢复成果时才在短锁事务中进入 `stopping`，将剩余 durable agent/work unit 收束为
`cancelled`，最后重新进入 branch mutation lock。尚无 source merge 时，如已接受 design commit，
必须先创建受控 revert commit；已有 source merge 时，必须先由 planner 更新 design 到当前
实现。该设计一致性检查必须在写入 `StopRequested` 和进入不可逆 `stopping` 之前完成；
检查失败时 run 保持原 phase 且仍可调用 `task_update_design`。存在尚未安全 abort 的
merge/conflict 时停止操作拒绝终态写入，保留现场供 planner 使用冲突工具处理。取消终态与
BranchLease 删除同样在一个事务中完成。
