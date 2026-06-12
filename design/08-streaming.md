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
- `plan`

每个 item 必须携带 `turnId`、`itemId`、`sequence`、`createdAt`、`updatedAt` 和 `status`。`sequence` 是会话内单调递增的 timeline event 顺序号；item 的展示顺序以首次创建时的 `sequence` 为准；后续 delta 和 completed/failed 事件只 upsert 同一个 item，并且不得改变该 item 的首次展示顺序。

每个 turn 被接收后，用户输入必须作为该 turn 的第一个可见 timeline item 记录和广播。enabled tools、`turn`、`inference` 等内部诊断或运行态 item 不得在 sequence 上排到用户输入之前，避免前端等待状态、内部状态或历史回放出现在用户问题上方。

历史加载和运行完成响应必须暴露 `nextSequence`/`timelineNextSequence` 作为事件游标。前端只能用该事件游标判断 snapshot 新旧，不能用 item 列表里的最大 `sequence` 代替游标。旧 snapshot 只能补齐缺失 item，不能覆盖已经通过实时事件或更新响应接收的新 turn 内容。该游标只由后端持久化 timeline 推进；前端 optimistic item 可以使用临时本地顺序参与展示，但不得预占或推进 `timelineNextSequence`。

## 8.2 数据流

```text
pl-model provider
  → async-openai stream
  → protocol stream event mapper
  → provider-independent stream accumulator
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

模型 provider 流的成功边界由 `StreamEvent::Completed` 明确表示。protocol mapper 可以把 provider 私有终止 chunk 转换为该事件；如果底层 SSE parse、transport 或 EOF 在 completed 之前发生，`pl-model` 必须返回错误，并由 turn 层发出 failed turn、`Error` 和 `Done`，不得把局部内容当作成功消息落库。completed 之后的 usage、文本、思考和工具调用 snapshot 才能进入最终 `CompletionResponse`。

Plan Mode 下模型输出的 `<proposed_plan>...</proposed_plan>` 块由 `pl-model::stream` accumulator 提取为 `plan` item。计划正文复用 `TimelineItem.content`，增量使用 `TimelineDelta::Plan`；同一块内容不得同时出现在普通 assistant `text` item 中。计划块之外的普通文本仍按 assistant `text` item 流式输出。

计划的采纳与实施状态不改变 `plan` item 本身，而是通过 `TraceEventKind::PlanLifecycleChanged` 追加到同一 session 的 `timeline_events`。事件包含 `planId`、`state`、可选 `turnId`、可选 `reason` 和 `updatedAt`；Studio 从历史 timeline 与运行完成响应中按 `planId` 折叠 latest plan state。Plan turn 完成后需要用户确认实施时，后端创建 `InteractionKind::PlanConfirmation`，前端不再从历史 timeline 自行恢复旧确认 composer。确认 resolution 固定为 `implementFreshContext | continuePlanning | dismiss`，实施时新建 Auto session，并把 plan markdown 放入新 session 的 handoff prompt。

Studio 前端的实时事件、`load_session_timeline` 历史 snapshot 和 `run_prompt` 完成响应必须进入同一个 timeline reducer：

- `load_session_timeline` 是可替换 snapshot，但只有当 `nextSequence` 不落后于当前游标时才能覆盖已有 snapshot。
- 如果 `nextSequence` 落后于当前游标，则该 snapshot 只用于补齐当前状态中不存在的 item。
- `run_prompt` 完成响应是当前 turn 的最终校准，只 upsert 返回的 items 并推进 `timelineNextSequence`，不得清空非 optimistic item。
- `run_prompt`、`resolve_interaction` 产生的 plan lifecycle 事件必须与 timeline item 使用同一个 `timelineNextSequence` 游标规则，避免刷新后重复弹出已处理计划。interaction 状态不使用 timeline 游标恢复；前端通过 `InteractionChanged` 实时更新，并在 `bootstrap`、`select_session`、`load_session_timeline` 和 run 完成响应中从 `interactions` 表加载 pending snapshot。
- `SkillActivated` 是 skill runtime fact 的实时通知与可追踪记录。它不渲染成普通 timeline item；Studio 收到后从后端 runtime snapshot 更新 `activeSkills`，历史恢复以结构化 session skill 表为准，而不是解析 `skill_view` 的 tool result 文本。
- `Done` 只表示 turn 状态完成，不携带 timeline 内容；最终正文必须通过 `text` item 表达。
- Plan Mode 的最终可执行计划必须通过 `plan` item 表达；如果模型只输出计划块而没有普通正文，不应生成空 assistant `text` item。

Studio 渲染可以在 selector 派生出展示项后使用虚拟滚动优化大 timeline，但虚拟滚动层不得改变 item-first 协议语义、事件游标或 reducer 合并规则。动态高度、流式 delta 和自动跟随底部属于前端渲染适配层职责；协议层仍只表达 timeline item 与 delta。

## 8.3 背压与容量

事件通道使用 `tokio::sync::broadcast`。默认容量由调用方创建，目前建议为 `256`。

高频 delta 允许通过 broadcast 丢失实时帧，但 turn 最终的 timeline event 集合必须随消息一起批量落库。`TimelineItemCompleted` 携带最终 snapshot，历史加载不依赖实时 delta 是否完整到达前端。只要 turn 最终有 assistant 正文，最终 timeline 集合中必须存在 completed assistant `text` item；不能只把正文写到 `turn` trace item。

## 8.4 事件边界

事件类型属于协议层，不应包含 provider 私有结构，也不应绑定具体前端。工具审批事件只承载通用工具名、参数和审批结果，不包含 Tauri、React 或桌面端私有状态。

`InteractionChanged` 是审批、用户输入和计划确认的唯一实时交互事件。事件携带 `InteractionRequest`，包括 `kind`、`status`、`scope` 和类型化 payload；持久恢复以 `interactions` 表为准。`userInput` 的 resolved 事件不回传 secret 答案明文到普通 timeline 展示；答案只通过 interaction resolution 返回给等待中的工具。旧 `UserInputRequested` / `UserInputAnswered` 与 `ToolApprovalRequested` 等事件不是 Studio 协议入口，不再由核心层发出。

子代理内部事件不直接转发完整文本流、思考流、工具调用流或工具输出。`pl-core` 将子代理生命周期压缩为 `agent` timeline item 和 `AgentStateChanged` snapshot，状态固定为 `queued`、`running`、`waiting`、`completed`、`errored`、`interrupted`、`shutdown`、`notFound`。`pure-studio` 持久化这些状态事件，并在聊天界面只渲染路径、状态、摘要和最终错误文本，避免把子代理内部执行细节混入父会话 timeline。

失败的子代理必须在 latest snapshot 的 `error` 字段保留可展示的失败文本。`reason` 只作为结构化分类，例如 `providerError`、`toolError`、`budgetLimited` 或 `interrupted`，不能替代 `error`。如果 provider 在子代理已有部分摘要后失败，最终状态仍必须把 provider/tool 错误写入 `error`，否则 UI 无法解释失败原因。

子代理执行遇到 provider `429` 错误码时，视为子代理并发或容量上限。父会话不得因为该子代理不可用而把整轮直接标记为失败；`wait_agent` 或 `list_agents` 的工具结果必须给父 agent 一个可恢复信号，要求当前 agent 停止继续创建子代理并自行完成剩余工作。对应子代理记录仍保持最终失败状态，并在 `error` 字段保留原始 429 错误文本，供 UI 和历史诊断使用。

root agent 的 provider `429` 错误码是当前轮的终止错误，不进入子代理可恢复降级路径。root 收到 429 错误码后必须立即以 failed turn 收尾，广播 `Error` 和 `Done`，不继续工具调用、不继续模型循环，也不写入 assistant 成功消息；会话本身保持可继续，用户之后可以发起新一轮。

子代理的运行指标使用独立的压缩事件转发。`AgentRuntimeUpdated` 只携带 agent 身份、实际模型、上下文窗口、本次 inference token、按币种估算的费用和未计价标记；不携带子代理内部正文、思考、工具参数或工具输出。

## 8.5 流式工具调用聚合与 ID

`pl-model` 负责把 provider 的工具调用 delta 聚合为完整的 `ToolCall` 后再交给 `pl-core` 执行。protocol 层先把 OpenAI Responses 或 Chat Completions SSE 映射为 provider 无关的 stream event，`stream` 层只消费该归一化事件，不解析 OpenAI 原始 JSON。Chat Completions 流式响应中的后续参数片段可能只带 `index`，不再重复 `id` 或 `name`；Responses API 的 custom/freeform 输入 delta 也可能只带 `item_id` / `call_id`。因此聚合层必须使用稳定的流式序号或 item/call id 合并片段，并保留最早出现的 provider id、工具名和调用种类。

工具 timeline item 的 `itemId` 使用 `toolCallId`。当 provider 提供 `call_id` 时，`toolCallId` 优先使用该值；provider 的原始 item id 只作为聚合辅助信息保留在内部。工具参数流、审批、执行和结果都 upsert 到同一个 tool item。

聚合完成前不得把缺少工具名的参数片段当作新的工具调用执行。只有在 `output_item.done` 缺失时，才允许用已聚合的 delta 兜底生成工具调用；该兜底调用仍必须带有前面片段提供的真实工具名和稳定 `toolCallId`。

如果 provider 在 completed 前结束但仍留下未完成 tool accumulator，聚合层只能在工具名和 `id` 或 `call_id` 都稳定时生成兜底 `ToolCall`。缺少工具名时返回 provider/protocol 错误，避免 `pl-core` 收到空工具名并误执行。工具调用进入 `pl-core` 后，started、approved 或 running 状态的 timeline item 在 turn 中断时必须写入唯一 `interrupted` 终态；已经 completed、failed 或 denied 的 item 不得被后续取消路径覆盖。

如果 provider 把工具调用以正文形式返回，例如 DSML/tool-call 标记或完整 JSON `tool_calls` 块，`pl-core` 不得把它作为 assistant 最终消息流给主 chat。该情况属于模型未产出可执行工具调用，turn 应以 `failed` 收尾并触发 `Error` + `Done`。检测必须只针对明显的协议/JSON tool-call 形状，不能因为普通摘要、源码解释或文档内容提到 `tool_calls`、`name`、`subagent` 等词而误判。

显式子代理分工的强制调度只适用于 root turn。子代理任务文本中可能包含 `subagent.rs`、`agent` 生命周期或“每个模块”等普通分析目标，这些内容不能触发子代理递归创建约束。

## 8.6 工具并行

`CompletionRequest.parallel_tool_calls` 随模型能力和 `TurnOptions.tool_execution_mode` 决定，不再硬编码关闭。

`pl-core` 对模型一次返回的工具调用使用 Codex 风格调度：

- 支持并行的工具可同时执行。
- 不支持并行的工具通过独占锁与其他工具互斥。
- 写文件、patch、delete、move、shell 等可能产生副作用的工具默认不并行。
- 只读文件、搜索、stat、list、spawn/wait/list agent 等工具可以按风险显式 opt-in。
- 工具结果写回 `CoreSession` 必须保持模型发出工具调用的顺序。

工具运行时把 unknown tool、权限拒绝、参数错误和本地执行失败都归为模型可恢复 tool result；内部 invariant、join failure 和历史协议污染归为 fatal tool error，当前 turn 使用 `TurnAbortReason::ToolError` 收尾。并行调度可以按完成顺序收集执行结果，但写回 session history 和 provider 下一轮输入时必须恢复模型发出顺序。

## 8.7 Usage 与状态栏

`pl-model::TokenUsage` 保留输入、输出和总 token，并额外记录 `cached_prompt_tokens`。Chat Completions 和 Responses API 的 usage detail 字段不同，protocol 层负责尽可能读取缓存 token；OpenAI 官方字段和一等 provider 私有字段都在内部 typed usage 结构中归一化，缺失时按 `0` 处理。

root agent 和 subagent 使用同一套 runtime usage 数据模型。每次模型 inference 完成后，`pl-core` 以实际使用的 model 计算本次运行指标，并发出 `AgentRuntimeUpdated`：

- `inferenceId` 作为幂等键，防止实时事件和历史回放重复计费。
- `usage` 记录本次 prompt/completion/cache token。
- `estimatedCosts` 按货币分组，只保存能由本地模型价格完整估算的费用。
- `hasUnpricedUsage` 表示存在 token 使用但缺少 currency 或价格字段，UI 不应把它并入任意币种。

Studio 状态栏必须在运行中即时反映上下文和费用。前端消费后端聚合后的运行态快照，并以 `RunPromptResponse.sessionRuntime` 作为完成后的最终校准。前端不得同时按 inference item 和 turn item 重复累计费用。

费用为本地估算值，使用配置中的每百万 token 单价。不同货币不做汇率转换，也不合并为单一数字。React 状态栏消费通用 runtime snapshot，不直接解析 provider 私有 usage 字段。
