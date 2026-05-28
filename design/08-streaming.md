# 08 - 流式事件

## 8.1 统一事件层

`AgentEvent` 定义在 `pl-protocol`，是系统统一输出通道。

当前事件来源：

- `pl-core`：`TurnStarted`、`Done`、核心错误事件。
- `pl-model`：文本增量、思考增量、工具调用增量。
- `pl-model`：完成事件中的 token usage，包括 provider 能提供的缓存命中 token。

当前事件消费者：

- `pure-studio`
- 测试
- 后续日志、审计和 UI 层

## 8.2 数据流

```text
pl-model provider
  → AgentEventSender
  → pl-core 汇总 TurnResult
  → pure-studio 实时渲染
```

`pure-studio` 必须订阅同一 `AgentEventReceiver`，并通过 Tauri event 把事件转换为 React 前端应用状态：

- `TextDelta` 追加到当前 assistant 消息。
- `ThinkingDelta` 追加到当前思考区域。
- `ToolCallDelta` / `ToolCallComplete` 展示工具调用状态。
- `ToolApprovalRequested` / `ToolApprovalGranted` / `ToolApprovalDenied` 展示并记录审批流程。
- `AgentStateChanged` 展示 agent 路径、状态、角色、任务摘要、最终摘要或错误。
- `Error` 渲染为可恢复或致命错误提示。

## 8.3 背压与容量

事件通道使用 `tokio::sync::broadcast`。默认容量由调用方创建，目前建议为 `256`。

未来如果需要多消费者，应保持同一事件源，避免为 GUI、日志和测试分别设计不同输出通道。

## 8.4 事件边界

事件类型属于协议层，不应包含 provider 私有结构，也不应绑定具体前端。工具审批事件只承载通用工具名、参数和审批结果，不包含 Tauri、React 或桌面端私有状态。

子代理内部事件不直接转发完整文本流。`pl-core` 将子代理生命周期压缩为 `AgentStateChanged`，状态固定为 `queued`、`running`、`waiting`、`completed`、`failed`、`interrupted`、`closed`。`pure-studio` 持久化这些状态事件，并在聊天界面渲染路径、状态和摘要。

## 8.5 流式工具调用聚合

`pl-model` 负责把 provider 的工具调用 delta 聚合为完整的 `ToolCall` 后再交给 `pl-core` 执行。Chat Completions 流式响应中的后续参数片段可能只带 `index`，不再重复 `id` 或 `name`；Responses API 的 custom/freeform 输入 delta 也可能只带 `item_id` / `call_id`。因此聚合层必须使用稳定的流式序号或 item/call id 合并片段，并保留最早出现的 provider id、工具名和调用种类。

聚合完成前不得把缺少工具名的参数片段当作新的工具调用执行。只有在 `output_item.done` 缺失时，才允许用已聚合的 delta 兜底生成工具调用；该兜底调用仍必须带有前面片段提供的真实工具名。

## 8.6 Usage 与状态栏

`pl-model::TokenUsage` 保留输入、输出和总 token，并额外记录 `cached_prompt_tokens`。Chat Completions 和 Responses API 的 usage detail 字段不同，provider 适配层负责尽可能读取缓存 token；缺失时按 `0` 处理。

`pl-core` 不把费用估算作为 provider 事件暴露，而是在 turn 完成后结合模型价格配置生成 Studio 会话运行态快照。React 状态栏消费快照，不直接推断 provider 私有 usage 字段。
