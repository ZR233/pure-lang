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

每个 agent actor 独占 session 集合、FIFO 输入队列、活动 turn、取消句柄、revision 和最近
结果。产品不得维护第二套 active-turn、queue 或 cancel 状态。

## 17.3 Host 端口

`AgentRuntimeHost` 组合四个窄端口：

- `AgentStateRepository`：恢复 runtime，并以 `AgentCommit` 原子提交 snapshot、session、
  turn、队列、usage 与 durable event；所有提交使用 expected revision 做 CAS。
- `AgentTurnFactory`：根据产品上下文准备 `PreparedAgentTurn`，不启动任务或修改状态。
- `AgentLifecycleAdapter`：以可回滚 lease 实现 spawn/close 外部资源 saga，操作必须幂等。
- `AgentEventSink`：只广播已经提交的事件；广播失败由实现内部记录，不回滚 durable 状态。

异步端口统一使用原生 RPITIT 并显式返回 `impl Future + Send`，不使用 `async_trait` 或
trait object。

## 17.4 状态与命令

状态正交拆分：

- lifecycle：`Active | Closing | Closed | Faulted`
- activity：`Idle | Queued | Running | WaitingTool | WaitingInteraction`
- turn outcome：`Completed | Cancelled | Failed | BudgetLimited`

turn 完成后回到 `Active + Idle`。turn 失败是结果而非 agent 生命周期失败；只有持久化
终态失败或无法补偿的不变量破坏会进入 `Faulted`。

`AgentRuntimeHandle` 提供 `register`、`submit`、`cancel_turn`、`close`、`snapshot`、
`list`、`wait` 和 `shutdown`。输入投递明确使用 `QueueOnly | Start | InterruptThenStart`。
未关闭 agent 可连续接收输入，不存在 `resume_agent`。

## 17.5 执行与恢复

一次 turn 依次执行：提交队列/活动状态、准备 kernel、注入 runtime 元数据与协作工具、
执行 `TurnEngine`、持久化有序 trace、原子提交 session/usage/outcome/event、回到 Idle、
启动队首输入。取消先触发 token，默认等待 500ms，再 abort；携带旧 turn id 或 revision 的
迟到 completion 不得覆盖新状态。trace 批次必须先通过 repository CAS 提交，再交给
`AgentEventSink` 广播；close/shutdown 必须等 worker 确认停止，随后 drain trace 并提交终态，
不得在 abort 与 worker 真正停止之间丢失尾部事件。

重启时宿主先恢复容器/worktree 等资源，再恢复 actors、sessions 和 pending inputs。
遗留 Running turn 收束为 `Cancelled(runtime_restarted)`；资源 ready 后按 FIFO 重放队列。

## 17.6 策略与协作

`AgentExecutionPolicy` 数据化描述可见工具、允许 effect、协作目标和 turn finalizer。
`AgentAccessPolicy` 明确可 spawn 的动态角色，以及 list/send/wait/close 的目标选择：
`None | Tree | Explicit | All`。协作工具通过 `AgentRuntimeHandle` 调用 runtime；框架不依据
产品角色、Simple/Task 模式或工具名前缀做授权。

spawn/close 采用 prepare、durable transition、activate/commit、失败逆序补偿的 saga。
补偿无法完成时保留诊断事实并把 agent 置为 `Faulted`。

## 17.7 配置边界

`pl-core` 只提供 serde 值对象 `AgentModelConfig`、`ProviderConfig`、`ModelRouteConfig`、
`ProviderId`、`AgentRoleId` 与 resolve/validate。角色是动态字符串映射，模型只由 route 选择；
provider 不含第二份 `default_model`。

配置文件位置、schema version、产品默认角色、TOML/JSON 读写属于宿主。Studio 组合
`StudioConfig`，mai 组合 `MaiConfig`，然后调用 `models.validate()`。

## 17.8 Studio 边界

`pl-studio-runtime` 拥有 Studio 配置、SQLite、事件投影、project/session/task/worktree、
Simple/Task 策略和 wire DTO。`pl-studio-bridge` 只依赖该 crate。Studio 配置版本为 5，
数据库和配置遇到旧版本直接重建，不保留迁移或 fallback。
