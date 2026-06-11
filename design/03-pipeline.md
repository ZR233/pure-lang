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
- `turnOptions.permissionMode`：默认固定 `request-approval`
- `prompt`、`sessionId`、`workspaceRoot` 等进入 application service

`compileMode` 是会话级协作模式。`auto` 是默认执行模式，允许模型在审批策略约束内主动修改工作区；`plan` 是 Codex 风格规划模式，允许读取、搜索、运行经审批的探索命令和调度探索型子代理，但最终交付物应是一段可执行计划，而不是直接修改文件。Plan Mode 的最终计划使用 `<proposed_plan>...</proposed_plan>` 包裹，由 streaming 层提取为独立 timeline item；普通 assistant 正文不显示这些标签。

`workspaceRoot` 是运行期有效工作区，而不是简单等于 UI 当前选中的目录。Studio 读取 project path 后先解析到规范化目录；如果该目录位于 Git 仓库中，则提升到最近的 Git 仓库根。这样用户从子 crate 或桌面壳层进入项目时，工具仍能访问完整仓库上下文。工作区记忆按 Codex 风格从 Git 根到当前工作目录读取层级文档，候选文件优先级为 `AGENTS.override.md`、`AGENTS.md`、`Agents.md`，并受配置的总字节预算限制。

提示词在核心层按三层组装：`base/system`、`developer`、`user context`。`base/system` 是模型请求的顶层系统提示词；`developer` 承载 Auto/Plan 模式、skills 索引和运行约束；`user context` 承载用户配置的上下文偏好和 AGENTS 项目记忆。这些临时前置内容参与模型请求和 token 估算，但不写入普通会话消息历史。

当启用 skills 时，运行时从 `<workspaceRoot>/skills/`、`~/.pure/skills/`、`~/.pure/skills/.system/` 和配置外部目录发现 skills，并在 developer 层注入简短索引。项目 skills 优先于用户、系统和外部只读 skills；自学习写入始终落到 `<workspaceRoot>/skills/`。

策略约束：

- 方案乙不保留旧命令别名和旧字段兜底
- `PermissionMode::RequestApproval` 为默认且主路径；旧 `ToolApprovalPolicy` 仅作为兼容构造
- 手动审批接口保留在系统能力中，但不作为默认流程；`request-approval` 模式只在工具请求 workspace 外访问时弹出用户审批，workspace 内读写直接放行
- `auto-review` 模式使用 reviewer 角色模型审批 workspace 外访问。reviewer 只返回批准或拒绝，不执行工具；解析失败、provider 失败或非明确批准均按拒绝处理
- `full-access` 放宽 Pure 文件工具和 `bash.workingDirectory` 的 workspace 边界，并在策略层直接放行 Plan Mode 中已暴露的 `bash`；该模式仍只执行已注册工具，不提供 OS 沙箱或系统级提权
- Studio 中的 Plan Mode 会保留 `bash` 探索能力；`request-approval` 与 `auto-review` 下 bash 必须走手动审批，`full-access` 下直接放行。明确写入类工具不会暴露给模型，也不能执行模型幻觉出的写入工具调用
- 用户显式要求 `subagent`/子代理分工时，核心提示必须将异步 agent 调度作为强约束；普通 shell 或文件探索不能替代子代理调度
- 显式子代理分工允许最多两轮只读定位；若仍未创建 agent，后续推理只暴露 `spawn_agent` 并保持 `auto` tool choice，避免触发不支持 required tool choice 的 provider 限制
- 多 agent 协作只通过 `spawn_agent`、`wait_agent`、`list_agents`、`send_message`、`followup_task`、`close_agent` 组成，不提供同步等待到最终摘要的 `subagent` 入口
- `request_user_input` 是 Codex 风格的瞬时交互工具，root agent 与 subagent 都可用；工具通过结构化问题请求用户输入，等待前端回答后把答案作为工具结果返回，不作为普通用户聊天消息写入历史
- agent 运行时状态对齐 Codex：`queued | running | waiting | completed | errored | interrupted | shutdown | notFound`
- `budgetLimited` 不是 agent 状态，而是 turn abort reason；子 agent 预算耗尽时状态为 `interrupted`，并携带 `reason`、`budgetLimitKind` 和 `budgetUsage`
- `interrupted` 是可恢复的非终局状态；`completed | errored | shutdown | notFound` 是终局状态
- `send_message` / `followup_task` 不能重新激活终局 agent；`interrupted` agent 可以通过 followup 恢复为 `queued`
- 父 agent 因中断、错误、预算限制或关闭而停止时，必须级联关闭仍在运行的子树，避免后台子 agent 残留为 `running`

## 3.3 核心 turn 编排

`StudioRuntime` 只做 use case 编排：

1. 读取 session/project/config
2. 解析或创建 session 级提示词快照
3. 构造 `TurnRequest` 与 `TurnOptions`
4. 组装 `PureCore`（含工具注册）
5. 执行 `run_turn_with_trace`
6. 事务化批量落库：message + trace + runtime snapshot
7. 输出命令响应 DTO 与 timeline DTO

`run_turn_with_trace` 在每次模型请求前用 `InstructionAssembler` 解析当前提示词快照。base/system 写入 `CompletionRequest.instructions`；developer 块作为临时 system 消息置于历史消息之前；user context 块作为临时 user 消息置于 developer 块之后、真实历史之前。临时前置消息只用于本次 provider request，不写入 `CoreSession`，因此压缩和持久化只处理真实对话历史。

`run_turn_with_trace` 在每次模型请求前执行自动上下文压缩检查。压缩阈值来自当前模型的 `autoCompactTokenLimit`，未配置时使用有效上下文窗口的 90%；模型没有上下文窗口信息时不触发自动压缩。压缩估算包含 base/system、developer、user context 和真实消息历史。压缩由 `pl-core` 本地摘要完成：用当前模型和固定 compact prompt 生成 handoff summary，再用一条带 metadata 的用户摘要消息加最近真实用户消息替换原始历史。工具调用、工具结果和 assistant 中间过程不以原始片段保留，避免压缩后出现破碎的 tool-call 配对。

子代理没有独立的压缩实现。`spawn_agent` 和 `followup_task` 创建的 child session 复用同一个 `PureCore` turn pipeline，因此每个子代理独立维护自己的压缩历史；父会话不会替子代理压缩，也不会因为子代理压缩而改写父历史。

子代理同样继承父 turn 的 `compileMode` 和 resolved instruction snapshot。父会话处于 Plan Mode 时，child session 也以 Plan Mode 运行，并复用同一套工具边界和 proposed-plan 输出约定；父会话处于 Auto Mode 时，child session 按 Auto Mode 执行。

子代理同样继承父 turn 的用户交互回调。`request_user_input` 在 root 或 child agent 中被调用时，核心层广播 `UserInputRequested`，Studio 用底部回答 UI 替换普通输入框并把回答发送回原工具调用；回答完成后广播 `UserInputAnswered`。该交互只解除当前工具等待，不触发新 turn，也不进入 agent timeline。

主 turn 保存完成后，如果 `[skills].auto_learn = true` 且本轮达到自学习触发条件，`StudioRuntime` 启动后台 reviewer。reviewer 只开放 skills 工具，复盘结果只写项目 skills 目录；失败只记录日志，不改变本轮响应。

文件与 shell 工具都以有效 `workspaceRoot` 为默认边界。`bash` 默认在 workspace root 下执行，`workingDirectory` 也按 workspace root 解析并拒绝逃逸；文件工具默认只允许访问 workspace root 内的路径。`full-access` 模式会放宽该边界，允许文件路径和 `bash.workingDirectory` 指向 workspace 外，但不绕过工具自身校验、写锁、超时、输出截断和 timeline 记录。

Skills 管理工具同样以 `workspaceRoot` 为边界，但写入面收窄到 `<workspaceRoot>/<skills.project_dir>/`。用户级、系统和外部 skills 只读参与发现，不允许被工具原地修改。

工具预算与收尾原则：

- 工具调用或 provider 返回 `end_turn = false` 只表示 `needsFollowUp`，不是完成条件
- root turn 和 child agent 默认只强制 `wallClockMs = 1800000`
- 模型采样、普通工具调用和 `wait_agent` 调用只记录 `modelSteps`、`toolCalls`、`waitCalls` 观测计数，不触发 step/tool/wait 限制
- `request_user_input` 等待用户期间仍受当前 turn 的 cancellation token 和 wall-clock 预算约束；用户停止时 pending 输入被取消并返回空回答
- agent tree 默认限制为 `maxAgents = 16`、`maxDepth = 3`
- 预算耗尽属于 `TurnAborted(reason=budgetLimited)`，必须写入 `TurnBudgetLimited` trace，不得伪装为 `failed` 或 `completed`
- wall-clock 预算耗尽时核心层按 `budgetLimited` 收尾，并在 trace 中保留预算用量
- 无工具总结或普通 assistant 文本中若出现未执行的工具调用标记，必须按 `budgetLimited` 收尾并写入 `TurnBudgetLimited`；不能把原始 tool-call 文本作为最终回答
- 用户显式要求子代理分工时，turn 完成前必须验证本轮实际创建了 agent；否则按 `failed` 收尾，不写入伪完成 assistant 消息

Agent 协作 timeline 与状态分层：

- agent timeline 是 append-only 协作事件流，只记录 spawn、wait、message、followup、close、final status 等事实事件
- agent tree 是 latest snapshot，只按 `agent_id/path` 覆盖最新状态，供状态栏、树视图和 `list_agents` 使用
- 前端不得用 latest snapshot 渲染 timeline；同一个 agent 的多次状态变化必须在 timeline 中保留为多条独立事件
- `AgentStateChanged` 只用于更新 latest snapshot；UI timeline 消费 `agentEvents` 中的 append-only event

持久化原则：

- 消息和 trace 采用事务批量写入，避免逐条写放大
- session 的 `mode` 表示下一轮默认协作模式，由 Studio 模式切换命令持久化；运行时按 session 当前 `mode` 构造 `TurnRequest`
- session 的 `instruction_snapshot_json` 保存首轮解析出的 base/developer/user context。已有 session 缺少快照时，在下一轮运行前按当前配置补建。后续配置、模型默认提示词或 AGENTS 文件变化不 retroactively 改写既有 session；新 session 才使用新配置。
- Plan Mode 生成的计划有独立生命周期事件：`accepted | implementing | implemented | implementationFailed | dismissed`。这些事件作为 `TraceEventKind::PlanLifecycleChanged` 追加到 `timeline_events`，不单独建表；前端按 `planId` 折叠最新状态
- 如果 turn 内发生上下文压缩，`CoreSession` revision 会变化，Studio 以事务重写当前 session 的消息历史并追加本轮 trace；未发生压缩时继续使用追加写入
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
