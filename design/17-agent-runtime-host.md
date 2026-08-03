# 17 - Agent Runtime 与产品宿主边界

## 17.1 目标

`pl-core` 提供产品无关、Codex 风格的 agent 执行器。每个 agent 固定拥有一个
canonical session、一个 durable input queue、一个命令循环和最多一个活动 turn。Pure
Studio、mai 与未来宿主只提供模型、工具、持久化和外部资源适配。

Pure Studio 的产品运行时位于独立 `pl-studio-runtime` crate。`pl-core` 不依赖 SeaORM，
不读写 `.pure` 路径，不定义 Studio schema version，也不包含 Task、BranchLease 或
worktree 状态机。

## 17.2 执行结构

```text
AgentControl
  -> AgentHandle command channel
  -> AgentLoop
  -> RunningTurn
  -> TurnEngine
  -> durable commit
  -> AgentDirectory watch / SessionEventHub
```

- `AgentRuntime`：维护 agent registry、容量以及 spawn/close saga。
- `AgentHandle`：向目标 agent 的命令通道提交显式输入、中断和关闭。
- `AgentLoop`：唯一拥有 agent snapshot、session、input queue 与 `RunningTurn`。
- `RunningTurn`：保存 turn id、进程内 identity、取消句柄、worker 和 steer channel。
- `TurnEngine`：执行模型采样、工具调用、steer 消费与 checkpoint，不做 agent 调度。
- `AgentDirectory`：保存最新 canonical snapshots，并在 durable commit 后推进单一
  `watch` revision；watch 只提示变化，读取者必须重读 snapshot。
- `SessionEventHub`：按 session 提供 canonical snapshot、durable replay 与实时 UI channel。

一个 agent 只有一个 session。follow-up 输入复用该 session；需要独立上下文时创建新 agent。
运行时不维护 current-session fallback、多 session owner 解析或空 session 自动插入。

## 17.3 Host 端口

`AgentRuntimeHost` 组合三个窄端口：

- `AgentStateRepository`：以 expected revision CAS 原子提交 agent、session、input queue、
  turn、usage、trace、session projection 和 durable event journal。
- `AgentTurnFactory`：根据产品上下文准备 `PreparedAgentTurn`，不启动任务或修改状态。
- `AgentLifecycleAdapter`：以可回滚 lease 实现 spawn/close 外部资源 saga。

durable commit 完成后可以调用 product event sink 更新 read model，但 sink 不得反写 turn
状态、提交输入或启动新 turn。异步端口使用原生 RPITIT 并显式返回 `impl Future + Send`。

## 17.4 状态与命令

状态正交拆分为：

- lifecycle：`Active | Closing | Closed | Faulted`
- activity：`Idle | Queued | Running | WaitingTool | WaitingInteraction | Cancelling`
- turn outcome：`Completed | Cancelled | Failed | BudgetLimited`

AgentLoop 只处理：

- `SubmitMessage`
- `Interrupt`
- `TurnFinished`
- `Close`
- `Shutdown`

模型可见的控制面使用 `send_message { target, message }` 和
`interrupt_agent { target }`。`send_message` 永不取消：目标运行时进入 steer channel，
目标空闲时成为一个明确的新 turn 输入。`interrupt_agent` 只终止当前 turn，不夹带新消息。
`close_agent` 才终结 agent；产品 lifecycle 可以拒绝丢弃有可恢复资源的 agent。

输入使用稳定 `mail_id` 幂等。AgentLoop 先持久化输入，再将其送入当前 turn 或从 idle
启动新 turn。输入只区分来源与 delivery state，不存在 notification、wake trigger、
`CurrentTurn | NextTurn` 补偿状态或模型可见 delivery 枚举。

## 17.5 RunningTurn 与取消

`RunningTurn` 固定包含：

```text
turn_id
identity
cancellation_token
abort_handle
done
steer_sender
```

`identity` 是进程内 `Arc` 身份。worker completion 回到 AgentLoop 时，必须同时匹配 turn id
并通过 identity 比较；不再拥有当前 turn 的 completion 直接丢弃。进程内迟到保护不使用
dispatch generation、waiting epoch 或 wake generation。

interrupt 顺序为：

1. AgentLoop 取走当前 `RunningTurn` 并进入 `Cancelling`。
2. 触发 CancellationToken。
3. 最多等待一秒让 worker 正常清理。
4. 超时后 abort，并等待 worker/trace drain 收束。
5. 持久化 interrupted marker、session checkpoint 与 turn outcome。
6. 推进 AgentDirectory watch 和 SessionEventHub。
7. 仅根据 canonical pending input 决定是否启动下一 turn。

worker 被取消后不得提交普通 completion。终态事件必须在 durable transcript 和 outcome
可读取之后广播。SQLite expected revision、session event sequence 和产品 Task generation
继续承担跨进程 CAS 与 durable 顺序，不能用进程内 identity 取代。

## 17.6 协作与等待

协作工具由 `pl-core` control plane 基于 registry、AgentDirectory 和访问策略实现：

- `spawn_agent`
- `report_progress`
- `send_message`
- `interrupt_agent`
- `list_agents`
- `wait_agents`
- `read_agent_session`
- `close_agent`

`report_progress` 由 agent 更新自己的 typed checkpoint：

```text
stage: exploring | implementing | verifying | blocked | readyForCompletion
summary
nextStep
revision
updatedAt
```

相同 checkpoint 不推进 revision。progress 只描述事实、当前结论、下一步和阻塞，不包含
隐藏推理。`readyForCompletion` 只表示 executor 准备调用产品层 required ending tool，
不表示 durable completion 已存在，也不授权审查。产品层只有在 completion 事务成功后，
才可把 canonical checkpoint 提升为 `readyForReview`。

`list_agents` 返回可见 agent 的稳定排序 snapshot，以及调用时根据 `updatedAt` 计算的
`summaryAgeSeconds`。派生 age 不持久化。

`wait_agents` 先订阅 AgentDirectory watch，再读取 canonical snapshots。它只在目标 agent
出现新 progress、等待交互或 terminal 时返回；没有 inactivity timer、deadline、周期轮询
或 synthetic continuation。父 turn 的用户输入、中断或关闭会取消等待。

`read_agent_session` 只在调用时检查 progress age。`Active` lifecycle 且 activity 非
`Idle` 的 agent 才视为仍有 active work，摘要未满五分钟时拒绝读取；turn 已 terminal
并回到 `Idle` 时可立即读取，即使 agent lifecycle 仍为 `Active`。达到五分钟或 agent
已 terminal 时，返回有界 user/assistant 文本。system、developer 和
reasoning 不返回；工具调用只返回工具名，不返回参数与结果。

不存在父代理自动订阅唤醒、completion notification、accepted wake receipt、
`WaitingAgentsSupervisor` 或 ProductGated signal。Planner 没有其他工作时显式调用
`wait_agents`；agent 沉默本身不会启动模型。

## 17.7 执行与恢复

一次 turn 依次执行：

1. AgentLoop 以 CAS claim durable input。
2. `AgentTurnFactory` 准备 kernel、instructions、tools 与 policy。
3. worker 运行 TurnEngine，并通过 steer channel 接收显式 follow-up。
4. checkpoint 由 AgentLoop 校验当前 turn identity 后提交。
5. worker completion 回到 AgentLoop，完成 durable session、trace、usage 和 outcome commit。
6. commit 成功后更新 watch 与 UI event，随后处理下一条 pending input。

应用重启无法恢复物理模型连接。遗留 Running turn 收束为
`Cancelled(runtime_restarted)`；pending explicit input、session、Task 事实和 worktree 保留。
没有 pending explicit input 的活动 Task 进入 paused，由用户显式继续；attach 不重放产品
signal，也不启动 Planner。

产品层若有绑定物理 turn 的临时 reviewer，必须在同一个恢复事务中收束其 pending review
与 outcome，并回到可由显式输入继续的产品状态；恢复不得重建 reviewer、提交父输入或启动
Planner。已完成的 completion、review round 历史和 worktree 继续保留。

`request_user_input` 的进程内 waiter 与 durable interaction 分层。同进程回答直接恢复 waiter；
detached 回答使用稳定 `mail_id = interaction:<id>` 提交到 canonical agent queue，收到 durable
receipt 后再标记 interaction resolved。崩溃恢复只按 mailbox/turn receipt 修复 projection，
不使用独立 continuation outbox，不创建第二个输入。

## 17.8 Studio 边界

`pl-studio-runtime` 拥有 Studio config、SQLite schema、project/session/Task/worktree、
Simple/Task policy 和 UI-facing records。TaskRun、WorkUnit、AgentOutcome、MergeRecord、
ReviewRound 与 BranchLease 是产品合同，不进入 TurnEngine。

Studio 数据库只接受当前精确 schema。低版本或未版本化用户库在完整归档 DB、`-wal` 和
`-shm` 后重建；更高版本拒绝打开。运行期不保留 migration chain、backfill 或旧 DTO 双栈。

## 17.9 Session 与 UI 不变量

- 一个 session 使用独立 durable sequence 和 bounded broadcast channel。
- durable event 只有在 repository transaction applied 后才广播。
- transient delta 只在当前 turn identity 与 part revision 仍有效时广播。
- AgentDirectory 使用单一 watch revision；watch lag 不需要事件重放，读取者直接读最新 snapshot。
- SessionEventHub 的 channel lag、cursor 越界或 delta revision 缺口仍返回 resync。
- Agent Directory 更新所有 root session；Flutter 不因当前选择不同而丢弃 agent 状态。
- 父 timeline 只展示父 agent 自己执行的协作工具，不复制 child 工具、reasoning 或 synthetic
  用户消息。
