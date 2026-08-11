# 17 - Thread Runtime 与产品宿主

## 17.1 Runtime 结构

```text
ThreadManager
  ├─ ThreadDirectory watch
  └─ ThreadHandle → ThreadActor → RunningTurn → TurnEngine
```

ThreadManager 管理 registry、容量和 spawn/close。ThreadHandle 查表后把 start、steer、interrupt、
snapshot 和 progress 命令直接发给目标 ThreadActor。只有 spawn/close 修改全局目录。

ThreadActor 唯一拥有 Thread revision、durable input queue 的内存镜像、活动 RunningTurn、取消
identity、live Item overlay 和当前 prompt generation/context baseline。它不缓存完整历史，也不
拥有 Task/worktree；context baseline 只用于生成模型输入差量，不能成为 runtime 事实源。

Agent activity 不再是调用方可独立写入的状态轴。公开形状固定为：

```text
Idle | Queued | Active(Running | WaitingTool | WaitingInteraction) | Cancelling
```

ThreadActor 在每次 commit 前从 lifecycle、RunningTurn.kind/cancelling 与 pending triggering input
派生 activity，优先级为：active cancellation → Cancelling，active Turn → Active(kind)，无 active
Turn 且存在 triggering input → Queued，其余 → Idle；非 Active lifecycle 不保留活动投影。调用方
不能直接修改 snapshot.activity。trace/tool/interaction 只更新 RunningTurn.kind，并在同一次
repository CAS 中提交 `TurnActivityChanged` runtime event 与新 snapshot。这样 durable snapshot、
directory watch 和产品投影共享一个提交点，重启恢复只从 active Turn 与 pending input 重建，不
维护第二份 activity truth。

所有改变 canonical session 的 Thread transition 都由 runtime 根据提交前后 session 自动派生
`Append | Replace | None`，调用方不能在 transcript 已变化时省略 context mutation。child 注册若
携带非空初始 transcript，必须写 replacement baseline；工作上下文与 child snapshot 同次提交。
TurnFinished/rollover 提交成功之前不得发布终态或调度 continuation。

产品可通过受限命令重配置 idle ThreadActor 的 role。该命令要求 lifecycle Active、没有活动 Turn、
active input 或 pending input，并通过 repository CAS 持久化 identity 与发布 directory revision；
运行中或排队中的 Turn 继续绑定创建时的 role，不允许热切换。

## 17.2 Host 端口

pl-core 只保留三个窄端口：

- `ThreadRepository`：以 expected revision 在单库事务中提交 Thread/Turn/Item/Input/Interaction
  mutation，并读取启动恢复所需状态。
- `TurnFactory`：准备 TurnEngine、request、instructions、tools 与 execution policy。
- `ChildLifecycle`：为 child Thread 准备/释放产品外部资源；Task 实现可以拒绝不安全的 close。

通知由 pl-core 在 repository 事务成功后直接发布，不经过额外 durable projection 或 replay
journal。Task tool 自己事务性写 TaskService；core 不携带 product mutation。

TurnFactory 为每次 turn 提供 typed `AgentWorkspace { root, boundary, mutability }`。Studio 通过
durable owner 解析 workspace：root/explorer 绑定 Project，executor 绑定 WorkUnit worktree，
Delivery reviewer 绑定目标 Completion worktree，Integrated reviewer 绑定 TaskRun 主 workspace。
child owner、Git identity 或路径无法精确解析时必须 fail closed；禁止因为进程内资源表缺失而
回退 Project root。进程内 lifecycle resource 只保存 handle/lease，不是 workspace 事实源。

## 17.3 取消与恢复

RunningTurn 包含 turnId、进程内 identity、当前 ActiveKind、CancellationToken、abort handle、done
和 steer sender。
completion 必须同时匹配 turnId 与 Arc identity。interrupt 先触发 token，等待一秒清理，超时才
abort；终态数据库事务成功后才能广播 turnCompleted。

产品提交的 `StartOrQueue` 输入可以携带通用 queue coalescing key。Thread idle 并准备下一 Turn
时，只合并队首连续、key 相同且仍为 pending 的输入：最后一条决定新 Turn identity，较早输入作为
该 Turn 的 `leadingInputs` 一并进入模型上下文。所有被合并输入先以同一 Turn claim 并持久化，首个
checkpoint 才 consume；进程在 checkpoint 前崩溃时仍可从 durable input queue 恢复。key 不相同、
中间存在其他输入，或首条输入已经 claimed/active 时不得合并，后到事实必须保留为下一 Turn。
coalescing key 属于 runtime envelope 元数据，不进入自然语言提示词或工具 schema。

Studio 使用专门的 durable interaction continuation 命令。`request_user_input` 产生 typed
`InteractionRequested` observation；ThreadActor 必须先在 repository 事务中提交 pending
Interaction，才允许原 Turn terminal。UserInput 回答和 PlanConfirmation `ContinuePlanning` 都由
ThreadActor 在一个 `ThreadCommit` 中同时提交 resolved Interaction、mail ID 为
`interaction-resolution:{interactionId}` 的 hidden input 和 `TurnQueued` runtime fact。该命令固定
采用 StartOrQueue，不读取 `active_turn_id` 猜测进程内 waiter，不 steer 活动 Turn，也不设置 queue
coalescing key。repository 失败时内存 state、Interaction projection 与 mailbox 都保持提交前状态；
重复命令以 pending/resolved Interaction 与稳定 mail ID 幂等收束。PlanConfirmation 的 fresh Turn
仍继承 Planner `RequiredTool(plan_exit)`，调整完成后必须再次产生新的确认。

RunningTurn 必须把“pending Interaction 已 durable 提交”携带为显式 completion boundary。Host 的
`RequiredTool` policy 不能把这个边界重写成 validation failure；它只在普通 Turn completion 时检查
业务 finalization tool，fresh Turn 继续继承同一 policy。

pending Interaction 是 active origin Turn 的成功 completion boundary，不再表达为独立的
等待 phase。ThreadActor 在 Interaction scope 的 turnId 等于 authoritative snapshot 当前
active Turn 时，把 pending Interaction 与随后的 `EndTurn` 一起提交，原 Turn 落 `completed`；
“等待用户”状态由 Thread 上挂的 pending Interaction 派生。Interaction resolution 通过稳定
mail ID 的 durable hidden input 在 fresh Turn 继续，绝不复活已 terminal 的 origin Turn 或
覆盖无关 Turn。普通 `budgetLimited` 不触发 continuation；预算续轮必须由产品状态机另行授权。

重启无法恢复物理连接。repository 在 manager 启动前收束遗留 active Turn/Item、恢复 queued
input 和 pending Interaction；manager 只创建 idle ThreadActor。任何恢复路径都不自动执行模型。

ThreadActor 另外提供 idle-only 的 conversation recovery 命令。Preview 读取 canonical session、
working state、Turn 消费的 mailbox input 与 runtime/session revision，不产生 mutation；Apply 同时
校验 expected runtime revision 与 expected session revision，并以 recoveryId 幂等。恢复时 transcript
replacement、working state、recovery marker、Thread revision 和通知必须由同一个 `ThreadCommit`
提交；提交冲突时 actor 不更新内存。

恢复模式为 `rewindTail | rebuildThread`。前者只接受经 user-message hash 和 tool 配对证明的安全
前缀，后者只重建普通 transcript。两者均保留 Timeline、usage、session note、Evidence Ledger 和
产品 owner，清空旧 Todo，推进 prompt generation，并废弃旧物理 model transport session。模型
恢复后的新 Turn 只能由显式 durable input 启动，conversation recovery 本身不自动执行模型。

Flutter Driver 的观察连接不是 Thread 生命周期 owner。只读 `snapshot` 在 disposed、closed 或 reset
时可重建连接并 health check，最多三次，退避 250ms、500ms、1s；tap、输入、prompt submit、计划
确认、恢复确认和 shutdown 永不自动重放。动作响应丢失后只能重连读取 canonical postcondition，
且 Driver reconnect 不刷新 Task stall 计时或 Task durable progress。

## 17.4 Agent control plane

模型工具名继续使用 spawn_agent、send_message、interrupt_agent、list_agents、wait_agents、
read_agent_session 和 close_agent；它们以 agentPath 解析 ThreadId。Thread directory 保存
root/parent/role/path/status/progress，不保存第二份 timeline 或 last turn outcome。

`wait_agents` 订阅 directory watch 后重读 snapshot，只因 progress、interaction 或 terminal
变化返回，并只返回本次变化 agent 的最新 progress message 和精简状态；没有 timer、轮询或
自动续轮。`list_agents` 保留完整目录查询，不作为 wait 后的重复刷新。child 内部 Item 只
进入 child Thread。
