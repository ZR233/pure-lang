# 工具调用运行时

## 目标

工具调用运行时负责把模型返回的 tool call 转换为本地工具执行、审批、timeline 事件和下一轮模型输入。它必须保证同一个工具调用身份稳定、终态事件唯一、失败结果可回传给模型，并在 Studio 中保留用户可读的错误原因。

## 身份字段

`ToolCall.id` 是 provider 返回的工具调用 item id。Chat Completions 历史回放时，assistant 消息中的 `tool_calls[].id` 和后续 tool 消息的 `tool_call_id` 必须使用该值。

`ToolCall.call_id` 是 Responses API 的调用 id。Responses 历史回放时，`function_call_output` 和 `custom_tool_call_output` 的 `call_id` 必须优先使用该值；缺失时才回退到 `ToolCall.id`。

Core 会话中的 tool result metadata 同时保存两个字段：

- `tool_call_id`：写入 `ToolCall.id`，供 Chat Completions tool message 使用。
- `tool_call_call_id`：写入 `ToolCall.call_id`，供 Responses output item 使用。

Timeline 的工具 item id 使用可展示、可去重的运行时 id：优先取 `ToolCall.call_id`，否则取 `ToolCall.id`，再按 turn 做命名空间隔离。Timeline item 的 `tool.provider_item_id` 保存 `ToolCall.id`，`tool.call_id` 保存 `ToolCall.call_id`。

## 生命周期

每个工具调用先写入一个 `TimelineItemStarted`，表示模型已请求该工具。随后运行时执行以下流程：

1. 检查当前模式是否允许该工具。
2. 查找工具注册表。
3. 计算权限策略，必要时请求用户审批或 reviewer 审批。
4. 对批准的工具执行本地实现；对禁用、未知或拒绝的工具直接生成工具结果。
5. 在统一收尾阶段写入唯一终态事件。

终态事件只允许出现一次。`completed` 表示工具成功执行，`failed` 表示工具实现或注册失败，`denied` 表示模式、策略或审批拒绝，`interrupted` 和 `budgetLimited` 表示 turn 控制层中断或预算限制。`approved` 可作为执行前的非终态状态展示，但不能替代最终 `completed` 或 `failed`。

## 结果回传

工具结果进入模型上下文时仍使用字符串内容。失败结果必须包含稳定前缀和原始错误文本：

- 未知工具：`Unknown tool: {name}`
- 策略或用户拒绝：`Tool execution denied: {reason}`
- 本地执行错误：`Tool execution error: {error}`

这些结果必须作为 tool result 写入会话历史，即使工具被禁用、未知或拒绝。后续模型可以据此恢复、改用其他工具或向用户解释失败原因。

`apply_patch` 的解析或上下文匹配失败属于本地执行错误，仍使用 `Tool execution error: {error}` 前缀写回模型上下文。错误文本应包含可恢复提示：不要重复同一个失败 patch；先重新读取目标文件当前内容，再生成更小、更精确的 Codex 风格 patch 重试。成功前已提交的 hunk 必须在错误文本中列出 committed delta，方便后续模型只处理剩余改动。

`bash` 和 `write_stdin` 成功执行时，写回模型上下文的 result 是一个紧凑 JSON 字符串，而不是完整原始输出。字段包括：

- `status`：`running`、`completed`、`failed`、`timedOut` 或 `interrupted`。
- `processId`：后台进程 id；仅当命令仍可继续观察或写入时存在。
- `exitCode`：进程已退出时的退出码，无法取得时为 `null`。
- `timedOut`：是否因 `timeoutSeconds` 触发终止。
- `stdout` / `stderr`：按 `maxOutputChars` 或默认 head/tail 预算截断后的文本。
- `outputFile`：完整 stdout/stderr 文件路径。
- `message`：面向模型的下一步提示。

当 `bash` 在 `yieldTimeMs` 内未完成时，result 使用 `running` 状态并带 `processId`。后续模型必须用 `write_stdin` 携带该 `processId` 发送输入或传空 `chars` 轮询，不应重复执行同一条 `bash` 命令。需要完整输出时，模型应使用文件读取工具读取 `outputFile`，不要要求命令工具把大输出完整塞回上下文。`write_stdin` 找不到 live process、进程数量达到上限、stdin 写入失败或后台命令已被终止时，应返回可恢复错误，让模型等待、轮询或解释当前状态。

`wait_agent` 和 `list_agents` 默认只回传紧凑 agent 摘要，避免把完整 agent snapshot 反复写入模型上下文。调用方显式传入 `includeDetails: true` 时，工具结果可包含完整 `AgentRecord`，用于诊断；普通协作流程应优先依赖精简摘要和最终子代理总结。`spawn_agent.forkTurns` 的历史继承只复制过滤后的父会话消息，不复制工具结果、工具调用 metadata、reasoning 内容或运行时调度提示。

## Studio 展示

Studio timeline 以 item-first 数据为准。工具 entry 和工具组详情必须显示工具名称、状态、关键路径或命令摘要。静默文件工具的成功结果可以隐藏；但失败、拒绝、中断和预算受限时必须展示 result/error 详情，避免用户只看到“工具调用失败”而无法定位原因。
