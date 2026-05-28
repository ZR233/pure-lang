# 03 - 编译流程（方案乙）

## 3.1 总览

方案乙将流程明确分成“桥接层 -> 应用层 -> 领域核心 -> 适配器”：

```text
React action
  -> Tauri commands
  -> pl-core application service (StudioRuntime)
  -> interfaces ports
  -> infrastructure adapters (sqlite/config/fs/event/tool)
  -> PureCore turn pipeline
  -> AgentEvent / TraceEvent
  -> Tauri event
  -> reducer action
  -> UI rendering
```

`main.rs` 不承载流程逻辑，只负责注册。

## 3.2 输入与策略

运行输入统一为新 DTO 契约（camelCase wire）。

- `compileMode`：`plan | auto`
- `turnOptions.toolApprovalPolicy`：默认固定 `autoAllow`
- `prompt`、`sessionId`、`workspaceRoot` 等进入 application service

`workspaceRoot` 是运行期有效工作区，而不是简单等于 UI 当前选中的目录。Studio 读取 project path 后先解析到规范化目录；如果该目录位于 Git 仓库中，则提升到最近的 Git 仓库根。这样用户从子 crate 或桌面壳层进入项目时，工具仍能访问完整仓库上下文。工作区记忆优先读取 `AGENTS.md`，兼容读取 `Agents.md`。

策略约束：

- 方案乙不保留旧命令别名和旧字段兜底
- `ToolApprovalPolicy::AutoAllow` 为默认且主路径
- 手动审批接口保留在系统能力中，但不作为默认流程
- 用户显式要求 `subagent`/子代理分工时，核心提示必须将 `subagent` 作为强约束；普通 shell 或文件探索不能替代子代理调度
- 显式子代理分工允许最多两轮只读定位；若仍未创建 agent，后续推理只暴露 `spawn_agent` / `subagent` 并保持 `auto` tool choice，避免触发不支持 required tool choice 的 provider 限制
- 多 agent 协作通过 `spawn_agent`、`wait_agent`、`list_agents`、`send_message`、`followup_task`、`close_agent` 组成；`subagent` 仅是同步便捷入口，底层创建 managed agent 并等待结果
- agent 运行时状态对齐 Codex：`queued | running | waiting | completed | errored | interrupted | shutdown | notFound`
- `budgetLimited` 不是 agent 状态，而是 turn abort reason；子 agent 预算耗尽时状态为 `interrupted`，并携带 `reason`、`budgetLimitKind` 和 `budgetUsage`
- `interrupted` 是可恢复的非终局状态；`completed | errored | shutdown | notFound` 是终局状态
- 父 agent 因中断、错误、预算限制或关闭而停止时，必须级联关闭仍在运行的子树，避免后台子 agent 残留为 `running`

## 3.3 核心 turn 编排

`StudioRuntime` 只做 use case 编排：

1. 读取 session/project/config
2. 构造 `TurnRequest` 与 `TurnOptions`
3. 组装 `PureCore`（含工具注册）
4. 执行 `run_turn_with_trace`
5. 事务化批量落库：message + trace + runtime snapshot
6. 输出命令响应 DTO 与 timeline DTO

文件与 shell 工具都以有效 `workspaceRoot` 为边界。`bash` 默认在 workspace root 下执行，`workingDirectory` 也按 workspace root 解析并拒绝逃逸；文件工具继续只允许访问 workspace root 内的路径。

工具预算与收尾原则：

- 工具调用或 provider 返回 `end_turn = false` 只表示 `needsFollowUp`，不是完成条件
- root turn 默认预算为 `modelStepBudget = 32`、`toolCallBudget = 120`、`waitBudget = 16`、`wallClockMs = 180000`
- child agent 默认预算为 `modelStepBudget = 24`、`toolCallBudget = 80`、`waitBudget = 12`、`wallClockMs = 120000`
- `wait_agent` 消耗 wait 预算；其他工具消耗 tool call 预算；模型采样消耗 model step 预算
- agent tree 默认限制为 `maxAgents = 16`、`maxDepth = 3`
- 预算耗尽属于 `TurnAborted(reason=budgetLimited)`，必须写入 `TurnBudgetLimited` trace，不得伪装为 `failed` 或 `completed`
- 预算耗尽后核心层保留一次 `tool_choice = "none"` 的无工具收尾采样；该采样不消耗普通工具预算
- 无工具总结仍未返回内容时，核心层返回明确兜底文本，不能静默 `completed`
- 无工具总结或普通 assistant 文本中若出现未执行的工具调用标记，必须按 `budgetLimited` 收尾并写入 `TurnBudgetLimited`；不能把原始 tool-call 文本作为最终回答
- 用户显式要求子代理分工时，turn 完成前必须验证本轮实际创建了 agent；否则按 `failed` 收尾，不写入伪完成 assistant 消息

Agent 协作 timeline 与状态分层：

- agent timeline 是 append-only 协作事件流，只记录 spawn、wait、message、followup、close、final status 等事实事件
- agent tree 是 latest snapshot，只按 `agent_id/path` 覆盖最新状态，供状态栏、树视图和 `list_agents` 使用
- 前端不得用 latest snapshot 渲染 timeline；同一个 agent 的多次状态变化必须在 timeline 中保留为多条独立事件
- `AgentStateChanged` 只用于更新 latest snapshot；UI timeline 消费 `agentEvents` 中的 append-only event

持久化原则：

- 消息和 trace 采用事务批量写入，避免逐条写放大
- timeline 读取以 `sequence` 为单调游标
- agent tree、agent events、agent messages 与 turn snapshot 分表持久化；`agents` 为 latest snapshot，`agent_events` 为 append-only event log

## 3.4 事件管线

`drain_events` 使用显式分支处理广播通道状态：

- `Ok(event)`：正常转发
- `Err(Lagged(n))`：记录丢帧指标并继续 drain，不退出
- `Err(Closed)`：结束循环

这保证高频 delta 下 UI 不会因为 lagged 直接断流。

`Done`、turn final、agent final 属于 lossless 事件：转发层必须确保它们不会因为普通 delta 的背压被丢弃。

## 3.5 Turn 收尾语义

turn 生命周期持久化语义固定：

- `started`
- `completed`
- `aborted`（具体原因见 `turnAbortReason`）
- `errored`

用户停止属于 `aborted(reason=interrupted)`，不可被延迟完成覆盖。
工具、模型或 agent 基座错误属于 `errored`，必须写入 `TurnFailed` trace。
工具、等待、模型采样或 agent tree 预算耗尽属于 `aborted(reason=budgetLimited)`，必须写入 `TurnBudgetLimited` trace。

## 3.6 输出模型

命令输出统一采用新 DTO：

- `bootstrapResponse`
- `projectSelectionResponse`
- `sessionSelectionResponse`
- `runPromptResponse`
- `sessionTimelineResponse`

前端 reducer 只消费 action 输入类型，不再由事件监听器直接拼装复杂 UI 状态。
