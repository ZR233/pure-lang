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

## Studio 展示

Studio timeline 以 item-first 数据为准。工具 entry 和工具组详情必须显示工具名称、状态、关键路径或命令摘要。静默文件工具的成功结果可以隐藏；但失败、拒绝、中断和预算受限时必须展示 result/error 详情，避免用户只看到“工具调用失败”而无法定位原因。
