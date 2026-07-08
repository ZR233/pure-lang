# Pure Studio UI

本文约束唯一的 `code/pure-studio-flutter` Flutter 桌面端。Flutter 端使用 Material 3 工具型设计、Riverpod 状态管理和 FRB session/global stream，覆盖 Studio 主路径功能。

## 1. 前端框架

Timeline 直接对齐 opencode app：使用 `virtua` 虚拟列表，自写 message/part row algebra、stable row key、row cache、bottom spacer、bottom anchoring 和 jump-to-bottom 交互。允许复制 opencode MIT timeline/UI 子集；复制文件必须保留来源说明，并在仓库 notice 中标注。

`code/pure-studio-flutter` 使用 Flutter Windows 桌面端。入口为 `MaterialApp.router`，页面栈由 `go_router` 管理；状态层使用 `flutter_riverpod`，数据入口只允许通过 `pl-studio-bridge`。Flutter UI 使用 Material 3 组件、紧凑桌面工具布局和响应式双栏/窄屏 rail，不做营销页或解释页。首版可以用 `ListView.builder` 实现 timeline，但必须保留 stable key、底部跟随、row cache、streaming Markdown overlay 和会话切换时取消旧订阅的语义；后续可再替换为虚拟列表。

Flutter 推荐目录结构：

```text
lib/src/app/
lib/src/data/frb/
lib/src/data/repositories/
lib/src/domain/models/
lib/src/domain/reducers/
lib/src/domain/view_models/
lib/src/features/shell/
lib/src/features/timeline/
lib/src/features/interaction/
lib/src/features/status/
lib/src/features/settings/
lib/src/shared/
```

## 2. Conversation Store

状态按 session 归一化保存：

- `messages[sessionId]`
- `parts[messageId]`
- `partTextAccumDelta[partId]`
- `messageSequences[messageId]`
- `partSequences[partId]`
- `eventNextSequence[sessionId]`
- `sessionStatus[sessionId]`
- `interactions[interactionId]`
- `planStates[planId]`
- `agents/sessionRuntime/agentTimelineEvents`
- `mcpServers/lspServers`
- `turnPhase/turnStartedAt`

`messageUpdated` upsert message snapshot；message snapshot 保留 `turnId/status/updatedAt/completedAt/error` 等 lifecycle 字段，但 message `createdAt` 首次创建后不可因后续 snapshot 回退或覆盖。message lifecycle 只更新 message snapshot，不驱动 part 终态，也不从 part 状态反推 message 状态。`messagePartUpdated` upsert 完整 part snapshot 并清除该 part 的 live delta；`messagePartDelta` payload 只携带 `partId/revision/field/delta/chunkIndex`，session 归属来自 envelope，message/turn 归属来自已有 part snapshot，不在 delta 内重复携带或信任第二套身份。`messagePartDelta` 只允许命中已有 part，orphan delta 直接丢弃。`messagePartDelta` 不推进 durable cursor，也不得覆盖 terminal snapshot。前端记录 part 的 snapshot sequence、delta sequence 和可选 `chunkIndex`，丢弃同 part stale delta、低序 delta 与重复/倒序 chunk。`messageRemoved`、`messagePartRemoved`、session reset 和 projection snapshot 替换必须清理相关 delta accum。

`StudioPart.revision` 与 `StudioPartDelta.revision` 是每个 part 的 live 版本号。start snapshot 使用 `revision=0`，同 part 每个 delta 递增，terminal snapshot 携带最新 revision。前端 reducer 必须按 `partId + field` 保存 overlay，并在 `delta.revision <= lastRevision` 时丢弃该 delta；terminal snapshot 到达后清理 overlay，terminal 后到达的 delta 一律丢弃。旧历史或旧后端缺失 revision 时按默认 0 读取，只能作为 durable snapshot 初始化；live delta 必须携带大于当前 field revision 的 revision，不能用缺省 0 覆盖已有 snapshot 或 overlay。

历史、实时和 stale backfill 都进入同一个 event reducer。`load_session_state` 用 projection snapshot 初始化 message/part 与 per-id sequence guard；`load_studio_events(afterSequence)` 只回放 durable envelope。前端不得恢复旧 `TimelineItem`、`ConversationEntry` 或 raw `AgentEvent` 入口。

Flutter 解析层必须接受 Studio 协议内的所有 part type。当前不直接渲染的 lifecycle/internal/file part 可以进入 normalized snapshot 后由 row projection 过滤，或在 bridge payload 层忽略，但不能把协议内类型当未知类型抛出导致 timeline 白屏。真正未知的 part type 仍应 fail fast。

切换或恢复选中 session 时必须建立 session load barrier：先带 generation 订阅目标 `sessionId` 的实时 stream，再加载 `load_session_state`，加载期间同 generation 的 session event 只进入 buffer，不直接修改 timeline。snapshot 返回后若 generation/session 已过期则丢弃；否则先用 per-message/per-part sequence guard 合并 snapshot，再按事件 sequence 和到达顺序重放 buffer。live-only 事件没有 durable 排序语义，若多个 live-only 事件在 barrier 中拥有相同 sequence/createdAt，必须保留接收顺序，不能用 eventId 重新排序导致 part revision gap。重放 durable event 前必须按 snapshot 合并后的 `eventCursorsBySession[sessionId]` 丢弃 `sequence <= cursor` 的旧事件；cursor 合并取较大值。`messagePartDelta` 继续作为 live-only overlay，不推进 durable cursor，只按 part revision 单独处理。旧订阅迟到事件、缺失 sessionId 的 timeline event 或 generation 不匹配的 event 必须丢弃，不能污染当前会话。

状态管理对齐 opencode `global-sync`：Flutter Riverpod store 只保存归一化 entity 表和少量 UI 本地状态，组件不得直接把多个表临时拼成业务状态。选中会话、状态栏、timeline、交互 dock 和会话列表都必须通过 selector/view model 派生：

- `selectedSessionView` 从 `selectedSessionId` 读取当前 session、message、part、runtime、agent、interaction、turn phase、busy、MCP/LSP active 列表。
- `visibleProjectSessions` 对 session list 做按 id 去重、过滤 `visibility=active` 且 `parentSessionId` 为空的 root session，并稳定排序，避免 handoff/archived/child session 或重复 DTO 出现在会话栏。Plan implementation 在当前 session 内运行，不创建 target session；legacy child session 即使存在也只可通过历史入口加载，不能作为侧栏 root 项。
- `SessionStatusBar` 只消费 `selectedSessionView.runtime/agents/activeMcpServers/activeLspServers`，不得直接读后台 session 的 runtime event。

`sessionRuntimeChanged` 只能更新 `sessionRuntime[sessionId]`；MCP/LSP 的全局 health event 更新 server catalog，当前会话实际 active 列表来自 selected session runtime。`agentChanged` 是 latest snapshot merge：如果新 snapshot 未携带 `runtimeUsage`，必须保留同 agent 已有的 runtime usage，避免状态变更覆盖 token/cost 信息。

Flutter Riverpod store 使用同一归一化状态结构：`StudioController` 负责 bootstrap、session stream 切换和全局 stream 生命周期；`timelineRowsProvider(sessionId)`、`selectedSessionViewProvider`、`statusBarViewProvider`、`settingsPageProvider` 等 selector 只派生 view model，不直接发起 bridge 调用。`subscribeSessionEvents(sessionId)` 的取消必须跟随选中会话变化，避免后台会话继续接收高频 delta。

Flutter 数据层必须保持编排与归约分离：`StudioController` 只负责桥接 API 调用、订阅生命周期、session load barrier、frame 批处理和 stale recovery 副作用；事件归约、session/config snapshot merge、durable cursor、part overlay 与 agent timeline projection 逻辑放在纯 reducer 模块。纯 reducer 不访问 Riverpod、不调用 bridge、不调度异步任务；需要触发 stale recovery 时只返回明确的 session id 给 controller 执行。

Flutter reducer 必须按 `sessionId` 过滤实时事件，旧 session stream 取消后迟到的事件不得覆盖当前会话。每个 session 维护 durable event cursor；收到 live-only `stale` 时优先调用 `loadStudioEvents(sessionId, afterSequence, limit)` 补拉缺口，补拉事件与实时事件进入同一 reducer。`messagePartDelta` 不推进 durable cursor，但只能追加到已有且未 terminal 的 part 字段。

`StudioState.copyWith` 必须支持对 nullable selection/config 字段显式置空。`selectedProjectId`、`selectedSessionId`、`defaultProviderId` 等字段的 `null` 表示清空领域状态，而不是“保持不变”；需要保持原值时调用方应省略对应参数。

Flutter store 中的 message snapshot、part snapshot、live overlay 与 agent timeline event 是 timeline 的事实源。`TimelineMessage` 是纯 message snapshot，不携带 `parts` 字段；可渲染 `TimelinePart` 只存在于 `TimelineRow` projection/view model 中，reducer 不得把 overlay 后的 `TimelinePart` 再写回 message snapshot，避免 snapshot state 与 projected part 双写不一致。`timelineRowsProvider` 必须按 message `sequence -> createdAt -> id`、part `order -> sequence -> id` 从 `messagesBySession + partSnapshotsBySession + partOverlaysBySession` 派生可渲染 row；`agentTimelineEventsBySession` 中的 `SubAgentActivity` 可按 `callId` 合并 begin/end 后投影为独立 `AgentActivity` row，`TodoListUpdated` 必须按 `eventId` 保留每次更新，不写入 `messagesBySession`，也不伪造 message/part identity。

Part snapshot、part delta、part removal 的 reducer 路径只能写 `partSnapshotsBySession` 与 `partOverlaysBySession`；不得把当前 message list 作为可写参数传入 part reducer，也不得因为 part 更新重排或重写 `messagesBySession`。message list 只由 message snapshot、message removal 和 session snapshot 初始化维护。

FRB JSON bootstrap 与 `load_session_state` 解包时只能写入 message snapshot 表和 part snapshot 表，不能为了方便 UI 渲染把 `timelinePartFromSnapshot` 的结果写回 message snapshot。刷新、重载和实时流必须通过同一个 selector 得到一致的 projected rows。

样例数据、demo API 和测试 fixture 也必须使用 message snapshot + part snapshot 表，或显式的 row projection helper，表达 timeline；不能绕过 selector 在持久 message 上挂载 parts。

Flutter timeline 协议解析必须严格处理枚举值。未知 `partType` 或非空未知 `textChannel` 是协议错误，应直接抛出并暴露给调用方；不得默认降级为 `text` 或 `final`，避免新协议字段被旧 UI 错误展示。

Flutter bridge event 协议解析同样必须严格处理事件类型。实时 stream 使用 FRB `BridgeEventPayload` sealed union；未知或不允许进入 Flutter 的事件是协议错误，应在 FRB/JSON 入口抛出。FRB adapter 必须把事件归一为 typed app `StudioBridgeEventPayload` 后交给 reducer，reducer 不得读取 `payloadJson`/Map 或用 `_ => current` 静默忽略未知事件。`AgentTimelineChanged` 进入 Flutter domain 后也必须保持 typed payload，不得用 string kind + `Map<String, Object?>` 作为 reducer 协议；桥接层如果遇到未知 agent timeline payload，应抛出协议错误而不是生成 generic activity row。`sessionHandoffChanged` 不再作为前端 bridge event 兼容入口；旧 handoff 数据只能通过历史会话列表/查询视图体现，不参与实时 reducer。

`lib/src/rust/**` 与 `frb_generated.dart` 是生成边界，业务代码不得手写修改；`lib/src/data/frb/studio_api.dart` 保持对外稳定 barrel，内部按 `StudioApi` 接口、FRB runtime adapter、typed bridge event、FRB DTO converter、legacy/demo converter 和 demo API 分文件维护。生产路径只从 FRB typed DTO/union 进入 domain model；legacy JSON 解析必须集中在明确命名的 legacy/demo adapter 中，不能混入实时 FRB stream reducer。

`StudioPartType::Turn` 与 `StudioPartType::Inference` 是后端 trace lifecycle synthetic part，只用于恢复 turn/inference 状态与诊断，不是 Studio timeline 可渲染 row。Flutter adapter 在 typed FRB 边界必须过滤历史 snapshot 中的这两类 part，并把实时 `messagePartUpdated` 中的这两类 part 归一为 no-op；其他未知 `partType` 仍应抛出协议错误。

## 3. Timeline Projection

Timeline row 从 `messages + parts + partTextAccumDelta + agentTimelineEvents` 派生，不从 raw event 渲染。Flutter 使用 `TimelineRow` view model 承载 row 类型，`TimelineView` 只消费 rows，不直接消费 message snapshot。row key 只由稳定领域身份组成：

- `user-message:{messageId}`
- `assistant-part:{userMessageId}:{groupKey}`
- `thinking:{userMessageId}`
- `diff-summary:{userMessageId}`
- `bottom-spacer`

`timeline_models.dart` 是对外稳定导出入口，调用方继续通过 `studio_models.dart` 或该入口读取 timeline 类型。内部可按实体模型、agent timeline payload、row projection/grouping/sorting 拆成 `part` 文件，但不得改变公开类型名、构造参数和 projection 函数语义。UI 渲染块也应按消息、runtime progress、tool、plan/agent 和 Markdown renderer 分文件维护，仍由 `TimelineView` 作为唯一 widget 入口消费 `TimelineRow`。

reasoning part 按 opencode 普通 assistant part 处理，参与 `groupParts`，不再把同 turn 的多个 reasoning part 合并成旧 thought entry。这样新 reasoning 的 `messageId + partId` 不会复用旧 row key，也不会把流式 delta 写回旧思考行。

reasoning 默认折叠为“思考中 / 已思考”结构化行，只在 header 展示状态与可选耗时；展开后展示 `StudioPart.text` 中的完整 provider-emitted reasoning summary/thinking stream，并使用与普通 Markdown 相同的 stream-safe 渲染。provider replay metadata、后端诊断和未进入 `StudioPart.text` 的内部推理不进入 timeline 正文。`showReasoningSummaries` 只能控制是否显示 reasoning summary/row，不能把多个 reasoning part 合并成一个旧 thought row。

reasoning 展开/折叠是前端 UI 状态，不写回 `StudioPart` snapshot，也不参与 live overlay。Flutter 以 `sessionId + partId` 为 key 保存展开状态；row 重排、snapshot 刷新或 widget 重建时必须保持同一 reasoning part 的展开状态，切换到其他 session 时不得复用同名 part 的 UI 状态。

text/reasoning 的显示文本读取 `partTextAccumDelta[partId] ?? part.text`。snapshot 到达后以 snapshot 为准并清 overlay；同一 frame 内同 part 的 snapshot 覆盖旧 delta。Flutter reducer 使用 frame callback 批处理 `messagePartDelta`；切换 session 或 durable snapshot 到达前必须先 flush 当前 pending delta。若 snapshot coalescing 替换了 start snapshot，同 part 的 pending delta 进入 stale set 并跳过，避免旧思考 chunk 倒灌到 terminal 文本。

阶段性文本输出使用普通 `text` part，`textChannel=commentary`。start snapshot 创建空 part，delta 追加到 live overlay，terminal snapshot 固化完整文本。即使终态 snapshot 很快到达，前端也必须能在流式期间显示 commentary/final 中间文本；不能把 commentary 合并进 final，也不能把工具后的新文本追加到工具前的 part。
Provider stream adapter 必须在工具输入开始、工具调用就绪或新 step 开始前关闭当前可见文本段；对于 Chat/DeepSeek 这类通过 `<commentary>/<final>` 或未标记文本解析可见输出的 provider，文本段边界也必须在 stream projection 层产生，不能由 `pl-core` 事后补写兜底文本。

`textChannel=commentary` 表示模型主动输出的可见进度，视觉层级应轻于最终答复但完整可读；`textChannel=final` 表示最终答复，必须单独完整展示。OpenAI Responses 原生 `phase=commentary/final_answer/final` 是优先来源；不支持 native phase 的 Chat provider 通过 `<commentary>` 和 `<final>` 标签映射到相同 text part 类型。计划内容不再通过 `<proposed_plan>` 进入 timeline，只能由 `plan_exit.content` 生成 plan part。隐藏 reasoning 不得被当作 commentary 展开。

`source=runtime` 且 `synthetic=true` 的 commentary 表示 Pure 运行时确定性产生的阶段进展，不代表模型输出。Flutter 展示层可以把同一 session/message/turn 内连续 runtime commentary 合并为一个可折叠进展组；分组只影响展示，不合并后端 part，不改变 durable cursor，也不得把模型 commentary 或 final 正文并入该组。

plan、commentary、reasoning 和普通 text 的 live overlay 必须使用 stream-safe Markdown 渲染。Flutter timeline 原生直接使用 `gpt_markdown` 的 `GptMarkdown` widget 渲染，不再包一层兼容 renderer facade。展示前只做轻量 agent repair（CRLF 归一化、CJK 标题补空格、行尾 closing fence 拆行、代码块内 inline closing fence 拆行），不在协议层或 reducer 层改写 Markdown。未闭合 fenced code block、不完整表格和逐字输出期间的临时结构由 renderer 容错展示，不能依赖 Rust/FRB 补全。运行中 delta 可以是不完整 Markdown，但 UI 仍应尽量即时显示列表、标题、表格、代码块等结构；terminal snapshot 到达后清 overlay，并以完整 snapshot 重新渲染。

工具展示使用 Codex/opencode 的 coalesced activity 思路：Studio store 仍保存逐工具 `StudioPart`，后端 turn timeline actor 为每个 assistant tool part 写入 `activityGroupId`，该字段是 Studio projection 元数据，不是模型历史或聚合工具事实。Flutter timeline selector 只按相同 `activityGroupId` 投影为一个默认折叠的 tool group row；不同 `activityGroupId` 即使属于同一 turn/message 也必须拆成多条工具活动 row，从而保留“文本 -> 工具 -> 文本 -> 工具”的阅读节奏。缺少 `activityGroupId` 的 tool part 按单工具组展示，不得按 turn/message 猜测合并。工具组 row 的 `order` 使用组内第一个工具 part 的 order，`sequence/renderVersion` 由组内所有工具 part 的 sequence、revision、status、arguments、result、工作目录、exit code、timeout、拒绝原因和 error 聚合计算；详情列表按 `part.order -> sequence -> id` 排序。工具状态以 part snapshot 的 `status` 为准；展示层不得改写 `StudioPart`。

Todo list 展示使用 Codex `update_plan` 的历史块语义：`update_todo_list` 每次调用生成一条新的 `TodoListUpdated` agent timeline row，标题显示 explanation 或固定 “Todo list”，内容按 pending、inProgress、completed 三态渲染完整快照。completed 项可弱化和删除线展示，inProgress 项高亮，但 row identity 必须使用 `eventId`，不能按 `callId`、agent path 或 turn 折叠。

工具组 header 显示工具数量和聚合状态：存在审批等待时为 `awaitingApproval`，存在 started/streaming/approved/running 时为 `running`，否则按 failed、denied、interrupted、budgetLimited、completed 的优先级折叠。header 中突出失败/拒绝数量；成功工具默认只占这一条折叠 row，展开后展示每个工具的工具名、状态、命令/路径/查询摘要、工作目录、exit code、timeout、拒绝原因和失败结果。工具、命令、文件修改和 subagent 活动文本由 Flutter timeline projection 基于结构化事实确定性生成，不在 `pl-core` 或 `pl-protocol` 中新增本地化文案字段。固定 UI 文案走 i18n；tool 名称、agent path、model slug、路径、命令摘要和 provider 返回值按领域值原样展示。`AgentTimelineChanged` 承载单条 agent timeline 事实：`SubAgentActivity` row identity 优先使用 `callId`，无 `callId` 时使用 event id；`TodoListUpdated` row identity 始终使用 event id。`AgentChanged` 只更新状态栏、agent 列表或活动详情，不应每次 snapshot 都在父 timeline 新增一行。父 timeline 默认不展开子代理内部工具 trace；`spawn_agent`、`wait_agent` 等 tool part 只作为父 turn 工具组详情项展示，不额外生成逐工具 timeline row。

Timeline 虚拟滚动必须监听 opencode 同款 active assistant content version：当前 active assistant message 的完成状态、错误、text/reasoning 展示长度、tool status、tool result/metadata 长度变化都要触发 `virtua.measure()` 和底部锚定。row key 不变但内容增长时，仍要保持底部跟随；切换 session 时写入/读取 row cache，并用 keep-mounted 行避免 active turn 被虚拟列表过早卸载。

Flutter `TimelineRow` 必须携带 `renderVersion`，由 part revision、状态、可见文本、tool arguments/result/workingDirectory/exitCode、agent 状态等会影响布局或内容的字段计算。滚动跟随、pending 新事件和 `ListView` 内容版本比较使用 `renderVersion`，不能只比较文本长度，避免同长度 authoritative replacement 或工具字段变化漏刷新。

Flutter 首版 `ListView.builder` 必须实现同一滚动语义。Timeline 以 `sessionId` 为边界保存滚动状态：用户位于底部附近（`extentAfter <= 80px`）时进入 bottom-following，新消息用短动画贴到底部，streaming 内容增长用 frame-coalesced `jumpTo(maxScrollExtent)` 保持即时跟随；用户向上滚动并离开底部阈值后进入 detached，不再因新消息或 delta 抢占滚动，只累计 pending 新事件。detached 或离底时，阅读流右下方、composer/status bar 上方显示紧凑“跳到最新”悬浮按钮，使用向下箭头图标和 `跳到最新` tooltip；点击后滚到当前会话底部、清空 pending 并恢复 bottom-following。程序滚动必须用内部标记和用户滚动区分，不能把自动滚动误判为用户操作。

## 4. Interaction 与状态栏

普通 prompt、Plan 确认、tool approval、ask-user、legacy session handoff、agent latest snapshot、agent timeline event 和 runtime usage 都以 `sessionId` 为边界。切换会话时用后端当前 session snapshot 替换当前 view；后台 session 事件只更新对应 view，不污染当前 timeline 或状态栏。Plan 确认的实施动作必须留在当前 session 内，不能改变 `selectedSessionId`。

Studio runtime 的恢复语义必须保证 UI 不展示已经无法唤醒的等待态。应用启动时，未完成 turn 标记为取消，`userInput` 与 `toolApproval` 这类依赖内存 waiter 的 transient pending interaction 同步取消并发出 interaction snapshot；`planConfirmation` 可在 turn 完成后继续等待用户决策，因此不会被普通启动恢复或 turn 收尾清理取消。单个 session 的 active turn 只在对应后台 turn 未终止时出现在 runtime snapshot 中，完成、失败、中断和取消后必须从 snapshot 中移除。

聊天底部只渲染一个最高优先级 pending interaction，优先级为 `toolApproval > userInput > planConfirmation`。这个区域采用 opencode dock prompt 语义：pending 的问题与权限请求不写入 timeline view model，timeline 中 pending `request_user_input` / `question` tool part 隐藏，由 dock 显示真实问题、选项和输入控件；完成后的问题 tool part 可以作为普通 assistant tool part 显示“Questions / answered”摘要。普通 prompt 输入不再渲染 Auto/Plan 二级按钮，模式切换只存在于状态栏，避免与状态栏重复。`submit_prompt` 和 `resolve_interaction` 只表示提交成功，不返回最终 timeline；后续展示完全由 Studio event stream 驱动。`toolApproval` 必须显示工具名、参数、工作目录和 approve/deny；`userInput` 必须显示每个问题、选项、free text/other/secret 输入并提交 `{ [questionId]: { answers } }`，secret 答案不得以明文出现在 timeline；`planConfirmation` 保留 implement fresh、continue planning、dismiss 三动作，并和问题/权限一样使用 dock prompt，而不是从 timeline 自行推断“是否实施计划”。

Flutter 的 `planConfirmation` dock 对齐 Codex 桌面 app 的决策式提示：标题固定为“实施此计划？”，计划正文留在 timeline plan card 中展示，dock 内常驻一个轻量调整输入框，用户可直接输入调整要求并提交 `continuePlanning`，不再通过二级按钮跳转到独立 composer 状态；实施动作不回传可编辑计划正文，继续调整只回传用户输入的调整内容，忽略动作保持弱化展示。Flutter 的 `userInput` dock 对齐 Codex 的分题交互：顶部显示问题数量与进度点，当前只聚焦一个问题，选项使用多选 checkbox row，Other/free text/secret 输入跟随当前问题展示，上一题/下一题/提交按钮保留在 dock footer；提交时为每个问题生成 `{ answers: [...] }`，未回答问题也保留空数组。

`userInput` dock 的本地草稿必须以 `interactionId` 与问题结构共同作为重置边界。后端未提供 question id 时，前端用题目 index 生成稳定 key；问题签名必须覆盖问题文案、header、选项内容和 secret/other 状态，避免连续不同问题复用上一题答案。

pending interaction 只替换普通 prompt 输入，不得隐藏当前 turn 的停止控制；只要当前 session 的 turn 仍处于非终态，footer 必须保留停止按钮并调用 `stop_prompt(sessionId)`。`busy` 与停止按钮状态必须按 `sessionId` 归属计算，后台 session 的 turn event 不能让当前 session 显示不可用的停止态。

Flutter 状态栏保留模式切换、planner 模型选择、reasoning effort、context/token/cost、active skills、MCP、LSP 和 subagent 活动列表。模式切换调用 `setSessionMode(sessionId, mode)`，planner 模型选择调用 `setModelRole(roleKey=planner, providerId, model, effort)`，不能只更新本地 chip 或 settings draft。权限模式不在状态栏重复展示，只在 composer 权限选择器和 Settings/Security 中修改。状态栏所有数据来自 Studio store；`mcpHealthChanged` 与 `lspHealthChanged` 必须更新对应 snapshot，不能在 reducer 中丢弃。

Flutter `SessionStatusBar` 展示同一组信息，并使用 Material 3 的 compact controls、tooltip 和 hover/focus 可达的弹层承载详情。Flutter 状态栏只消费 Riverpod selector，不直接订阅 bridge stream 或解析 raw JSON。

Flutter context readout 对齐 Codex 桌面 app 的圆形用量 ring：状态栏中只显示无数字圆形进度，不直接显示 `contextTokens/contextWindow`；hover/focus 详情中展示上下文数字、百分比、总 token 和模型。费用、active skills、MCP、LSP 与 subagent 活动必须继续作为独立状态 chip/readout 展示，不能合并进 context tooltip 后从状态栏消失。

状态栏、interaction dock、timeline 工具/计划/提问摘要中的 UI 文案必须走 i18n；模型名称、provider 名称、模型 slug、tool 名称、agent 路径、reasoning effort 等来自配置或运行时的领域值按原始字符串透传展示，不做翻译或本地化映射。这样 zh-CN/en 只负责固定 UI 标签与状态说明，不改变用户配置、provider 返回值或协议枚举的可辨识性。

状态栏的 waiting 状态以 active interaction 为一等输入。`busy` 表示 turn 是否仍在运行，`activeInteraction` 表示 UI 是否必须等待用户响应；Plan confirmation 可以在 `busy=false` 时仍阻塞 composer。状态栏 phase 优先级为 `toolApproval -> userInput -> planConfirmation -> turnPhase`。

会话列表是独立滚动区域，row 采用 opencode 式单行 flex 布局：图标/状态固定宽度，标题 `min-width:0` 且 `truncate`，列表项 `flex-shrink:0`。Sessions 区域过长时只滚动列表，不挤压 project 区、settings 按钮或相邻 session row。

项目和会话管理继续走 Studio store/runtime API，不能在组件里手动拼接状态。Flutter 使用 `pl-studio-bridge.openProject(path)`，该接口在 `pl-core` 内完成 open project、LSP reconcile、session ensure 和 bootstrap，然后返回新的 project/session/sidebar 快照。打开项目支持两种入口：系统目录选择器和手动路径输入。Flutter 选择项目调用 `selectProject(projectId)`，关闭项目调用 `archiveProject(projectId, selectedProjectId)`，新建会话调用 `createSession(projectId, title)`；所有返回 payload 都必须原子替换 `projects`、当前项目的 `sessions`、`selectedProjectId`、`selectedSessionId`、agent/runtime/interaction 快照，并通过 `sessionRuntime.activeMcpServers/activeLspServers` 恢复状态栏 active 能力；MCP/LSP server catalog 由 config snapshot 与全局 health event 更新。若有 `selectedSessionId`，前端必须立即用 `loadSessionState` 恢复会话历史 projection。若没有选中会话，timeline、状态栏和 composer 显示无会话空态。

项目关闭和会话关闭都是归档语义，不删除磁盘内容、配置或历史会话。Project row 上的关闭按钮调用 `archiveProject(projectId, selectedProjectId)`；关闭当前项目后切换到后端返回的下一个可用项目/会话，关闭最后一个项目后清空当前 selection 并取消 session stream。Session row 上的关闭按钮调用 `archiveSession(sessionId, selectedSessionId)`；后端会拒绝 active turn，会取消该会话 pending interaction，并返回同项目的新 session selection。前端收到 payload 后删除/隐藏归档 session、切换到返回的 `selectedSessionId`，并用 `loadSessionState` 恢复新会话 projection；如果项目内没有剩余 session，状态栏与 composer 禁用，用户可以用新建会话按钮创建会话。会话列表只显示 `visibility=active && parentSessionId=null`，legacy handoff child/archived session 不作为 root row 出现。

Settings 是独立页面栈中的配置编辑入口。它必须覆盖 Providers、Instructions、Skills、Roles、MCP、Security 和 General 页签。Flutter 通过 FRB 读取 bootstrap config 与 runtime snapshot；设置页不提供全局保存、重载或草稿保存按钮。普通设置项改完即保存：Security 权限模式调用 `saveRuntimePermissionMode(mode)`，Roles 模型角色修改调用 `setModelRole`，Instructions 文本停止输入后调用 typed instructions save，Skills 禁用项调用 typed skills save，MCP inline 开关和 endpoint 调用 typed MCP save，General UI 偏好即时写入 Studio store。Provider 新增/编辑是独立页面，保留取消和保存按钮，编辑期间只改本地草稿，点击保存后调用 `saveProviderSettings(settingsJson)` 写回 config。Provider payload 对齐 Studio provider settings wire 格式：`defaultProviderId`、`providers[]`、`roles[]`、provider 的 `id/templateKind/name/baseUrl/bearerToken/defaultModel/providerKind/customModels[]`，以及 model 的 `slug/displayName/reasoningEfforts/baseInstructions`。Skills 页的 Discover 按钮调用 `listDiscoveredSkills(projectId)`，用返回 catalog 刷新当前页的可选 skill 列表。`saveStudioSettingsDraft(section, draftJson)` 只保留兼容旧入口，不作为设置页可生效配置的保存路径。所有 typed save 成功后必须用返回的 canonical config 更新 providers、roles、templates、instructions、skills、MCP servers、permission mode、config TOML 和 config exists 状态。

Provider typed save 返回的 canonical config 必须同步 `defaultProviderId`。保存默认 provider 后列表、卡片和状态栏应立即从 store 反映新默认值，不能等下一次 bootstrap 或页面重载。

Flutter Provider 页采用页面栈式互斥视图：列表页、详情页、新增页和编辑页不得同时显示。列表页提供可搜索 provider 卡片、刷新用量、选择默认 provider 和新增入口；点击详情或编辑进入当前 Provider tab 内的独立页面，顶部提供返回列表和保存/取消操作。新增时从内置模板创建 provider，自动建议不冲突的 id，编辑时支持 provider key、模板类型、显示名、协议类型、base URL、API key、默认模型和自定义模型。Provider 列表必须显示当前默认 provider、credential 状态、模型数量、默认模型、usage 摘要和可用模型 chip；删除 provider 时至少保留一个 provider，并在删除默认 provider 后选择下一个 provider。保存成功后以 bridge 返回的 bootstrap snapshot 归一化刷新 Flutter store，而不是只更新本地 draft。

Settings 不作为悬浮 modal、popover、fixed overlay 或右侧嵌入页展示。Studio shell 采用页面栈语义：chat 页面和 settings 页面互斥，打开设置时压入 settings 页面并替换整个窗口，包括左侧项目/会话栏；设置页顶部提供返回聊天入口，返回后恢复当前会话的 sidebar、timeline、状态栏和 composer。设置页不得模糊、遮罩或覆盖聊天背景，而是作为独立页面参与导航。

Provider 设置支持搜索、刷新用量、选择默认 provider、新增/编辑/删除 provider、切换 provider template、编辑 base URL/API key/default model，以及追加/删除 custom model。Provider 卡片必须消费 `load_provider_usages` 的 typed 结果展示查询状态：打开 Providers 页时自动进行一次过期刷新；全局刷新和单卡刷新都走同一 store action，单卡刷新只在该卡展示 busy/retry 状态，保存 provider 配置后要重新刷新用量，并同步触发 MCP health 刷新。默认 provider 身份来自 config/settings payload 的 `defaultProviderId`，不得用当前详情页、编辑页或列表焦点状态推断。DeepSeek 显示余额与赠送/充值拆分，Zhipu Coding Plan 显示 5 小时、周额度和 MCP 额度的剩余进度、重置时间与完整工具明细；缺 key、失败、不支持、未查询、更新时间和重试入口都必须在卡片内可见。保存 Zhipu Coding Plan token 后，内置 Zhipu MCP 列表和状态栏应随 `mcpHealthChanged` 立即进入 checking/available/unavailable，而不是等待下一轮 prompt。Role 设置固定展示 explorer/planner/executor/reviewer 四个角色，下拉选择后立即写回；provider/model 删除或不可用时规范化到可用 provider/model/effort。MCP 设置支持 stdio 和 streamable HTTP，保留 built-in/locked server metadata，只允许可编辑 server 修改身份；内置 server 的 endpoint 只读、启用开关可用，inline 修改即时保存，内置 server 的启用开关也通过 typed MCP save 写入并立即影响 runtime 暴露，新增或完整编辑 server 若进入独立页面则使用保存/取消模型。Instructions、Security、Skills 和 General 设置不能绕过 store 直接写 UI-only 状态。

Security 页是紧凑的权限配置页，不使用与 provider/MCP 相同的大卡片网格来填充空间。权限模式应作为单个设置组展示：标题、当前状态、三项可选模式和简短说明保持在可扫描的窄宽度内，避免大面积空白。

## 5. 验收目标

- `pure-studio-flutter` 可在 Windows 上 `flutter analyze`、`flutter test`、`flutter build windows`，并通过 FRB 调用 `pl-core` runtime。
- `messagePartDelta` 可以实时显示 text/reasoning/tool/plan 中间输出。
- terminal snapshot 清除 overlay，reload/backfill 与 live terminal UI 收敛。
- 用户一次输入只出现一条用户消息。
- 多个 reasoning part 不复用旧 row，不发生“新思考更新到旧信息上”。
- 真实 UI 回归通过：项目/会话侧栏、输入、流式输出、停止、切换 session、Plan 确认、tool approval、user input、状态栏和全部设置页均可用。

## 6. 视觉与组件约定

聊天页面保持双栏布局：左侧项目/会话栏，右侧主聊天区。设置页面是页面栈中的全窗口页面，不保留聊天侧栏。不得新增常驻右侧环境信息栏；模型、上下文、MCP/LSP 与子代理信息继续由状态栏和弹层承载，权限模式由 composer 中的权限选择器承载。主聊天区采用居中阅读流，timeline 内容宽度由 `--conversation-content-width` 控制，底部 composer/dock 与阅读流对齐。

Pure Studio UI 采用低对比、紧凑、可扫描的桌面工具风格：侧栏背景浅于主内容区，列表项单行截断，当前项目/会话用轻量底色和状态点标识；聊天正文优先可读性，减少装饰性卡片。计划正文在 timeline 中作为计划卡展示，卡片只承载计划内容；计划确认仍属于 footer dock，不从 timeline 自行推断操作。

Flutter 端使用 Material 3 的工具型界面表达同一信息架构：`NavigationRail`/紧凑侧栏承载项目和会话，主区承载 timeline、状态栏和 composer/dock，Settings 作为全窗口页面替换聊天页。Provider、Instructions、Skills、Roles、MCP、Security、General 以 tab 或分段导航组织；Security 页保持紧凑设置组，Provider/MCP 页允许更密集的表单和状态卡。图标按钮优先使用 Material Icons，按钮内文字必须在桌面和窄屏约束下不溢出。

Flutter 主聊天界面视觉应靠拢 Codex 桌面版的工作台气质：中性色浅色主题、低对比侧栏、白色阅读面、单一聚焦 composer 托盘和轻量状态信息行。Timeline 中普通 assistant 正文不使用卡片背景；只有 tool、reasoning、plan、agent 等结构化 part 使用轻边框面板。用户消息使用窄宽度浅色气泡，避免大面积品牌色。状态栏默认只展示当前模式、planner 模型、上下文、费用与活动能力摘要，不重复显示已在模型选择控件中的 runtime model；高频或诊断信息通过 tooltip/popover 承载。

Flutter shell 的二级视觉层级继续收敛：顶部 header 只展示当前会话标题、项目名和短路径，不放大图标或重复品牌；侧栏底部操作使用低饱和按钮，只有发送按钮保留明显主操作色；session row 用 mode 小图标和轻量选中底色表达状态。Composer 的底部控制行承载权限、附件/后续工具入口和发送/停止，输入区域保持单一视觉焦点，不再把状态栏和输入控件混成一排同等权重按钮。

Flutter 交互组件优先使用 Material 3 原生控件，按业务领域组织：shell 负责双栏、header、footer 与侧栏，status 负责状态栏 select/readout/popover，interaction 负责不同 pending interaction 的 dock，timeline 负责消息 part 与计划卡。`MaterialApp.router` 只负责路由和顶层主题组合，业务 wiring 由 Riverpod controller 与 feature widget 承担。

视觉参考以 `output/design` 中的 Pure Studio chat 状态图为准：默认聊天、流式响应、计划确认、环境弹层、select 菜单与窄屏响应式。实现时必须保持低对比侧栏、居中阅读流、底部同宽状态栏与 dock、计划卡渐隐预览、以及窄屏 icon rail，不得新增常驻右侧环境信息栏。

聊天输入框中的权限模式是可交互设置项，使用 Flutter/Material 的紧凑菜单控件调用 `saveRuntimePermissionMode(mode)`，不得退化为静态提示文字，也不得在状态栏重复放置权限选择。状态栏的上下文、费用、能力、子智能体等 readout 使用 Flutter hover/focus popover 或 tooltip 展示详情，鼠标或焦点离开触发器和浮层后必须自动关闭；readout 本身不显示下拉箭头。点击选择只保留给模式、模型和 reasoning effort 这些真正的状态栏菜单控件。

Flutter 窗口 resize 时 UI 不应持续触发昂贵测量。Timeline 的贴底逻辑只在新内容、会话切换和少量后续 layout settle 帧内测量，不能长时间逐帧调用列表 measure 或反复写入 scroll offset。

计划确认 dock 的固定文案语义为“实施此计划？”：主操作是实施计划，次操作是从同一卡片内输入并提交调整要求，忽略动作保持弱化展示。所有固定 UI 文案必须走 i18n；模型名称、provider 名称、tool 名称、agent 路径、reasoning effort 等领域值仍按原始字符串透传。

用户选择“实施此计划”后，当前 session 必须退出 Plan 模式并切回执行用的 Auto 模式，再提交用于实施计划的后台 prompt。前端收到 resolve interaction 响应后要同步 session 列表中的 mode，避免状态栏和会话列表仍显示 plan。
