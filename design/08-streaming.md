# 08 - Message/Part 流式事件

## 8.1 统一 Message/Part 层

`AgentEvent` 与 `TracePart` 定义在内部 `pl-trace` crate，是核心 turn 与 provider/tool 之间的内部输出通道。`pl-protocol` 只承载 Studio wire DTO 与跨 crate 公共状态类型，不再导出 trace part 或 raw agent event。Studio 对外只消费 `StudioEventEnvelope`，其中对话变化以 opencode 式 `StudioMessage` / `StudioPart` 表达；`TracePart` 只能作为 core/provider 内部诊断输入，不得作为 Studio wire、桥接 DTO 或前端事实源。

实时对话协议固定为：

- `messageUpdated`：完整 message snapshot，等价于 opencode 的 `message.updated`，是 message projection 的事实来源。
- `messagePartUpdated`：完整 part snapshot，等价于 opencode 的 `message.part.updated`，是 part projection、历史恢复和 terminal UI 的事实来源。
- `messagePartDelta`：live overlay，等价于 opencode 的 `message.part.delta`，只用于中间文本、思考、计划和工具参数/结果的即时展示，不写入 `studio_events`。

part 类型固定为：

- `text`
- `reasoning`
- `tool`
- `agent`
- `turn`
- `inference`
- `plan`

每个 message snapshot 必须携带 `messageId`、`sessionId`、`turnId`、`role`、`status`、`createdAt` 和 `updatedAt`。message 首次投影后 `sessionId/turnId/role/createdAt` 不可变；后续 snapshot 只能推进 status、`updatedAt`、`completedAt`、error 和 metadata，terminal message 不得再被后续 snapshot 改写。每个 part snapshot 必须携带 `partId`、`messageId`、`sessionId`、`turnId`、`partType`、`order`、`revision`、`createdAt` 和 `updatedAt`。`StudioEventEnvelope.sequence` 是会话内唯一 durable 事件顺序号；part `order` 只表示同一 message 内展示顺序。后续 snapshot 只 upsert 同一个 part，并且不得改变该 part 的首次展示顺序。`revision` 是单个 part 的 live 内容修订号，start snapshot 为 0，delta 递增，terminal snapshot 携带最新 revision；旧历史或旧客户端缺失该字段时默认 0。delta payload 只携带 `partId/revision/field/delta/chunkIndex`，不携带第二套 message/session 身份或 durable 顺序，只表达对某个 part 字段的 live 追加。

`text` part 必须携带 `textChannel`，固定为：

- `user`：真实用户输入。
- `commentary`：Codex 风格可见进展更新，只用于 UI 展示，不写入最终 assistant 消息历史。
- `final`：最终 assistant 正文，会写入会话历史并作为本轮最终答复。

`commentary` 可以来自模型可见输出，也可以来自运行时生命周期。内部 trace 必须能区分模型输出与运行时主动生成的进展提示；运行时生成的 commentary 在 Studio part 中标记为 `synthetic=true`，用于 timeline 可见展示和历史恢复，但不得作为模型最终正文、assistant 历史消息或后续 provider replay 输入。模型输出的 commentary 保持普通可见输出语义，不自动标记为 synthetic。

`StudioPartDelta` 必须包含 `messageId`、`partId`、`revision`、`field` 和 `delta`；`field` 固定为 `text`、`tool.arguments`、`tool.result`、`reasoning.summary` 或 `planContent` 等协议字段。delta 只能在前端已有 part snapshot 时应用；孤儿 delta 按 opencode 逻辑丢弃。旧的 `role` 字段不再作为 Studio 协议语义入口。

用户 `text` part 可以携带 `attachments` 元数据，当前只用于图片输入缩略图展示。part 的持久语义只依赖附件 id、文件名、媒体类型、尺寸和大小；GUI 事件可以携带派生的 `dataUrl` 预览值用于即时缩略图。模型可见的 base64 图片内容由 `pl-core` 在请求前按附件 id materialize，不写入消息正文。

每个 turn 被接收后，用户输入必须作为该 turn 的第一个 durable user message + text part 记录和广播。用户输入使用固定 identity：message id 为 `{turnId}:user`，part id 为 `{turnId}:user-text`；这是 Studio UI 中用户输入的唯一 durable 来源。后续 core trace 如仍产生用户输入 snapshot，只能作为内部诊断事件，进入 Studio event sink 前必须忽略，不能覆盖或新增用户 text part。纯图片输入也必须产生用户 `text` part，text 可以为空但 attachments 不得为空。enabled tools、`turn`、`inference` 等内部诊断或运行态 part 不得在 projection 上排到用户输入之前，避免前端等待状态、内部状态或历史回放出现在用户问题上方。

assistant 输出 part identity 按 opencode semantic part 建模，而不是按 provider 局部 stream id 建模。`pl-model::stream` 可以继续把 provider text/reasoning/plan block 映射为 `{inferenceId}-{kind}-{ordinal}` 或 `{inferenceId}-text-{channel}-{ordinal}` 形式的 trace item id；该 id 只用于一次 turn 内 start/delta/end 的临时相关性和诊断，不作为最终 `StudioPart.partId`。Trace part 首次进入 Studio runtime 时，由 turn timeline actor 按 message scope 分配 actor-owned `StudioPart.partId`，后续 snapshot 与 delta 均通过 actor 的 trace item -> Studio part 映射路由。每次 inference 重新开始、step boundary、工具调用 start/ready 或 stream 完成都会关闭当前 text/reasoning/plan block。工具后的新 text/reasoning delta 必须先产生新的 empty snapshot，再追加 live delta，不能回写工具前的 part。

历史、实时和 stale backfill 都必须使用 `StudioEventEnvelope.sequence` 作为唯一 durable cursor。`studio_events` 保存 durable snapshot 事件；`studio_messages/message_parts` projection 只能由 `StudioEventRuntime` 从 durable snapshot 事件派生，不提供第二套 timeline cursor 或第二套 wire 协议。`stale` 是 live-only 补拉提示，不写入 `studio_events`，不推进 durable cursor；前端收到后先标记对应 session view，再用当前 cursor 补拉缺失 durable 事件。前端只能用 StudioEvent cursor 判断新旧，不能用 part 列表里的最大 `order` 代替游标。旧 snapshot 只能补齐当前状态中不存在或尚未应用的 event，不能覆盖已经通过实时事件接收的新 turn 内容。前端 optimistic message/part 可以使用临时本地 order 参与展示，但不得预占或推进后端 cursor。

snapshot 与 live delta 的优先级完全对齐 opencode：

- start、完成、失败、审批等待、工具运行、工具完成等状态变化都发 `messagePartUpdated` 完整 snapshot。
- 文本、commentary、reasoning、plan、工具参数和工具结果的流式片段发 `messagePartDelta`，只作为 live overlay。
- 前端按 16ms frame 合批事件；同一 frame 内同一个 part 的多个 snapshot 只保留最后一个，若同 part 的 snapshot 到达，跳过同 part 尚未应用的旧 delta。若 snapshot 因 coalescing 覆盖了更早 snapshot，也必须把同 part 的 pending delta 标记为 stale 并跳过。
- snapshot 到达后清除同 part 的 delta overlay，并以 snapshot 内容为准。
- terminal snapshot 到达后，后续 `streaming`、`started` 或低 revision snapshot 不得覆盖 `completed`、`failed`、`interrupted`、`denied`、`budgetLimited` 等终态；revision 不高于当前 overlay/snapshot 的 live delta 不得再修改 part。live delta 的 revision 必须相对当前可见 revision 严格连续递增；发现 revision 跳号时前端必须丢弃该 part 的 live overlay 并触发 session 恢复，不能静默拼接缺失片段后的内容。带 `chunkIndex` 的 delta 必须按 part 去重；重复或倒序 chunk 直接丢弃。
- reload、历史恢复和 stale backfill 只依赖 durable snapshot；丢失 live delta 不影响最终可恢复状态。

Studio store 持久化 `messagePartUpdated` 前必须在同一事务内验证 part 状态转移，但不得再使用 durable event sequence 分配新 part 的展示 order。`StudioEventRuntime` 托管的 turn timeline actor 在 trace part 首次进入 Studio 前按 message scope 分配 `StudioPart.partId` 与 `order`，并为 assistant tool part 分配 `StudioPart.activityGroupId` 工具活动段；同一 `sessionId + assistant messageId` 内，连续工具复用当前活动段，遇到最终会进入 assistant 阅读流的可见非工具 part（text、reasoning、plan、agent）先关闭当前活动段，之后的工具新开段。用户消息、turn/inference/file 诊断 part、ignored part 与 `synthetic=true` 的 runtime commentary 不开启也不关闭工具活动段。actor 维护 `(sessionId, turnId, trace item id) -> Studio part id` 的路由表和每个 part 的 live revision 基线；message order cursor 也必须按 `(sessionId, messageId)` 隔离，不能让不同 session/turn 中复用的 provider id 或 message id 共享顺序状态。store 暴露的下一可用 order 只作为 actor 在恢复或并发进入时的 durable 下界，不能替代 actor 对本轮新 part 的分配状态。`messagePartDelta` 只有在 trace item 已映射到目标 Studio part、目标 part 已存在、目标 part 尚未终态、且 revision 相对当前 live revision 严格 `+1` 时才广播。孤儿、跳号、重复或终态后的 delta 统一转成 live-only `stale` 提示，由前端按当前 cursor 补拉 durable snapshot。首次创建后的 `partId`、`messageId`、`sessionId`、`turnId`、`partType`、`textChannel`、`activityGroupId`、`order` 和 `createdAt` 不可变；同一 message 内不能出现重复 order；revision 不得倒退；terminal part 不允许被任何后续 snapshot 修改正文、工具结果、附件、错误、usage、synthetic/ignored 或其他内容字段。校验失败的 snapshot 不写入 projection，也不写入 durable `studio_events`。

模型流投影在收到 provider tool input 增量时创建 tool part，并把 arguments 增量作为 `tool.arguments` live delta；当同一 provider item 后续被确认为可执行 tool call 时，只能刷新名称、参数和 provider/call id 等结构字段，不能把已经 `streaming`、`awaitingApproval`、`approved` 或 `running` 的 tool part 回退成 `started`。执行阶段只能沿 `awaitingApproval -> approved -> running -> terminal` 或直接 terminal 的方向推进。

工具调度层不得在普通 verbosity 下为工具阶段生成 runtime commentary；工具阶段的可见进展由 Studio tool part 和 Flutter 的“工具活动”结构化块承担。普通 timeline 不显示“模型请求调用 N 个工具”、工具执行结束、结果回写上下文、准备继续调用模型这类与工具活动块重复或低信息量的状态。若诊断仍需要这些状态，只能作为 `toolDetail` 或 `debug` 在 verbose/debug 下生成。单个工具开始、完成、审批、审查等细节同样属于 `toolDetail` 级别，只在 verbose/debug 下生成。该 commentary 不替代 tool part，也不承载工具参数、stdout/stderr 或最终结果；工具事实仍以逐工具 `tool` part snapshot 和 `tool.result` delta 为准。工具进度 commentary 必须标记为 synthetic，并且不得写入后续 provider history。

上下文自动压缩属于 turn 内部 runtime 阶段，也应生成 synthetic commentary：开始压缩、因 context/token limit 缩小历史后重试、压缩完成并继续模型调用。该进展不改变消息历史协议；压缩后的摘要仍只通过 session history replacement 生效，重试过程不得写入 assistant final 正文。

`bash` 命令运行期间，stdout/stderr chunk 作为原始命令输出追加到原 `bash` tool part 的 `tool.result` live overlay；stderr chunk 由投影层保留来源标记，前端按普通工具结果增量展示。每个 chunk 使用该 tool part 当前 revision 作为基线继续递增，终态 `messagePartUpdated` 携带不低于最后一个输出 delta 的 revision，并以紧凑 JSON 结果固化 snapshot；`write_stdin` 轮询只返回自己的紧凑结果，不把同一后台进程的输出复制成新的父 timeline tool part。

reasoning part 默认只作为折叠的“思考中 / 已思考”结构化行显示，来源只能是 provider 明确标记的 reasoning summary 或 canonical thinking stream，并通过 `reasoning.summary` delta 字段进入 Studio。展开后可以显示该 provider-emitted reasoning 文本；provider replay metadata、后端诊断和未进入 `StudioPart.text` 的内部推理不得混入 timeline 正文。`CompletionResponse.reasoning_content` 只保存 raw reasoning replay 内容，不混入 commentary/final 文本。只有 Responses native phase，或 Chat provider 明确输出的 `<commentary>`、`<final>` 可见标签段，会被投影成对应 commentary 或 final part。Plan part 只能来自 `plan_exit.content`。

## 8.2 数据流

```text
pl-model provider
  → async-openai stream
  → protocol stream event mapper
  → provider-independent stream accumulator
  → pl-trace AgentEventSender / TraceRecorder 内部事件
  → pl-core StudioRuntime::drain_agent_events
  → StudioEventRuntime 分配 durable sequence、规范化 payload、持久化 message/part snapshot + projection、广播 snapshot/live delta
  → pl-studio-bridge subscribeSessionEvents(sessionId)
  → FRB Stream<BridgeEventEnvelope>
  → Flutter Riverpod Studio event reducer
```

`pure-studio-flutter` 不直接订阅 `AgentEventReceiver`，也不依赖内部 `pl-trace` crate。raw `AgentEvent` 到 Studio event 的映射在 `pl-core` 内完成；`pl-studio-bridge` 只转发已经规范化的 `StudioEventEnvelope`。Flutter FRB adapter 从 `BridgeEventPayload` sealed union 归一为 app 内部 typed `StudioBridgeEventPayload`；event reducer 按 payload 类型更新当前或后台 session view：

- `messageUpdated` upsert 完整 message snapshot。
- `messagePartUpdated` upsert 完整 part snapshot，并清空同 part 的 delta overlay。
- `messagePartDelta` 只在已有 part snapshot 时追加 live overlay；孤儿 delta 丢弃。delta 不得覆盖 terminal snapshot，也不得让低序/重复 chunk 反向污染已完成的 text/reasoning/plan/tool 字段。
- `turnChanged` 更新 queued、contextLoading、waitingForModel、streaming、runningTool、waitingForInteraction、persisting、completed、failed、cancelled。
- `interactionChanged` 更新 footer 交互状态。
- `agentChanged` / `agentTimelineChanged` 分别更新 latest snapshot 与 append-only 协作事件。
- `sessionRuntimeChanged`、`skillActivated`、`mcpHealthChanged`、`lspHealthChanged` 即时更新状态栏。
- `sessionListChanged` 是会话元状态的事实事件，payload 必须包含 `projectId` 和该项目最新 root `sessions`。创建会话、归档会话、归档项目、切换 session mode、Plan 实施把当前 session 切回 `auto` 等会话列表或会话摘要变化，都必须在持久写入后广播该事件；命令返回值只作为请求确认或冷启动 snapshot，不作为 UI 唯一刷新路径。
- `sessionHandoffChanged` 不再作为 Flutter 前端协议入口；Plan 实施在当前 `sessionId` 内启动新 turn，不再依赖 handoff target child session 展示实施过程。`sessionListChanged` 只驱动 root 会话列表可见性，legacy child/archived session 不计入 root session 列表。

`TextDelta`、`ThinkingDelta`、`ToolCallDelta`、`ToolCallComplete` 不再是 Studio 的协议或兼容入口。

## 8.2.1 Flutter/FRB 订阅边界

Flutter 端通过 `pl-studio-bridge` 从 `StudioEventRuntime` 创建两类 stream：

- `subscribeSessionEvents(sessionId)`：会话级高频流，包含该会话的 `messageUpdated`、`messagePartUpdated`、`messagePartDelta`、`turnChanged`、`interactionChanged`、`sessionRuntimeChanged`、`agentChanged` 和 `agentTimelineChanged`。
- `subscribeGlobalEvents()`：全局低频流，包含项目/会话列表变化、配置变化、Provider usage、MCP/LSP health、全局 stale 和 runtime lifecycle 变化。

`messagePartDelta`、reasoning delta、tool 参数/结果 delta、plan/content delta 等高频事件只能进入会话流。Flutter 页面切换会话时必须取消旧会话 stream，再订阅新会话；后台会话只保留 durable projection 与低频列表状态，不继续推送高频 delta。

FRB 事件 envelope 固定为：

```text
BridgeEventEnvelope {
  eventId,
  sessionId,
  turnId,
  sequence,
  createdAt,
  payload: BridgeEventPayload,
}
```

字段命名在 Dart wire 层使用 camelCase。`payload` 是 FRB/Freezed sealed union，承载结构化事件事实；Dart adapter 只做 DTO 到 app domain 的归一化，Riverpod reducer 不读取 payload Map。桥接层不得暴露 `serde_json::Value`，也不得把 Flutter 专用字段混入 `StudioEventEnvelope`。实时 stream、`loadStudioEvents` backfill、bootstrap snapshot、session snapshot 和命令响应均使用 typed FRB DTO；完整 config/general settings 与工具参数这类开放 JSON 标量只能停留在 Dart adapter 边界，interaction payload 与 agent timeline payload 必须使用 typed DTO/union，不得重新成为实时 reducer 协议。

模型 provider 流的成功边界由 canonical `ModelStreamEvent::Completed` 明确表示。protocol mapper 可以把 provider 私有终止 chunk 转换为该事件；如果底层 SSE parse、transport 或 EOF 在 completed 之前发生，`pl-model` 必须返回错误，并由 turn 层发出 failed turn、`Error` 和 `Done`，不得把局部内容当作成功消息落库。completed 之后的 usage、文本、思考和工具调用 snapshot 才能进入最终 `CompletionResponse`。

Plan Mode 下计划确认的主触发源是 `plan_exit` 工具。模型完成可执行计划后调用 `plan_exit({ content })`，`pl-core` 使用工具参数中的 Markdown 计划补齐或覆盖同一 turn 的 `plan` part，并在 turn 完成后创建 `PlanConfirmation` interaction。`plan_exit` 只提交计划，不在工具内部等待用户选择，也不写入 opencode 风格计划文件。

Chat provider 不再通过 `<proposed_plan>...</proposed_plan>` 提交或展示计划；该旧标签按普通未标记文本处理，不生成 `plan` part，也不能触发计划确认。计划正文写入 `StudioPart.plan.content`，增量使用 `StudioPartDelta(field=planContent)`，来源只能是 `plan_exit.content` 或后续明确的 Plan lifecycle 事件。

Auto 与 Plan Mode 下模型可见输出优先使用 provider 原生可见 phase；Chat 兼容 provider 使用显式标签：`<commentary>...</commentary>` 表示中间进展，`<final>...</final>` 表示最终正文；Plan Mode 的最终计划必须通过 `plan_exit` 提交。`pl-model` visible output decoder 负责按 endpoint 协议跨 chunk 解析或归属这些可见输出，标签本身不得进入 timeline。未标记的普通输出默认进入 `final` 文本通道并写入 assistant 正文，不能导致 turn 失败；Plan Mode 下未标记文本也不伪造 `plan` part。

Chat tagged decoder 必须把显式可见标签建模为 block lifecycle，而不是把同一通道的所有标签段合并到固定 provider id：每个 `<commentary>` 或 `<final>` 开标签创建新的 provider-local text block，闭标签立即完成该 block；连续出现的同通道标签也必须得到不同 block id。未标记普通 `content` 可作为 fallback `final` block 继续流式展示，直到遇到显式标签、工具边界或 stream 完成；进入 Studio trace 前仍由 trace projection 映射为 turn/inference 作用域内的稳定 semantic part id。

部分 OpenAI-compatible Chat provider 会把带可见标签的输出放入 `reasoning_content`。Chat tagged decoder 必须保留这部分原始内容作为 raw reasoning，同时只把其中显式 `<commentary>` 和 `<final>` 标签段投影到可见 timeline；无标签 `reasoning_content` 不得变成 assistant 正文、plan 或 reasoning summary row。

计划的采纳与实施状态不改变 `plan` part 本身，而是通过 `StudioEventKind::PlanLifecycleChanged` 写入 durable `studio_events` 并广播。事件包含 `planId`、`state`、可选 `turnId`、可选 `reason` 和 `updatedAt`；Studio 从 durable events 中按 `planId` 折叠 latest plan state。Plan turn 完成后需要用户确认实施时，后端创建 `InteractionKind::PlanConfirmation`，前端不再从历史 timeline 自行恢复旧确认 composer。确认 resolution 固定为 `implementFreshContext | continuePlanning | dismiss`；`continuePlanning` 的 `content` 是确认 composer 同次提交的用户补充内容，resolution 成功后由前端立即作为普通 prompt 发送；`implementFreshContext` 保留 wire 名称但不再创建 fresh session，后端必须在当前 session 内解决 interaction、把当前 session mode 持久切换为 `auto`、广播 `accepted/implementing`，并用同一 `sessionId` 启动实施 turn。前端在提交 `implementFreshContext` 后应立即把当前 session mode 乐观投影为 `auto`，避免状态栏在实施 turn 已启动时仍显示 Plan。实施 turn 的实时 `turnChanged/messageUpdated/messagePartUpdated/messagePartDelta/sessionRuntimeChanged` 直接更新当前会话，不能通过 `sessionHandoffChanged` 切换目标会话。

Studio 前端的实时事件、`load_session_state` projection snapshot 和 `load_studio_events` 补拉结果必须进入同一个 StudioEvent reducer：

- `load_session_state` 返回当前 `{ message, sequence }[]`、`{ part, sequence }[]` projection snapshot、非 message/part durable 状态事件与 `eventNextSequence`；前端先用 projection record 初始化 message/part state 和 per-id sequence guard，再用同一个 reducer 应用状态事件。
- `load_studio_events(afterSequence)` 返回缺失的 typed bridge envelope；其 payload 事实必须与数据库中保存的 durable envelope 一致，并通过与实时 stream 相同的 reducer 入口应用。
- `StudioMessage` / `StudioPart` 是前端 reducer 的状态事实源；timeline row 只是 selector/view model 的折叠结果，不作为 bridge command 的主输入 DTO。
- `submit_prompt` 与触发实施的 `resolve_interaction` 不返回最终 timeline；它们只返回提交成功、目标 `sessionId/turnId/cursor`。
- `set_session_mode`、`create_session`、`archive_session` 和 Plan 实施确认必须通过 `SessionListChanged` 更新前端会话摘要；前端可以做乐观投影，但最终仍以 stream reducer 中的事件为准。
- Plan lifecycle 与 interaction 状态均通过 `StudioEvent` 实时更新，并在 `bootstrap`、`select_session`、`load_session_state` 和 `load_studio_events` 中恢复。
- `SkillActivated` 是 skill runtime fact 的实时通知与可追踪记录。它不渲染成普通 timeline row；Studio 收到后从后端 runtime snapshot 更新 `activeSkills`，历史恢复以结构化 session skill 表为准，而不是解析 `skill_view` 的 tool result 文本。
- `Done` 只表示 turn 状态完成，不携带 timeline 内容；最终正文必须通过 `textChannel=final` 的 `text` part 表达。
- Plan Mode 的最终可执行计划必须通过 `plan_exit.content` 生成 `plan` part。如果模型只提交计划而没有普通正文，不应生成空 assistant `text` part。

Studio 渲染使用 opencode app 同款 timeline 框架语义：`virtua` 虚拟列表、自写 row algebra、stable row key、bottom spacer、row cache、part group 和 delta overlay。虚拟滚动层不得改变 Message/Part 协议语义、事件游标或 reducer 合并规则。动态高度、流式 delta overlay 和自动跟随底部属于前端渲染适配层职责；协议层仍只表达 message/part snapshot 与 live delta。

流式 Markdown 使用 opencode 的 stream-safe 渲染规则。`planContent`、普通 text 和 commentary 的 live overlay 在 Flutter timeline 展示层原生直接用 `GptMarkdown` 渲染，以当前 part 累计文本作为输入，不再通过自定义兼容 renderer facade 转发。展示前只允许做轻量 agent repair，未闭合代码块、链接引用和不完整 Markdown 由 renderer 容错展示；Rust/FRB 事件协议仍只表达 message/part snapshot 与 live delta，不承担 Markdown 补全。terminal `messagePartUpdated` 到达后清除 overlay，并用完整 snapshot 重新渲染。

状态栏同样是 Studio store projection 的消费者。Flutter store 必须保存当前 session 的 `sessionRuntime`、`turnPhase/turnStartedAt`、`agents`、`mcpServers/activeMcpServers`、`lspServers/activeLspServers`、`providers/roles/permissionMode`，并由 Studio event 与 bootstrap/session snapshot 恢复。模型、reasoning effort、模式和权限控制通过 `pl-studio-bridge` command 更新配置或 session mode；状态栏不得直接推断 timeline 内容来累计 token 或费用。普通 root turn 和 Plan 实施 turn 在写入最新 runtime snapshot 后必须广播 `SessionRuntimeChanged`，避免只有刷新或切换 session 后才看到 context/cost 更新。Flutter 状态栏的 context 展示使用无数字圆形进度条；鼠标悬停进度条时显示具体 context token/window、百分比、总 token 和模型。费用、active skills、MCP、LSP 与 subagent 活动仍作为独立状态项保留。

## 8.3 背压与容量

内部事件通道可以继续使用 `tokio::sync::broadcast`，但 Studio 对外语义是持久事件流。默认容量由调用方创建，目前建议为 `256`。

高频 delta 可以在 broadcast 层 lag，但不能静默丢失 Studio 状态：每个 durable snapshot 必须先写入 `studio_events` 和 projection，再广播同一份 canonical envelope。live delta 和 `stale` 只进入实时 event stream；前端发现 sequence 缺口或收到 stale 事件时调用 `load_studio_events(sessionId, afterSequence, limit)` 补齐 durable snapshot。completed/failed snapshot 携带最终内容，历史加载不依赖实时 delta 是否完整到达前端。只要 turn 最终有 assistant 正文，最终 message/part 集合中必须存在 completed assistant `text` part；不能只把正文写到 `turn` trace item。

Flutter runtime bridge 检测到底层 broadcast receiver `Lagged` 时，必须为 active session 广播 live-only `stale` 事件。`stale` 不写入 `studio_events`，只驱动前端按当前 durable cursor 补拉缺失 snapshot；补回事件仍进入同一个 reducer。

实现允许把 live delta 和 stale 通知保留为内部诊断记录，但不得写入 `studio_events`。`load_session_state` 通过 message/part projection snapshot 恢复终态，并只附带非 message/part durable 状态事件；`load_studio_events` 只返回 durable snapshot 与其他状态事件。

## 8.4 事件边界

事件类型属于协议层，不应包含 provider 私有结构，也不应绑定具体前端。工具审批事件只承载通用工具名、参数和审批结果，不包含桌面端私有状态。

`StudioEventKind::InteractionChanged` 是审批、用户输入和计划确认的唯一实时交互事件。事件携带 `InteractionRequest`，包括 `kind`、`status`、`scope` 和类型化 payload；持久恢复以 `interactions` 表为准。`userInput` 对齐 opencode 的 `question` 工具体验：pending/running 阶段由 dock prompt 负责真实问题输入，timeline 隐藏对应 `request_user_input`/`question` tool part；resolved 后可以从 redacted tool result 渲染问题与答案摘要。`userInput` 的 resolved 事件不回传 secret 答案明文到普通 timeline 展示；答案只通过 interaction resolution 返回给等待中的工具。`planConfirmation` 同样是 dock prompt 交互，不是从 timeline plan part 自行派生的按钮。旧 `UserInputRequested` / `UserInputAnswered`、`ToolApprovalRequested`、`studio-interaction-changed` 等 sideband 不是 Studio 协议入口。

agent 协作 timeline 遵循 Studio 规范化协议：`agentChanged` 更新 latest snapshot，`agentTimelineChanged` 只携带精简 `SubAgentActivity` payload。`bootstrap`、`select_session` 和 `load_session_state` 的 `agentEvents` 历史快照返回 `BridgeAgentTimelineEventDto`，其中 payload 为 `BridgeAgentTimelinePayloadDto` typed union；Flutter 不解析 raw `AgentEvent` 或历史 `payloadJson` 记录。旧 `spawnBegin`、`spawnEnd`、`waitingBegin` 等非 `SubAgentActivity` agent timeline 历史数据由迁移清理，不作为运行期兼容入口保留。agent timeline 历史 payload 如果无法反序列化为规范化事件，应作为协议/数据错误显式暴露，不能降级成 unknown activity row。MCP/LSP health 事件同样携带 canonical health snapshot。内部 trace 如需保留原始事件，只能作为诊断输入，在进入 Studio wire 前完成映射。

模型 stream trace projection 可以继续使用当前 turn recorder sequence 作为每次 inference 的 `trace_sequence_base`，为内部诊断和历史 trace 提供单调事件序；Studio 展示 order 不读取 provider event sequence 或 trace event sequence，而由 turn timeline actor 结合现有 projection 和 message scope 分配。后续如果移除 `trace_sequence_base`，不得重新把 provider event sequence、trace event sequence 与 Studio part order 混为一个概念。

子代理内部事件不直接转发完整文本流、思考流、工具调用流或工具输出。`pl-core` 将子代理生命周期压缩为 `agent` part 和 `AgentStateChanged` snapshot，状态固定为 `queued`、`running`、`waiting`、`completed`、`errored`、`interrupted`、`shutdown`、`notFound`。Studio 持久化这些状态事件，并在聊天界面只渲染路径、状态、摘要和最终错误文本，避免把子代理内部执行细节混入父会话 timeline。

失败的子代理必须在 latest snapshot 的 `error` 字段保留可展示的失败文本。`reason` 只作为结构化分类，例如 `providerError`、`toolError`、`budgetLimited` 或 `interrupted`，不能替代 `error`。如果 provider 在子代理已有部分摘要后失败，最终状态仍必须把 provider/tool 错误写入 `error`，否则 UI 无法解释失败原因。

子代理执行遇到 provider `429` 错误码时，视为子代理并发或容量上限。父会话不得因为该子代理不可用而把整轮直接标记为失败；`wait_agent` 或 `list_agents` 的工具结果必须给父 agent 一个可恢复信号，要求当前 agent 停止继续创建子代理并自行完成剩余工作。对应子代理记录仍保持最终失败状态，并在 `error` 字段保留原始 429 错误文本，供 UI 和历史诊断使用。

root agent 的 provider `429` 错误码是当前轮的终止错误，不进入子代理可恢复降级路径。root 收到 429 错误码后必须立即以 failed turn 收尾，广播 `Error` 和 `Done`，不继续工具调用、不继续模型循环，也不写入 assistant 成功消息；会话本身保持可继续，用户之后可以发起新一轮。

子代理的运行指标使用独立的压缩事件转发。`AgentRuntimeUpdated` 只携带 agent 身份、实际模型、上下文窗口、本次 inference token、按币种估算的费用和未计价标记；不携带子代理内部正文、思考、工具参数或工具输出。

## 8.5 流式工具调用聚合与 ID

`pl-model` 负责把 provider 的工具调用 delta 聚合为完整的 `ToolCall` 后再交给 `pl-core` 执行。protocol 层先把 OpenAI Responses 或 Chat Completions SSE 映射为 provider 无关的 content block lifecycle `ModelStreamEvent`，`stream` 层只消费该归一化事件，不解析 OpenAI 原始 JSON。Responses 和 Chat 的可见文本协议必须在 decoder 层显式分离：Responses 使用 native phase decoder，优先读取 `response.output_item.added/done` 中 assistant message 的 `phase=commentary/final_answer/final`，并把后续 `response.output_text.delta` 归属到对应 `textChannel`；Chat Completions 使用 tagged text decoder，把普通 `content` 以及带标签的 `reasoning_content` 中的 `<commentary>`、`<final>` 转成 canonical text lifecycle。provider-independent accumulator 不再持有 tag parser，也不再根据 endpoint 猜测可见输出协议。Chat Completions 流式响应中的后续参数片段可能只带 `index`，不再重复 `id` 或 `name`；Responses API 的 custom/freeform 输入 delta 也可能只带 `item_id` / `call_id`。因此聚合层必须使用稳定的流式序号或 item/call id 合并片段，并保留最早出现的 provider id、工具名和调用种类。

`ModelStreamEvent` 的 assistant content 语义与 opencode 的 part/lifecycle 模型对齐：`text`、`reasoning`、`plan` 和 `tool` 都有独立 start/delta/complete 生命周期。OpenAI decoder 与 Chat tagged decoder 是兼容边界：如果 provider 没有显式 start，但后续 delta 或 authoritative done 足以证明存在可见 block，decoder 必须先补出 `BlockOpened`，再输出 delta/close。`pl-model::stream::lifecycle` 只校验和收尾已打开的 block，不再为缺失 start 的 delta 自动创建 block；delta 或 close 命中未打开/已关闭 block 属于 provider stream protocol error。进入 Studio 前必须转为 `StudioMessage` / `StudioPart` snapshot 或 live delta，不再从普通文本或 thinking 内容推断 plan。

Plan 是协议级 block，Plan Mode 中只能由成功执行的 `plan_exit.content` 生成 `plan` part。`<commentary>` 与 `<final>` 由 Chat tagged decoder 转换为带 `TraceTextChannel` 的 text lifecycle。Responses native phase decoder 不解析这些标签，也不得把 native commentary/final 文本再次交给 tag parser。未标签普通 Chat text 默认进入 `final` text block；未标签 raw reasoning 只进入内部 reasoning buffer，不能生成 assistant 正文、plan 或 reasoning summary row。部分 OpenAI-compatible Chat provider 会把带可见标签的输出放入 `reasoning_content`，该兼容转换只允许提取显式 commentary/final 标签段，同时保留完整 reasoning 原文作为 raw reasoning。

Chat tagged decoder 输出的 text lifecycle 必须反映标签开闭边界：每个显式可见标签段拥有独立 provider-local block id，并产生 start/delta/complete；闭标签之后的后续标签或未标记 fallback 文本不得继续复用已完成 block。该 id 只用于 `pl-model::stream` 内部归并，不能作为最终 Studio part identity。

Chat tagged decoder 对未标记普通 `content` 保持兼容：默认仍作为 fallback `final` block 展示和进入最终正文，避免旧 provider 直接失败；同时应记录未标记可见文本诊断，供开发者观察 provider 是否遵守 commentary/final 标签合约。诊断不得改变 wire 形状，也不得把未标记 raw reasoning 转成可见正文。

工具 trace item 可以继续使用模型工具调用 id、Responses `call_id` 或 runtime tool id 做相关性锚点，但 `StudioPart.partId` 仍由 turn timeline actor 分配。`StudioToolPart.toolCallId` 保存 runtime 工具展示/执行 id，`StudioToolPart.providerItemId` 保存 provider item id，`StudioToolPart.callId` 保存 Responses call id。工具参数流、审批、执行和结果都通过 actor 映射 upsert 到同一个 tool part。Studio wire、FRB 和持久化 projection 不新增聚合工具 part；每个工具仍作为独立 `StudioPartType::Tool` 持久化，并携带 actor 分配的 `activityGroupId`。Flutter timeline selector 只能把相同 `activityGroupId` 的 tool part 投影成一个默认折叠的工具活动组；缺少 `activityGroupId` 的旧工具 part 按单工具组展示，不能按 turn/message 猜测合并。展开后仍展示每个原始工具 part 的结构化详情。

聚合完成前不得把缺少工具名的参数片段当作新的工具调用执行。只有在 `output_item.done` 缺失时，才允许用已聚合的 delta 兜底生成工具调用；该兜底调用仍必须带有前面片段提供的真实工具名和稳定 `toolCallId`。

如果 provider 在 completed 前结束但仍留下未完成 tool accumulator，聚合层只能在工具名和 `id` 或 `call_id` 都稳定时生成兜底 `ToolCall`。缺少工具名时返回 provider/protocol 错误，避免 `pl-core` 收到空工具名并误执行。工具调用进入 `pl-core` 后，started、approved 或 running 状态的 tool part 在 turn 中断时必须写入唯一 `interrupted` 终态；已经 completed、failed 或 denied 的 part 不得被后续取消路径覆盖。

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

Studio 状态栏必须在运行中即时反映上下文和费用。前端消费 `StudioEventKind::SessionRuntimeChanged` 中后端聚合后的运行态快照；刷新或切换 session 时用 `load_session_state` / `select_session` 的 `sessionRuntime` 恢复。`AgentRuntimeUpdated` 进入 Studio bridge 时必须先写入 agent/session runtime projection，再广播 `SessionRuntimeChanged`；turn 收尾只在本 turn 没有实时 inference runtime snapshot 时，才用最终 usage 补写 legacy root delta，避免同一轮 usage 被实时事件和收尾事件重复累计。前端不得同时按 inference item 和 turn item 重复累计费用。

费用为本地估算值，使用配置中的每百万 token 单价。不同货币不做汇率转换，也不合并为单一数字。Flutter 状态栏消费通用 runtime snapshot，不直接解析 provider 私有 usage 字段。
