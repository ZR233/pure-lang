# 17 - Thread Runtime 与产品宿主

## 17.1 Runtime 结构

```text
ThreadManager
  ├─ ThreadDirectory watch + ToolManager
  └─ ThreadHandle → ThreadActor → RunningTurn → TurnEngine
```

ThreadManager 管理 agent registry、`ToolManager`、容量和 spawn/close。每个驻留 Thread/agent 持有
一个持久 `AgentToolSet`，并显式决定是否继承 global 工具。ThreadHandle 查表后把 start、steer、interrupt、
snapshot 和 progress 命令直接发给目标 ThreadActor。只有 spawn/close 修改全局目录。

ThreadActor 唯一拥有 Thread revision、durable input queue 的内存镜像、活动 RunningTurn、取消
identity、live Item overlay 和当前 prompt generation/context baseline。它的内存 snapshot 是该
会话的唯一权威实例，SQLite 由 write-behind 批量事务异步跟随（持久化语义见 19.2，驻留策略见
19.6）。它保留当前驻留期观察到的 Turn 热窗口和完整 live Item timeline，用于覆盖尚未耐久化或版本
落后的历史页；更早 Turn 仍从 SQLite 冷分页。它不拥有 Task/worktree；context baseline 只用于生成
模型输入差量，不能成为 runtime 事实源。

Agent lifecycle、activity 与 active Turn 不再是三个可独立写入的状态轴。唯一公开状态固定为：

```text
Idle | Queued | Running | WaitingTool | WaitingInteraction | Cancelling
| Closing | Closed | Faulted
```

`AgentState` 的每个 variant 承载独立 state struct；Running、WaitingTool、WaitingInteraction 与
Cancelling 必须携带 active Turn identity，WaitingInteraction 还携带 Interaction identity，Faulted
携带 typed error 和可选诊断 Turn。ThreadActor 只接受 `AgentCommand`，由状态机返回 next state 与
effects；trace/tool/interaction、mailbox 和 close saga 不得直接写 snapshot 字段。pending triggering
input 通过显式 Queue/Start command 把 Idle 推进到 Queued/Running，不能再由调用方临时派生 activity。
同一次内存 commit 原子提交 state、runtime event 与 snapshot，directory watch 和产品投影读取同一
状态事实源。

Faulted 携带的可选 Turn 只用于关联故障来源，不表示该 Turn 仍在活动。故障收束包含来源 Turn 时，
同一个内存 commit 必须同时保存 Faulted Agent 状态和该 Turn 的 Failed 终态，并立即发布；持久化
投影不得再把这个诊断 Turn 投影为 queued/running。诊断 Turn 与 `last_turn` 的身份或终态不一致属于
内存转换前的不变量破坏，必须拒绝该转换，不能先修改 owner 再由广播或 SQLite 错误把 Agent Faulted。

Faulted 额外携带类型化故障分类：可恢复运行时故障、可恢复协议故障、聚合损坏和未知旧故障。只有前
两类可接受 `RecoverFaulted`；该命令必须先验证快照、修订号、transcript 与诊断 Turn，终结遗留活动
Turn、清除忙碌投影并回到 Idle。聚合损坏和未知旧故障保持 Faulted。旧字符串故障只有命中已知 reasoning
分块回归且会话审计通过时，才能在恢复加载阶段升级为可恢复协议故障。

Studio `AgentDirectoryChanged` 直接携带上述九态相邻标签 union；目录条目不再并列传输
`status/lifecycle/activity/activeTurnId/error/reason`。Flutter 以 sealed `StudioAgentState` 穷尽消费，
展示标签和 fault message 只能作为只读派生值。Faulted 的 `StateError` 保留 code、message、retryable
及可选诊断 Turn，不能在 Bridge 层退化成字符串。

Thread 的公开状态只从 canonical Agent/Turn 状态穷尽投影为 Idle、Queued、Running、WaitingTool、
WaitingInteraction、Cancelling、Closing、Closed、Faulted；它不是另一个可写 lifecycle。没有实际
生产来源的 Completed Thread 状态被删除。

所有改变 canonical session 的 Thread transition 都由 runtime 根据提交前后 session 自动派生
`Append | Replace | None`，调用方不能在 transcript 已变化时省略 context mutation。child 注册若
携带非空初始 transcript，必须写 replacement baseline；工作上下文与 child snapshot 同次提交。
TurnFinished/rollover 在内存提交后立即发布终态并调度 continuation，不等待 SQLite。只有会淘汰 owner、
正常关机或执行不可逆外部动作时，才显式等待目标修订号耐久化。

产品可通过受限命令重配置 idle ThreadActor 的 role。该命令要求 lifecycle Active、没有活动 Turn、
active input 或 pending input，并通过 repository CAS 持久化 identity 与发布 directory revision；
运行中或排队中的 Turn 继续绑定创建时的 role，不允许热切换。

## 17.2 Host 端口

pl-core 只保留三个窄端口：

- `ThreadRepository`：纯持久化端口，接收已经由 ThreadActor 提交的
  Thread/Turn/Item/Input/Interaction 批次，提供耐久修订号、待写查询、显式屏障和惰性恢复读取；
  它不返回业务转换结果，也不决定内存提交是否成立。耐久屏障只有一种形式：显式
  `awaitDurable(threadId, revision)`；端口不提供按线程的无目标 flush，全局排空只属于宿主关机
  流程。实现侧 repository 与 TaskRuntime 共享同一个进程级 writer 实例，恢复出的耐久基线必须
  seed 进该共享实例，不构造只读用的第二 writer。
- `TurnFactory`：准备 TurnEngine、request、instructions、持久 `AgentToolSet` 与 execution policy；
  不为每个 Turn 建立临时工具注册表。
- `ChildLifecycle`：为 child Thread 准备/释放产品外部资源；Task 实现可以拒绝不安全的 close。

通知由 pl-core 在内存 state 更新后直接发布，不经过额外 durable projection 或 replay journal；
Turn 终态、Interaction 提交与 resolution 都不等待 flush。发布通道关闭或消费者落后只记录诊断并
触发从 ThreadActor 快照重同步，不得使已经提交的 Agent 进入 Faulted。Task tool 把类型化事实提交给
TaskRuntime；core 不携带 SQLite mutation。

合法 provider 输入上的 trace/Thread 投影准备错误只终结当前 Turn，使用类型化协议失败结果并让 Agent
回到可接收输入状态。只有 ThreadActor 的复合聚合本身无法验证、或无法构造一致的 Turn 终态时，才允许
进入 Faulted。每个独立 reasoning TracePart 内的 Thinking 与 ReasoningContent 首个增量都使用本地
分块编号零；provider 原始 chunk index 只标识独立条目和摘要关联，条目内部真正跳号仍严格拒绝。

ThreadEventBus 是 timeline 顺序的唯一分配者：新 item 的 ordinal 在通知首次应用时按到达序
分配（`max+1`），首次分配后不可变；投影/广播/落库消费同一份规范化通知。生产者（runtime
事件投影、trace 投影、observation 投影）不再自行计算 timeline ordinal，trace 的
`started_sequence` 只作为 trace 事件自身的去重与批内排序键。

`ProgressEmitter` 的 milestone 与模型 trace 共用同一 durable trace 入口，投影为 runtime source
的 commentary Item。它不得只发送进程内 observation broadcast；Thread commit 成功后，订阅、
历史与重启恢复必须看到同一 Item。heartbeat、百分比等瞬时 progress 继续只属于 runtime snapshot。

TurnFactory 为每次 turn 提供 typed `AgentWorkspace { root, boundary, mutability }`。Studio 通过
durable owner 解析 workspace：root/explorer 绑定 Project，executor 绑定 WorkUnit worktree，
Delivery reviewer 绑定目标 Completion worktree，Integrated reviewer 绑定 TaskRun 主 workspace。
child owner、Git identity 或路径无法精确解析时必须 fail closed；禁止因为进程内资源表缺失而
回退 Project root。进程内 lifecycle resource 只保存 handle/lease，不是 workspace 事实源。

RunningTurn 在每个 provider 请求前调用一次宿主 `before_model_step` 刷新端口。端口获得 agent
identity、turn/step identity、当前 route 与目标 `AgentToolSet` 的事务窗口，只能原子替换本 agent
的 registration group；失败时在发请求前终止该 step，不能留下部分工具。窗口关闭后 core 冻结
`ToolPlan`，同一次请求、transport retry 和其返回的 tool calls 全部使用该 plan。Task phase、
collaboration caller、MCP generation、LSP availability 或宿主自定义能力的变化从下一 model step
可见，不重写在途请求。

## 17.3 取消与恢复

RunningTurn 包含 turnId、进程内 identity、canonical Turn running state、CancellationToken、abort handle、done、
steer sender 与单一 budget-refresh signal。
completion 必须同时匹配 turnId 与 Arc identity。interrupt 先触发 token，等待一秒清理，超时才
abort；Turn 终态完成内存提交后立即广播 turnCompleted。

parent→direct-child `send_message` 是唯一预算刷新 mailbox。runtime 在 durable `TurnQueued` 提交且
steer 被活动 Turn 接受后，以消息接受时刻推进 refresh signal；TurnEngine 在每个预算检查点应用
最新 epoch，重置 wall-clock、等待排除与本 tranche 的 model/tool/wait 计数，不取消或替换活动
Turn。消息送到 idle child 时 fresh Turn 自带新预算。该语义以 typed mailbox budget action 表达，
并复用 `$plAgentRuntime` metadata 持久化；不得根据 hidden presentation、自然语言或 tool 名称反推。
Interaction continuation、Planner wake 与产品自动 continuation 固定 Preserve。

所有 local、remote、automatic、manual 与 rollover compaction 共用一个 execution controller。
attached Turn 传入同一 CancellationToken，provider-backed operation 固定 120 秒硬超时；取消和超时
均 drop 未完成 future，且 session replacement 只在完整成功后安装。rollover 的失败被保存为
BudgetLimited outcome 的 compaction error，不能卡在预算 Item 与 TurnFinished 之间；用户 stop
取消 compaction 后仍按 interrupt/cancelled 终态收束。

产品提交的 `StartOrQueue` 输入可以携带通用 queue coalescing key。Thread idle 并准备下一 Turn
时，只合并队首连续、key 相同且仍为 pending 的输入：最后一条决定新 Turn identity，较早输入作为
该 Turn 的 `leadingInputs` 一并进入模型上下文。所有被合并输入先以同一 Turn claim 并持久化，首个
checkpoint 才 consume；进程在 checkpoint 前崩溃时仍可从 durable input queue 恢复。key 不相同、
中间存在其他输入，或首条输入已经 claimed/active 时不得合并，后到事实必须保留为下一 Turn。
coalescing key 属于 runtime envelope 元数据，不进入自然语言提示词或工具 schema。

Studio 使用专门的 durable interaction continuation 命令。`request_user_input` 产生 typed
`InteractionRequested` observation；ThreadActor 必须把 pending Interaction 与原 Turn 终态放在
同一个内存 commit 中，随后立即允许原 Turn terminal。UserInput 回答和 PlanConfirmation
`Confirm` 与 `RevisePlan` 都由 ThreadActor 在一个 `ThreadCommit` 中同时提交 resolved Interaction、mail ID
为 `interaction-resolution:{interactionId}` 的 hidden input 和 `TurnQueued` runtime fact。该命令固定
采用 StartOrQueue，不读取 `active_turn_id` 猜测进程内 waiter，不 steer 活动 Turn，也不设置 queue
coalescing key。Interaction 提交与 resolution 立即对内存订阅者可见；落库失败进入全局持久化
降级状态并暂停新工作，不回滚已提交事实。重复命令以 pending/resolved Interaction 与稳定 mail ID
幂等收束。`RevisePlan` 的 fresh Turn
仍继承 Planner `RequiredTool(task_transition)`，调整完成后必须再次提交计划并产生新的确认。

RunningTurn 必须把“pending Interaction 已 durable 提交”携带为显式 completion boundary。Host 的
`RequiredTool` policy 不能把这个边界重写成 validation failure；它只在普通 Turn completion 时检查
业务 finalization tool，fresh Turn 继续继承同一 policy。

pending Interaction 是 active origin Turn 的成功 completion boundary，不再表达为独立的
等待 phase。ThreadActor 在 Interaction scope 的 turnId 等于 authoritative snapshot 当前
active Turn 时，把 pending Interaction 与随后的 `EndTurn` 一起提交，原 Turn 落 `completed`；
“等待用户”状态由 Thread 上挂的 pending Interaction 派生。Interaction resolution 通过稳定
mail ID 的 durable hidden input 在 fresh Turn 继续，绝不复活已 terminal 的 origin Turn 或
覆盖无关 Turn。普通 `budgetLimited` 不触发 continuation；预算续轮必须由产品状态机另行授权。

重启无法恢复物理连接。repository 在 manager 启动前收束钉住集合遗留的 active Turn/Item、恢复
queued input 和 pending Interaction；manager 只为钉住集合创建 idle ThreadActor，其余 Thread 惰性
驻留（见 19.6），订阅、提交输入或 Task 恢复引用时按需恢复。任何恢复路径都不自动执行模型。

ThreadActor 另外提供 idle-only 的 conversation recovery 命令。Preview 读取 canonical session、
working state、Turn 消费的 mailbox input 与 runtime/session revision，不产生 mutation；Apply 同时
校验 expected runtime revision 与 expected session revision，并以 recoveryId 幂等。恢复时 transcript
replacement、working state、recovery marker、Thread revision 和通知必须由同一个 `ThreadCommit`
提交；conversation recovery 完成内存提交后立即可见，SQLite 异步跟随。内存是唯一 writer，不存在
进程内冲突；落库失败只改变持久化健康状态。

恢复模式为 `rewindTail | rebuildThread`。前者只接受经 user-message hash 和 tool 配对证明的安全
前缀，后者只重建普通 transcript。两者均保留 Timeline、usage、session note、Evidence Ledger 和
产品 owner，清空旧 Todo，推进 prompt generation，并废弃旧物理 model transport session。模型
恢复后的新 Turn 只能由显式 durable input 启动，conversation recovery 本身不自动执行模型。

Flutter Driver 的观察连接不是 Thread 生命周期 owner。只读 `readThreadSnapshot` 只能读取
repository 与已驻留 actor 的 live overlay；actor 未驻留时返回 inactive，不触发恢复、不改 role、
不投递 durable wake。wire 快照（订阅 bootstrap 帧与 `readThreadSnapshot`）在 bridge 转换层
窗口化：items 按整 Turn 对齐截断到最近 400 条，并携带 `historyCursor`（窗口首 Turn 的 id，
`listThreadTurns` 的 before 语义锚点）——内部 protocol 快照保持全量，只有 wire 边界窗口化。`subscribeThread` 是显式激活命令，未驻留 Thread 经订阅按需恢复；连接
disposed/closed 时 Driver 可以重建 transport 并再次纯读；tap、输入、prompt submit、计划确认、
恢复确认和 shutdown 永不自动重放。动作响应丢失后只能重连读取 canonical postcondition，且
Driver reconnect 不刷新 Task stall 计时或 Task durable progress。

启动 command 负责建立全部未归档 durable Thread 的目录索引（见 19.6），只为钉住集合 materialize
pending wake。运行中 actor 缺失由 `repairThreadRuntime(threadId)` 或订阅/提交输入按需恢复；
`readThreadSnapshot` 对未驻留 Thread 仍返回 typed inactive，不产生副作用。驻留 actor 由 manager
的 LRU 双端队列管理：订阅、提交或修复时移到队尾；空闲判定为无活动 Turn、无活跃订阅且无
pending input，超容量时从队首淘汰，淘汰前显式等待该 Thread 目标修订号耐久化，不要求冲刷无关
owner。订阅是
显式观察者注册：bridge 订阅 producer 存活期间持有驻留 pin，被观察的线程不参与淘汰，订阅
取消/流关闭即解除 pin——淘汰一个仍被订阅的线程会让该订阅流永久静默（总线无事件也无关闭
信号）。

## 17.4 Agent control plane

模型工具名使用 spawn_agent、report_progress、send_message、interrupt_agent、list_agents、
wait_agents、read_agent_session、read_agent_submissions 和 close_agent；它们以 agentPath 解析
ThreadId。Thread directory 保存 root/parent/role/path/status/progress，不保存第二份 timeline
或 last turn outcome。

通信模型为 **pull**：子代理不得向父代理或 peer 主动 push 消息。

- `send_message` 是唯一的消息插入原语，且仅允许 **parent→direct-child** 方向（main→sub
  调度）。user→planner 由宿主 `AgentRuntimeHandle::submit` 走同一原语。所有 agent（含 Task
  planner）共享同一套协作基础能力，不再按模式剥离 send_message。
- `report_progress` 是子代理向主代理汇报的唯一通道：每次调用追加一条 durable 阶段提交到
  `thread_submissions`（含可选 `detail` 实质负载），并照旧更新 snapshot checkpoint。主代理通过
  `wait_agents`（实时最新增量）或 `read_agent_submissions`（全历史、分页、不截断、子代理关闭
  后仍可查）主动 pull。`report_progress` 从不创建 completion 或 review 授权。
- `thread_submissions` 表生命周期隶属于该 Thread（主 agent 会话树），子代理关闭后行保留。

`wait_agents` 订阅 directory watch 后重读 snapshot，只因 progress、interaction 或 terminal
变化返回，并只返回本次变化 agent 的最新 progress message 和精简状态；没有 timer、轮询或
自动续轮。`list_agents` 保留完整目录查询，不作为 wait 后的重复刷新。child 内部 Item 只
进入 child Thread。Faulted/Closed 等非 operational 状态直接视为 terminal；Idle 只有在没有活动
Turn 且 pending input 为零时才视为 settled。Faulted 携带的诊断 Turn 不属于 `activeTurnId` 判定。
fresh Turn 已分配 ID 但尚未发布首个 activity 的窗口不能泄漏上一 Turn 的 `lastTurnOutcome`。
`wait_until_idle` 与 `wait_agents` 共用该判定。

Faulted 是当前 Agent 代次的 terminal 事实。Faulted Agent 与来源 Turn 失败结果完成同一内存提交后，
runtime 立即发布目录 revision 并使 `wait_agents` 返回，同时把类型化终态投递给 TaskRuntime；SQLite
在后台异步跟随。等待方不得复活或重放已经结束的来源 Turn。只有显式 `RecoverFaulted` 路径在快照、
修订号与 transcript 全部验证通过后，才可终结遗留活动 Turn、清除忙碌投影并把同一会话 Agent 转为
Idle；恢复创建下一条全新 Turn，不复活旧 TaskRun 或旧 Turn。聚合校验失败时继续保持 Faulted。
