# 17 - Agent Runtime 与产品宿主边界

## 17.1 目标

`pl-core` 是产品无关的 agent 框架。它唯一拥有 agent、session、输入队列、活动 turn、
取消、等待、恢复和协作状态机；Pure Studio、mai 及未来宿主只提供模型、工具、持久化和
外部资源适配。

Pure Studio 的产品运行时位于独立 `pl-studio-runtime` crate。`pl-core` 不依赖 SeaORM，
不读写 `.pure` 路径，不定义产品 schema version，也不导出 `Studio*` API。

## 17.2 核心层次

- `TurnEngine`：执行单轮模型与工具循环。
- `AgentKernel`：一次 turn 可运行的 provider、instructions、tools 与策略组合。
- `AgentSession`：有序上下文、压缩状态与 usage。
- `AgentRuntime<H>`：协调 agent actor，并通过 `AgentRuntimeHost` 调用产品能力。
- `AgentRuntimeHandle`：不含 host 泛型的命令 sender，供产品 facade 和协作工具使用。
- `AgentEventHub`：保存 canonical agent snapshots，在 durable commit 后向 direct-parent
  subscription 发布 typed meaningful update；lag 只发 stale，订阅者重读 snapshot。
- `WaitingAgentsSupervisor`：合并 parent updates，并用单一 timer queue 管理每个 direct child
  的独立 inactivity deadline。
- `SessionEventProjector`：把 runtime/trace/working-set facts 投影为唯一公共 session 协议。
- `SessionEventHub`：按 session 提供 snapshot、durable replay 和实时 channel。

每个 agent actor 独占 session 集合、durable mailbox、活动 turn、取消句柄、
`dispatch_generation`、revision 和最近结果。产品不得维护第二套 active-turn、queue 或
cancel 状态。mailbox 中的 `DurableMailboxEnvelope` 使用稳定 `mail_id` 幂等，记录来源、
payload、入队序号、Unix 秒时间戳、`MailboxTurnTrigger::{DoNotStart, StartIfIdle}` 与
`MailboxDeliveryState::{Pending, Claimed { turn_id, checkpoint_seq },
Consumed { turn_id, checkpoint_seq }}`。session durable cursor 由
`SessionEventHub` canonical projection 唯一拥有；actor/repository 中的 sequence 只是提交后
checkpoint 镜像。

## 17.3 Host 端口

`AgentRuntimeHost` 组合四个窄端口：

- `AgentStateRepository`：恢复 runtime，并以 `AgentCommit` 原子提交 snapshot、session、
  turn、队列、usage、session projection、durable event journal 与 trace；同时提供 session
  snapshot/replay 读取；所有提交使用 expected revision 做 CAS。
- `AgentTurnFactory`：根据产品上下文准备 `PreparedAgentTurn`，不启动任务或修改状态。
- `AgentLifecycleAdapter`：以可回滚 lease 实现 spawn/close 外部资源 saga，操作必须幂等。
- `AgentCommitObserver`：在 PL session channel 广播完成后观察已提交事实，只更新产品 read
  model 和低频 product event；失败由实现内部记录，不回滚 durable 状态。

异步端口统一使用原生 RPITIT 并显式返回 `impl Future + Send`，不使用 `async_trait` 或
trait object。

## 17.4 状态与命令

状态正交拆分：

- lifecycle：`Active | Closing | Closed | Faulted`
- activity：`Idle | Queued | Running | WaitingTool | WaitingInteraction | WaitingAgents`
- turn outcome：`Completed | Cancelled | Failed | BudgetLimited`

turn 完成后回到 `Active + Idle`。turn 失败是结果而非 agent 生命周期失败；只有持久化
终态失败或无法补偿的不变量破坏会进入 `Faulted`。

`AgentRuntimeHandle` 提供 `register`、`submit`、`submit_current_session`、`cancel_turn`、
`close`、`snapshot`、`list`、`subscribe_children`、内部 `wait_until_idle`、
`subscribe_session`、`session_snapshot` 和 `shutdown`。模型可见的跨 agent 输入只表达
`send_input { target, message, interrupt: false }`：`interrupt=false` 是显式 steer，
`interrupt=true` 是显式取消当前 turn 后以新 generation 启动输入。模型不得选择
`QueueOnly`、`InterruptThenStart` 或通用 quiesce；内部 runtime/recovery 仍可使用强类型
delivery 命令，并在领域入口转换为 mailbox trigger。未关闭 agent 可连续接收输入，不存在
`resume_agent`。

mailbox 还维护 `MailboxDeliveryPhase::{CurrentTurn, NextTurn}`。用户可见 final 在对外发送
前，必须与 session checkpoint 在同一 CAS 中切换为 `NextTurn`；显式 steer 或 final 后确有
后续 tool call 时，按同一 turn revision 重新开放 `CurrentTurn`。任何 defer、wake 或 start
reservation 都携带 turn generation/revision，过期操作不能覆盖新 phase 或启动新 turn。

固定投递语义如下：

| 输入 | 活动 turn | 已输出 final 或 agent 空闲 |
|---|---|---|
| 显式 steer，`interrupt=false` | 加入当前 turn，并重新开放 `CurrentTurn` | 启动一个新 turn |
| notification，`trigger=false` | 仅在 `CurrentTurn` 消费 | 留在 mailbox，不自动启动 |
| task/wake，`trigger=true` | 可在 `CurrentTurn` 消费 | 当前 turn 结束后或空闲时只启动一次 |
| 显式 `interrupt=true` | 取消当前 turn，以新 generation 启动输入 | 直接启动新 turn |

跨 agent 输入必须先由 runtime resolver 生成
`ResolvedAgentSessionTarget { root, agent, current_session, owner_revision }`。submit 只接受
该强类型目标；未知、历史、跨 root 或 owner 不匹配的 session 直接失败，不得回退到调用者
session，也不得在 actor 中自动插入空 session。协作工具只提交目标 agent id；coordinator
先解析稳定 root，目标 actor 再在同一条命令内原子解析 current session 并入队。活动 turn
或 durable pending FIFO 只能指向一个 current session；idle agent 只有一个 owned session
时可直接解析，存在多个 idle 历史 session 且没有 current 指针时必须报歧义，禁止使用
`last_turn.session_id` 猜测。

## 17.5 执行与恢复

一次 turn 依次执行：以 `AgentCommit` CAS 把 mailbox 队首从 `Pending` claim 为活动输入、
准备 kernel、注入 runtime 元数据与协作工具、执行 `TurnEngine`、持久化有序 trace、在首个
包含该输入的 session checkpoint 将其标记为 `Consumed`、原子提交 usage/outcome/session
event、回到 Idle，并仅在仍存在 `StartIfIdle` 输入时启动队首。enqueue、claim、checkpoint
与消费确认都使用同一 revision CAS；首 checkpoint 前崩溃时 claim 回到 Pending，checkpoint
已经包含该消息时恢复流程只确认消费而不再次注入，从而实现模型上下文 effectively-once。
传输层可按 `mail_id` 重试。

普通取消先递增 `dispatch_generation`，再清理当前 turn 的 pending steer、approval/input
waiter 与执行任务，保留 session mailbox；旧 queued start、defer 和 completion 因 generation
不匹配自动失效。取消先触发 token，默认等待 500ms，再 abort；携带旧 turn id、revision 或
generation 的迟到 completion 不得覆盖新状态。运行时随后只根据 canonical mailbox 中的
`StartIfIdle` 输入决定是否重新调度，不能沿用上一 turn 的粘性 run-queue 标志。durable
session event 必须先通过 repository CAS 提交，再由
`SessionEventHub` 广播，最后交给 `AgentCommitObserver`；close/shutdown 必须等 worker 确认停止，
随后 drain trace 并提交终态，
不得在 abort 与 worker 真正停止之间丢失尾部事件。

重启时宿主先恢复容器/worktree 等资源，再恢复 actors、sessions 和 durable mailbox。
遗留 Running turn 收束为 `Cancelled(runtime_restarted)`；资源 ready 后只重放 Pending
mailbox，旧的非持久化 queued start 不迁移，而是按 canonical agent/task snapshot 重建。
direct-child subscription、未消费 live update 和 timer 不单独持久化，而是从 durable
`parent_id`、snapshot、typed wake pending input 与 accepted wake receipts 重建。receipt
同时保存 wake id 和组成批次的 durable signal ids；即使原 wake 已离开 FIFO 或正在恢复的
批次边界不同，同一产品事实也不会产生第二个 turn。恢复中的 `WaitingAgents` 会重新挂载
timeout；已有 queued/running Planner 不重复激活。

Studio session owner 只从 durable `sessions.owner_agent_id` 恢复。宿主对 session 提交输入、
取消 turn、记录外部 fact 或取消重启前 transient interaction 时，必须使用该 canonical
owner；只有 `session_kind = root` 的会话才允许使用 `studio:{sessionId}` root identity。
repository 恢复 actors 前可以清除严格可证明的 ghost registration：该 agent 没有任何
canonical owned session，且其全部 runtime session claim 都与 durable owner 冲突。混合
ownership、缺少足够证据或共享 projection 的情况必须拒绝恢复，不能靠覆盖 snapshot 或
猜测 owner 继续启动。

`request_user_input` 的 callback/waiter 是进程内资源，durable interaction record 才是重启
事实源。重启不恢复旧 waiter，也不把答案伪装成普通用户 prompt：pending `userInput` 继续
展示；回答时若 origin turn 已终止，宿主使用 interaction id 作为幂等 mailbox id，提交
`SyntheticHidden` typed continuation。重启前 pending `toolApproval` 一律拒绝，因为无法仅凭
checkpoint 证明外部工具尚未产生副作用。对旧运行时已经取消的 restart user input，只允许
在 turn 明确为 `Cancelled(runtime_restarted)`、没有更新 pending 请求且尚无 recovery receipt
时恢复最新一条。

Studio observer attach 后还会修复历史 Plan 投影缺口：以最新完整 Plan trace 为唯一内容证据，
仅在确认 interaction 缺失、没有活动 TaskRun、且该 plan 尚未进入实施或终态时补建 durable
plan lifecycle 与 confirmation。该步骤不重放模型 turn，也不修改用户项目资源。

## 17.6 策略与协作

`AgentExecutionPolicy` 数据化描述可见工具、允许 effect、协作目标和 turn finalizer。
`AgentAccessPolicy` 明确可 spawn 的动态角色，以及 list/send/close 的目标选择：
`None | Tree | Explicit | All`。协作工具通过 `AgentRuntimeHandle` 调用 runtime；框架不依据
产品角色、Simple/Task 模式或工具名前缀做授权。

`AgentRegistration` 使用 `AgentWakePolicy::{RuntimeTerminal, ProductGated}`。普通 explorer
可由 runtime terminal 唤醒父代理；managed executor/reviewer 的 runtime terminal 只报告底层
状态，必须等产品 durable delivery/review/merge/recovery signal 才形成可执行 wake。父代理
`Running/Queued` 时只合并更新不抢占；无更新但仍有 live direct child 时进入
`WaitingAgents`。它统一拥有 direct-child 状态订阅、等待注册和每个 child 唯一的 30 秒
inactivity deadline；executor 与产品 coordinator 不得各自实现订阅等待循环。commentary、
tool activity、todo 与普通 progress 只更新最近进展并重置 deadline，不形成 actionable wake。
只有 `NeedsAttention`、真实 runtime terminal，以及 ProductGated 的 durable
delivery/review/merge/recovery phase 才能唤醒父 agent。30 秒无活动只生成一次
`InactivityDiagnostic`，携带最新 canonical snapshot，但自身不授权 interrupt 或 stop；只有
新活动或 Planner 明确重新等待后才重新计时。channel lag 时从 canonical durable snapshot
对账并清除过期 wake-in-flight，不把丢失的 commentary 推断成失败。

`InactivityDiagnostic` 不是提示词约定，而是 typed turn purpose。Core 根据 wake context
收窄 `AgentExecutionPolicy`：仍处于 `Queued | Running | WaitingTool |
WaitingInteraction` 且具有 active turn 或可启动 pending turn 的超时 child 是受保护目标，
`send_input`/`close_agent` 必须同时在工具 schema 和 dispatch 授权中排除这些目标。纯诊断轮
不得 spawn、`task_stop` 或执行其他任务控制工具；`list_agents` 仍可读取 canonical snapshot。
含真实 actionable update 的混合批次可保留对应产品操作，但不能解除健康 child 的目标保护。
诊断轮正常结束后若仍有 live child，父 agent 重新进入 `WaitingAgents` 并从当时重置完整
inactivity deadline；其间到达的 actionable update 保留到下一 continuation。非 synthetic
用户输入到达活动诊断轮时必须以普通策略启动新 turn，而不能 steer 进受限诊断策略；用户显式
`interrupt=true` 的合法中断流程不受影响。

每个 wake 使用 typed `WakeContext`，固定包含当前 agent 状态、wake reason、最后活动时间、
最近最多 8 个重要 progress/tool 阶段、一份最新 commentary 摘要、终态事实、用户停止请求、
signal revision 与 lag reconciliation 结果。completion watcher 只观察终态并写入
`trigger=false` 结果通知；仅当父级有等待注册时，`WaitingAgents` 才以 accepted wake receipt
原子生成 `trigger=true` continuation。未订阅的完成通知不得启动父 agent。
如果父 turn 通过 execution policy 要求的 finalizer 工具成功完成，`TurnFinished` 必须携带
该 finalizer，并把执行期间已缓冲的 child signal ids 持久化为 accepted wake receipt；该
finalizer 是当前阶段的消费屏障，旧信号不能再排入续轮，屏障之后的新事实仍按正常规则处理。

spawn/close 采用 prepare、durable transition、activate/commit、失败逆序补偿的 saga。
补偿无法完成时保留诊断事实并把 agent 置为 `Faulted`。

## 17.7 配置边界

`pl-core` 只提供 serde 值对象 `AgentModelConfig`、`ProviderConfig`、`ModelRouteConfig`、
`ProviderId`、`AgentRoleId` 与 resolve/validate。角色是动态字符串映射，模型只由 route 选择；
provider 不含第二份 `default_model`。

配置文件位置、schema version、产品默认角色、TOML/JSON 读写属于宿主。Studio 组合
`StudioConfig`，mai 组合 `MaiConfig`，然后调用 `models.validate()`。

## 17.8 Studio 边界

`pl-studio-runtime` 拥有 Studio 配置、SQLite repository 适配、product event、
project/session/task/worktree、Simple/Task 策略和 Studio-only wire DTO。session timeline
事件直接消费 `pl-protocol` 公共类型，不在 Studio 重做 trace mapping。`pl-studio-bridge`
只依赖该 crate。Studio 配置与数据库独立演进；当前数据库版本为 7。受支持的旧数据库通过
带备份的事务迁移升级；未版本化且已有用户表的 legacy 数据库不做结构兼容，而是归档原库后
重建当前 schema。未来版本明确拒绝打开，任何迁移失败都不得删除或降级原数据库。

## 17.9 Session 订阅不变量

- 一个 session 使用独立 durable sequence 和独立 bounded broadcast channel。
- subscription 必须先注册 receiver，再建立 snapshot/replay bootstrap。
- durable event 只在 transaction `Applied` 后广播；transient delta 只在 actor 校验 active
  turn 与 part revision 后广播。
- 下一 durable sequence 只从 hub canonical snapshot 的 `through_sequence` 分配；恢复时
  自动修复落后的 repository checkpoint，owner 冲突则拒绝挂载。
- transient delta 不进入 journal；terminal snapshot 必须包含完整最终内容。
- channel lag、cursor 超出保留窗口或 delta revision 缺口返回 `ResyncRequired`。
- 产品 UI 只订阅当前可见 session；项目、设置和资源变化使用独立 product stream。
