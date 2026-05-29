# 08 - Timeline 流式事件

## 8.1 统一 Timeline 层

`AgentEvent` 定义在 `pl-protocol`，是系统统一输出通道。Studio 主界面只消费 item-first timeline 事件，不再消费旧的文本、思考或工具 delta。

实时 timeline 协议固定为：

- `TimelineItemStarted`
- `TimelineItemDelta`
- `TimelineItemCompleted`
- `TimelineItemFailed`

timeline item 类型固定为：

- `text`
- `thinking`
- `tool`
- `agent`
- `turn`
- `inference`

每个 item 必须携带 `turnId`、`itemId`、`sequence`、`createdAt`、`updatedAt` 和 `status`。`sequence` 是会话内单调递增顺序号，item 的展示顺序以首次创建时的 `sequence` 为准；后续 delta 和 completed/failed 事件只 upsert 同一个 item。

## 8.2 数据流

```text
pl-model provider
  → item-aware stream accumulator
  → AgentEventSender
  → pl-core TimelineRecorder 汇总 TurnResult
  → pure-studio 实时 upsert
```

`pure-studio` 订阅同一 `AgentEventReceiver`，并通过 Tauri event 把事件转换为 React reducer action：

- `TimelineItemStarted` 插入 item 并记录首次顺序。
- `TimelineItemDelta` 按 `itemId` 追加文本、思考 chunk 或工具参数。
- `TimelineItemCompleted` 用最终 snapshot 覆盖同一 item。
- `TimelineItemFailed` 标记同一 item 失败并保留错误。
- `AgentStateChanged` 展示 agent 路径、状态、角色、任务摘要、最终摘要或错误。
- `Error` 渲染为可恢复或致命错误提示。

`TextDelta`、`ThinkingDelta`、`ToolCallDelta`、`ToolCallComplete` 不再是 Studio 的协议或兼容入口。

## 8.3 背压与容量

事件通道使用 `tokio::sync::broadcast`。默认容量由调用方创建，目前建议为 `256`。

高频 delta 允许通过 broadcast 丢失实时帧，但 turn 最终的 timeline event 集合必须随消息一起批量落库。`TimelineItemCompleted` 携带最终 snapshot，历史加载不依赖实时 delta 是否完整到达前端。

## 8.4 事件边界

事件类型属于协议层，不应包含 provider 私有结构，也不应绑定具体前端。工具审批事件只承载通用工具名、参数和审批结果，不包含 Tauri、React 或桌面端私有状态。

子代理内部事件不直接转发完整文本流、思考流、工具调用流或工具输出。`pl-core` 将子代理生命周期压缩为 `agent` timeline item 和 `AgentStateChanged` snapshot，状态固定为 `queued`、`running`、`waiting`、`completed`、`errored`、`interrupted`、`shutdown`、`notFound`。`pure-studio` 持久化这些状态事件，并在聊天界面只渲染路径、状态和摘要，避免把子代理内部执行细节混入父会话 timeline。

## 8.5 流式工具调用聚合与 ID

`pl-model` 负责把 provider 的工具调用 delta 聚合为完整的 `ToolCall` 后再交给 `pl-core` 执行。Chat Completions 流式响应中的后续参数片段可能只带 `index`，不再重复 `id` 或 `name`；Responses API 的 custom/freeform 输入 delta 也可能只带 `item_id` / `call_id`。因此聚合层必须使用稳定的流式序号或 item/call id 合并片段，并保留最早出现的 provider id、工具名和调用种类。

工具 timeline item 的 `itemId` 使用 `toolCallId`。当 provider 提供 `call_id` 时，`toolCallId` 优先使用该值；provider 的原始 item id 只作为聚合辅助信息保留在内部。工具参数流、审批、执行和结果都 upsert 到同一个 tool item。

聚合完成前不得把缺少工具名的参数片段当作新的工具调用执行。只有在 `output_item.done` 缺失时，才允许用已聚合的 delta 兜底生成工具调用；该兜底调用仍必须带有前面片段提供的真实工具名和稳定 `toolCallId`。

如果 provider 把工具调用以正文形式返回，例如 DSML/tool-call 标记或 JSON `tool_calls` 文本，`pl-core` 不得把它作为 assistant 最终消息流给主 chat。该情况属于模型未产出可执行工具调用，turn 应以 `failed` 收尾并触发 `Error` + `Done`。

## 8.6 工具并行

`CompletionRequest.parallel_tool_calls` 随模型能力和 `TurnOptions.tool_execution_mode` 决定，不再硬编码关闭。

`pl-core` 对模型一次返回的工具调用使用 Codex 风格调度：

- 支持并行的工具可同时执行。
- 不支持并行的工具通过独占锁与其他工具互斥。
- 写文件、patch、delete、move、shell 等可能产生副作用的工具默认不并行。
- 只读文件、搜索、stat、list、spawn/wait/list agent 等工具可以按风险显式 opt-in。
- 工具结果写回 `CoreSession` 必须保持模型发出工具调用的顺序。

## 8.7 Usage 与状态栏

`pl-model::TokenUsage` 保留输入、输出和总 token，并额外记录 `cached_prompt_tokens`。Chat Completions 和 Responses API 的 usage detail 字段不同，provider 适配层负责尽可能读取缓存 token；缺失时按 `0` 处理。

Studio 状态栏必须在运行中即时反映上下文和费用。流式事件里的 `inference` item 在完成时携带本次模型调用 usage；前端 reducer 用当前模型价格对 `sessionRuntime` 做增量更新，展示最新上下文、累计输入/输出、缓存命中和费用估算。turn 完成后后端仍返回持久化后的 `sessionRuntime` 快照作为最终校准。

`pl-core` 不把费用估算作为 provider 事件暴露；费用只由 Studio 结合模型价格配置估算。React 状态栏消费运行态快照和 inference usage，不直接解析 provider 私有 usage 字段。
