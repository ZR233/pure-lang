# Pure Studio UI

本文约束唯一的 `code/pure-studio` Flutter 桌面端。Flutter 端使用 Material 3 工具型设计、Riverpod 状态管理和 FRB session/global stream，覆盖 Studio 主路径功能。Flutter 视觉在统一事件迁移中保持不变；session stream 直接消费 `pl-protocol::SessionStreamFrame`，不再定义 Studio 私有 timeline 协议。

## 1. 前端框架

Timeline 直接对齐 opencode app：使用 `virtua` 虚拟列表，自写 message/part row algebra、stable row key、row cache、bottom spacer、bottom anchoring 和 jump-to-bottom 交互。允许复制 opencode MIT timeline/UI 子集；复制文件必须保留来源说明，并在仓库 notice 中标注。

`code/pure-studio` 使用 Flutter Windows 桌面端。入口为 `MaterialApp.router`，页面栈由 `go_router` 管理；状态层使用 `flutter_riverpod`，数据入口只允许通过 `pl-studio-bridge`。Flutter UI 使用 Material 3 组件、紧凑桌面工具布局和响应式双栏/窄屏 rail，不做营销页或解释页。首版可以用 `ListView.builder` 实现 timeline，但必须保留 stable key、底部跟随、row cache、streaming Markdown overlay 和会话切换时取消旧订阅的语义；后续可再替换为虚拟列表。

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
- `eventCursor[sessionId]`
- `sessionStatus[sessionId]`
- `interactions[interactionId]`
- `planStates[planId]`
- `agentDirectoryByRoot/sessionRuntimeBySession/agentTimelineEventsBySession`
- `mcpServers/lspServers`
- `turnPhase/turnStartedAt`

大会话与 agent 工作区分层保存：

- `selectedRootSessionId` 决定左侧大会话、标题、project、mode 和 task 全局信息。
- `selectedAgentSessionId` 决定 timeline、Todo、runtime/context、skills、interaction、
  状态栏和 Composer。
- `AgentDirectoryProjection` 是 root 下轻量目录；`AgentWorkspaceProjection` 是当前
  agent session 的唯一工作区事实源。
- Flutter 使用不可变 `AgentWorkspaceView` 聚合当前 agent 的 timeline、Todo、runtime、
  turn phase、interaction、Composer 和状态栏投影，并由单一
  `selectedAgentWorkspaceProvider` 交给 `AgentWorkspacePane`。这些区域不得分别读取
  `StudioState` 后再拼接当前会话。
- child 启动、完成、停止或故障只刷新目录状态，不自动切换当前 agent。

`messageUpdated` upsert message snapshot；message snapshot 保留 `turnId/status/updatedAt/completedAt/error` 等 lifecycle 字段，但 message `createdAt` 首次创建后不可因后续 snapshot 回退或覆盖。message lifecycle 只更新 message snapshot，不驱动 part 终态，也不从 part 状态反推 message 状态。`messagePartUpdated` upsert 完整 part snapshot 并清除该 part 的 live delta；`messagePartDelta` payload 只携带 `partId/revision/field/delta/chunkIndex`，session 归属来自 envelope，message/turn 归属来自已有 part snapshot，不在 delta 内重复携带或信任第二套身份。`messagePartDelta` 只允许命中已有 part，orphan delta 直接丢弃。`messagePartDelta` 不推进 durable cursor，也不得覆盖 terminal snapshot。前端记录 part 的 snapshot sequence、delta sequence 和可选 `chunkIndex`，丢弃同 part stale delta、低序 delta 与重复/倒序 chunk。`messageRemoved`、`messagePartRemoved`、session reset 和 projection snapshot 替换必须清理相关 delta accum。

`StudioPart.revision` 与 `StudioPartDelta.revision` 是每个 part 的 live 版本号。start snapshot 使用 `revision=0`，同 part 每个 delta 递增，terminal snapshot 携带最新 revision。前端 reducer 必须按 `partId + field` 保存 overlay，并在 `delta.revision <= lastRevision` 时丢弃该 delta；terminal snapshot 到达后清理 overlay，terminal 后到达的 delta 一律丢弃。旧历史或旧后端缺失 revision 时按默认 0 读取，只能作为 durable snapshot 初始化；live delta 必须携带大于当前 field revision 的 revision，不能用缺省 0 覆盖已有 snapshot 或 overlay。

snapshot、durable replay 和 live event 都进入同一个 event reducer。`subscribeSessionEvents` 的首帧
原子初始化 projection 或推进 durable cursor；不再通过 `load_studio_events` 建立第二条补拉路径。
前端不得恢复旧 `TimelineItem`、`ConversationEntry` 或 raw trace/agent event 入口。

Flutter 解析层必须接受 Studio 协议内的所有 part type。当前不直接渲染的 lifecycle/internal/file part 可以进入 normalized snapshot 后由 row projection 过滤，或在 bridge payload 层忽略，但不能把协议内类型当未知类型抛出导致 timeline 白屏。真正未知的 part type 仍应 fail fast。

切换或恢复选中 session 时先增加 generation、取消旧 stream，再订阅目标 `sessionId`。PL runtime
已经按“先注册 receiver、再读取 snapshot/replay”建立 load barrier；Flutter 只需原子应用首个
bootstrap frame，再处理同 generation live frame。durable event 不大于 snapshot cursor 时丢弃；
transient delta 不推进 cursor，只按 part revision 处理。旧订阅迟到事件、缺失 sessionId 或
generation 不匹配的 frame 必须丢弃，不能污染当前会话。

状态管理对齐 opencode `global-sync`：Flutter Riverpod store 只保存归一化 entity 表和少量 UI 本地状态，组件不得直接把多个表临时拼成业务状态。选中会话、状态栏、timeline、交互 dock 和会话列表都必须通过 selector/view model 派生：

- `selectedSessionView` 从 `selectedAgentSessionId` 读取当前 owner、message、part、runtime、Todo、interaction、turn phase、busy、MCP/LSP active 列表和 Composer 草稿。
- `visibleProjectSessions` 对 session list 做按 id 去重，只把 `visibility=active` 且 `sessionKind=root` 的大会话放入左侧栏；同一 root 下的 agent session 只出现在标题区 agent 菜单，并按父子层级和创建顺序稳定排列。
- `SessionStatusBar` 只消费 `selectedSessionView.runtime/activeMcpServers/activeLspServers` 与当前 owner 身份，不得直接读后台 session 的 runtime event，也不得聚合其他 agent。

`StudioState` 中 runtime、turn phase 和 Composer 草稿只按 session 归一化保存，不保留一套
可独立写入的 selected-session 镜像。reducer、snapshot 和 controller action 都必须携带明确
`sessionId`；selection 只决定读取哪个 `AgentWorkspaceView`，不能改变事件的归属。

`sessionRuntimeChanged` 只能更新 `sessionRuntimeBySession[sessionId]`；MCP/LSP 的全局 health event 更新 server catalog，当前会话实际 active 列表来自 selected agent session runtime。大会话的 Agent Directory 使用独立轻量事件刷新 owner、session、父子关系、状态和 attention，不通过单 agent session 的 `AgentChanged` 聚合 agent tree。

Flutter Riverpod store 使用同一归一化状态结构：`StudioController` 负责 bootstrap、session stream 切换和全局 stream 生命周期；`timelineRowsProvider(sessionId)`、`selectedSessionViewProvider`、`statusBarViewProvider`、`settingsPageProvider` 等 selector 只派生 view model，不直接发起 bridge 调用。`subscribeSessionEvents(sessionId, afterSequence)` 的取消必须跟随选中会话变化，避免后台会话继续接收高频 delta。

Flutter 数据层必须保持编排与归约分离：`StudioController` 只负责桥接 API 调用、订阅生命周期、bootstrap frame、frame 批处理和 resync 副作用；事件归约、session/config snapshot merge、durable cursor、part overlay 与 agent timeline projection 逻辑放在纯 reducer 模块。纯 reducer 不访问 Riverpod、不调用 bridge、不调度异步任务；需要 resync 时只返回明确原因给 controller 重新订阅。

Flutter reducer 必须按 `sessionId` 过滤实时事件，旧 session stream 取消后迟到的事件不得覆盖当前会话。每个 session 维护 durable event cursor；收到 `ResyncRequired` 时关闭旧 stream 并以无 cursor subscription 获取 authoritative snapshot。`messagePartDelta` 不推进 durable cursor，但只能追加到已有且未 terminal 的 part 字段。

`StudioState.copyWith` 必须支持对 nullable selection/config 字段显式置空。`selectedProjectId`、`selectedSessionId`、`defaultProviderId` 等字段的 `null` 表示清空领域状态，而不是“保持不变”；需要保持原值时调用方应省略对应参数。

Flutter store 中的 message snapshot、part snapshot、live overlay 与 agent timeline event 是 timeline 的事实源。`TimelineMessage` 是纯 message snapshot，不携带 `parts` 字段；可渲染 `TimelinePart` 只存在于 `TimelineRow` projection/view model 中，reducer 不得把 overlay 后的 `TimelinePart` 再写回 message snapshot，避免 snapshot state 与 projected part 双写不一致。`timelineRowsProvider` 必须按 message `sequence -> createdAt -> id`、part `order -> sequence -> id` 从 `messagesBySession + partSnapshotsBySession + partOverlaysBySession` 派生可渲染 row；`agentTimelineEventsBySession` 中的 `SubAgentActivity` 可按 `callId` 合并 begin/end 后投影为独立 `AgentActivity` row。`TodoListUpdated` 继续作为 canonical session 事实保存，但 timeline projection 必须过滤它，Todo selector 只读取当前 agent session 序列中最新的完整 replacement。

应用更新不是 Studio 会话或 canonical snapshot 的组成部分。Flutter 使用独立的 Riverpod
update controller 保存检查、下载、校验和安装状态；该 controller 只通过 typed FRB updater
API 工作，不得向 `StudioState`、session reducer 或 product event stream 写入更新状态。Windows
release 构建在应用启动后静默检查稳定更新，debug 与 demo 构建不得联网。发现更新时只在
Settings/General 与侧栏设置按钮显示低干扰提示，下载和安装必须由用户明确触发。

Part snapshot、part delta、part removal 的 reducer 路径只能写 `partSnapshotsBySession` 与 `partOverlaysBySession`；不得把当前 message list 作为可写参数传入 part reducer，也不得因为 part 更新重排或重写 `messagesBySession`。message list 只由 message snapshot、message removal 和 session snapshot 初始化维护。

FRB JSON bootstrap 与 `load_session_state` 解包时只能写入 message snapshot 表和 part snapshot 表，不能为了方便 UI 渲染把 `timelinePartFromSnapshot` 的结果写回 message snapshot。刷新、重载和实时流必须通过同一个 selector 得到一致的 projected rows。

样例数据、demo API 和测试 fixture 也必须使用 message snapshot + part snapshot 表，或显式的 row projection helper，表达 timeline；不能绕过 selector 在持久 message 上挂载 parts。

Flutter timeline 协议解析必须严格处理枚举值。未知 `partType` 或非空未知 `textChannel` 是协议错误，应直接抛出并暴露给调用方；不得默认降级为 `text` 或 `final`，避免新协议字段被旧 UI 错误展示。

Flutter bridge event 协议解析同样必须严格处理事件类型。实时 stream 使用 FRB
`BridgeSessionStreamFrame` sealed union；未知或不允许进入 Flutter 的事件是协议错误，应在
FRB 入口抛出。FRB adapter 必须把公共事件归一为 typed app payload 后交给 reducer，reducer
不得读取 `payloadJson`/Map 或用 `_ => current` 静默忽略未知事件。Studio-only handoff/task/
session-list 继续只通过 product stream 或查询视图进入，不混入 session stream。

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

工具展示使用 ordered item 上的相邻 coalescing：Studio store 仍保存逐工具 `StudioPart`，
timeline selector 先按 message 与 part order 得到可见阅读流，再单次扫描合并相邻 tool part。
text、commentary、final、reasoning、plan、agent row 或 message 边界立即结束当前工具组；
隐藏 inference 不制造分组边界。`activityGroupId` 只保留为 deprecated wire/数据库兼容字段，
新事件不生成，旧值也不参与展示。这样 `tool, tool, text, tool` 必须投影为两个工具组。
工具组 row 的 `order` 使用组内第一个工具 part 的 order，`sequence/renderVersion` 由组内所有
工具 part 的 sequence、revision、status、arguments、result、工作目录、exit code、timeout、
拒绝原因和 error 聚合计算；详情列表保持扫描顺序。工具状态以 part snapshot 为准，展示层
不得改写 `StudioPart`。所有工具组默认折叠为低对比的单行活动摘要，不使用常驻卡片底色或
边框；摘要行以工具类型图标、工具名称和本地化状态表达实际动作，整行可点击并暴露展开
语义。完成态不显示状态 pill，运行中、待授权、失败或拒绝等需要关注的状态仍应可见。
展开后才按原始顺序展示命令、路径、结果和错误详情；展开状态只属于当前 row widget，
不写回 part snapshot。

Todo list 不进入 timeline。当前 agent snapshot 中最新的 `TodoListUpdated` 是唯一展示值，
保持 runtime 原始顺序，并以 pending、inProgress、completed 三态使用 Material 3 dense
`ListTile` 渲染；不显示分组、数量、比例、预计时间、进度条或统计。宽屏使用右侧可收放面板，
窄屏使用 `endDrawer` 覆盖打开；展开状态按 agent session 保存，首次出现未完成 Todo 时自动
展开一次，用户手动关闭后不重复抢焦点。Todo toggle 只存在于 agent 本地状态栏，不进入
大会话 Header。宽窄判断使用 agent workspace `LayoutBuilder.maxWidth` 和可读 timeline 最小
宽度计算，不使用全局固定 breakpoint；侧栏目标宽度约 300px，空间不足时 drawer 覆盖而不
压缩阅读流。

工具组 header 显示工具数量和聚合状态：存在审批等待时为 `awaitingApproval`，存在 started/streaming/approved/running 时为 `running`，否则按 failed、denied、interrupted、budgetLimited、completed 的优先级折叠。header 中突出失败/拒绝数量；成功工具默认只占这一条折叠 row，展开后展示每个工具的工具名、状态、命令/路径/查询摘要、工作目录、exit code、timeout、拒绝原因和失败结果。工具、命令、文件修改和 subagent 活动文本由 Flutter timeline projection 基于结构化事实确定性生成，不在 `pl-core` 或 `pl-protocol` 中新增本地化文案字段。固定 UI 文案走 i18n；tool 名称、agent path、model slug、路径、命令摘要和 provider 返回值按领域值原样展示。`AgentTimelineChanged` 承载当前 owner 主动执行的单条协作事实：`SubAgentActivity` row identity 优先使用 `callId`，无 `callId` 时使用 event id；`TodoListUpdated` 不生成 row。Agent Directory 事件只更新标题区 agent 列表或 attention，不进入单 agent timeline。父 timeline 默认不展开子代理内部工具 trace；`spawn_agent`、`send_input`、`list_agents`、`close_agent` 等 tool part 只作为父 turn 工具组详情项展示，不额外生成逐工具 timeline row。订阅唤醒输入为 synthetic ephemeral continuation，不生成用户消息或 executor timeline 副本。

Timeline 虚拟滚动必须监听 opencode 同款 active assistant content version：当前 active assistant message 的完成状态、错误、text/reasoning 展示长度、tool status、tool result/metadata 长度变化都要触发 `virtua.measure()` 和底部锚定。row key 不变但内容增长时，仍要保持底部跟随；切换 session 时写入/读取 row cache，并用 keep-mounted 行避免 active turn 被虚拟列表过早卸载。

Flutter `TimelineRow` 必须携带 `renderVersion`，由 part revision、状态、可见文本、tool arguments/result/workingDirectory/exitCode、agent 状态等会影响布局或内容的字段计算。滚动跟随、pending 新事件和 `ListView` 内容版本比较使用 `renderVersion`，不能只比较文本长度，避免同长度 authoritative replacement 或工具字段变化漏刷新。

Flutter 首版 `ListView.builder` 必须实现同一滚动语义。Timeline 以 `sessionId` 为边界保存滚动状态：用户位于底部附近（`extentAfter <= 80px`）时进入 bottom-following，新消息用短动画贴到底部，streaming 内容增长用 frame-coalesced `jumpTo(maxScrollExtent)` 保持即时跟随；用户向上滚动并离开底部阈值后进入 detached，不再因新消息或 delta 抢占滚动，只累计 pending 新事件。detached 或离底时，阅读流右下方、composer/status bar 上方显示紧凑“跳到最新”悬浮按钮，使用向下箭头图标和 `跳到最新` tooltip；点击后滚到当前会话底部、清空 pending 并恢复 bottom-following。程序滚动必须用内部标记和用户滚动区分，不能把自动滚动误判为用户操作。

## 4. Interaction 与状态栏

普通 prompt、Plan 确认、tool approval、ask-user、legacy session handoff、agent latest snapshot、agent timeline event 和 runtime usage 都以 `sessionId` 为边界。切换会话时用后端当前 session snapshot 替换当前 view；后台 session 事件只更新对应 view，不污染当前 timeline 或状态栏。Plan 确认的实施动作必须留在当前 session 内，不能改变 `selectedSessionId`。

root Planner 使用普通 Composer。child agent 默认显示只读 Composer“此 Agent 会话由运行时驱动”，
不允许直接发送普通 prompt；tool approval、user input、plan confirmation 与停止操作仍使用当前
agent 的 interaction dock。切换 agent 时整套 timeline、Todo、状态栏、interaction 与 Composer
同帧切换，Planner 草稿不能显示在 child workspace。

所有 agent（包括 executor）都展示自己的 session timeline 与 Todo，不再把 executor workspace
重定向到 Planner timeline。目标 workspace 尚无缓存时先展示空的 loading workspace；首个
snapshot/replay 到达后再原子替换完整 workspace，期间不得保留上一 agent 的内容。

Studio runtime 的恢复语义必须保证 UI 不展示已经无法唤醒的等待态。应用启动时，未完成 turn 标记为取消，`userInput` 与 `toolApproval` 这类依赖内存 waiter 的 transient pending interaction 同步取消并发出 interaction snapshot；`planConfirmation` 可在 turn 完成后继续等待用户决策，因此不会被普通启动恢复或 turn 收尾清理取消。单个 session 的 active turn 只在对应后台 turn 未终止时出现在 runtime snapshot 中，完成、失败、中断和取消后必须从 snapshot 中移除。

聊天底部只渲染一个最高优先级 pending interaction，优先级为 `toolApproval > userInput > planConfirmation`。普通 prompt 输入不再渲染 Simple/Task 二级按钮，模式切换只存在于状态栏；确认实施后保持 Task 并由 coordinator 推进。

Flutter 的 `planConfirmation` dock 对齐 Codex 桌面 app 的决策式提示：标题固定为“实施此计划？”，计划正文留在 timeline plan card 中展示，dock 内常驻一个轻量调整输入框，用户可直接输入调整要求并提交 `continuePlanning`，不再通过二级按钮跳转到独立 composer 状态；实施动作不回传可编辑计划正文，继续调整只回传用户输入的调整内容，忽略动作保持弱化展示。Flutter 的 `userInput` dock 对齐 Codex 的分题交互：顶部显示问题数量与进度点，当前只聚焦一个问题，选项使用多选 checkbox row，Other/free text/secret 输入跟随当前问题展示，上一题/下一题/提交按钮保留在 dock footer；提交时为每个问题生成 `{ answers: [...] }`，未回答问题也保留空数组。

`userInput` dock 的本地草稿必须以 `interactionId` 与问题结构共同作为重置边界。后端未提供 question id 时，前端用题目 index 生成稳定 key；问题签名必须覆盖问题文案、header、选项内容和 secret/other 状态，避免连续不同问题复用上一题答案。

pending interaction 只替换普通 prompt 输入，不得隐藏当前 turn 的停止控制；只要当前 session 的 turn 仍处于非终态，footer 必须保留停止按钮并调用 `stop_prompt(sessionId)`。`busy` 与停止按钮状态必须按 `sessionId` 归属计算，后台 session 的 turn event 不能让当前 session 显示不可用的停止态。

Flutter 状态栏保留当前 owner 身份、模型、context/token/cost、active skills、MCP 和 LSP。root
workspace 额外提供模式、当前根角色模型和 reasoning effort 选择：Simple 编辑 executor role，
Task 编辑 planner role；任务非终态期间禁用模式切换。child workspace 不显示这些 root 专属
编辑控件，只读展示该 session runtime 的实际模型；runtime 未提供 effort 时不得用根角色配置
冒充。状态栏所有数据来自当前 agent workspace，不显示 agent 数量或其他 agent 列表。

Flutter `SessionStatusBar` 展示同一组信息，并使用 Material 3 的 compact controls、tooltip 和 hover/focus 可达的弹层承载详情。Flutter 状态栏只消费 Riverpod selector，不直接订阅 bridge stream 或解析 raw JSON。

状态栏使用 `LayoutBuilder` 按优先级保留 agent 身份、Todo、mode/model、context 与 turn/
interaction 状态；低优先级的 effort、skills/MCP/LSP、费用进入 overflow menu。不得用水平
滚动把控件藏到视口外，也不得显示大会话 agent 数量。

Flutter context readout 使用紧凑圆形进度环，不显示百分比文字，也不直接显示 `contextTokens/contextWindow`；hover/focus 详情继续使用圆形进度，并展示上下文数字、百分比、总 token 和模型。费用继续作为独立文字 readout；active skills、MCP 与 LSP 进入当前 agent 的能力摘要和分区弹层，不能合并进 context 或费用详情。其他 agent 的状态只在标题区 `n agents` 菜单展示。

状态栏、interaction dock、timeline 工具/计划/提问摘要中的 UI 文案必须走 i18n；模型名称、provider 名称、模型 slug、tool 名称、agent 路径、reasoning effort 等来自配置或运行时的领域值按原始字符串透传展示，不做翻译或本地化映射。这样 zh-CN/en 只负责固定 UI 标签与状态说明，不改变用户配置、provider 返回值或协议枚举的可辨识性。

状态栏的 waiting 状态以 active interaction 为一等输入。`busy` 表示 turn 是否仍在运行，`activeInteraction` 表示 UI 是否必须等待用户响应；Plan confirmation 可以在 `busy=false` 时仍阻塞 composer。状态栏 phase 优先级为 `toolApproval -> userInput -> planConfirmation -> turnPhase`。

状态栏 phase 必须对 `TurnPhase` 与 `InteractionKind` 使用穷尽的本地化映射，不得直接展示协议或 Dart enum 的 `.name`。英文使用自然短语，简体中文使用简洁状态说明；active interaction 的本地化标签仍按上述优先级覆盖 turn phase。

会话列表是独立滚动区域，row 采用 opencode 式单行 flex 布局：图标/状态固定宽度，标题 `min-width:0` 且 `truncate`，列表项 `flex-shrink:0`。Sessions 区域过长时只滚动列表，不挤压 project 区、settings 按钮或相邻 session row。

项目和会话管理继续走 Studio store/runtime API，不能在组件里手动拼接状态。Flutter 使用 `pl-studio-bridge.openProject(path)`，该接口在 `pl-core` 内完成 open project、LSP reconcile、session ensure 和 bootstrap，然后返回新的 project/session/sidebar 快照。打开项目支持两种入口：系统目录选择器和手动路径输入。Flutter 选择项目调用 `selectProject(projectId)`，关闭项目调用 `archiveProject(projectId, selectedProjectId)`，新建会话调用 `createSession(projectId, title)`；所有返回 payload 都必须原子替换 `projects`、当前项目的 `sessions`、`selectedProjectId`、`selectedSessionId`、agent/runtime/interaction 快照，并通过 `sessionRuntime.activeMcpServers/activeLspServers` 恢复状态栏 active 能力；MCP/LSP server catalog 由 config snapshot 与全局 health event 更新。若有 `selectedSessionId`，前端必须立即用 `loadSessionState` 恢复会话历史 projection。若没有选中会话，timeline、状态栏和 composer 显示无会话空态。

项目关闭和会话关闭都是归档语义，不删除磁盘内容、配置或历史会话。Project row 上的关闭按钮调用 `archiveProject(projectId, selectedProjectId)`；关闭当前项目后切换到后端返回的下一个可用项目/会话，关闭最后一个项目后清空当前 selection 并取消 session stream。Session row 上的关闭按钮调用 `archiveSession(sessionId, selectedSessionId)`；后端会拒绝 active turn，会取消该会话 pending interaction，并返回同项目的新 session selection。前端收到 payload 后删除/隐藏归档 session、切换到返回的 `selectedSessionId`，并用 `loadSessionState` 恢复新会话 projection；如果项目内没有剩余 session，状态栏与 composer 禁用，用户可以用新建会话按钮创建会话。会话列表只显示 `visibility=active && parentSessionId=null`，legacy handoff child/archived session 不作为 root row 出现。

Settings 是独立页面栈中的配置编辑入口。它必须覆盖 Providers、Instructions、Skills、Roles、MCP、Security 和 General 页签。App bootstrap/first-run 先调用 `loadProviderCatalog()`，目录只按 revision 做进程内缓存；加载失败显示错误与重试，不回退本地常量。普通设置项改完即保存；Provider 新增/编辑使用独立本地草稿，点击保存后调用 `saveProviderSettings(settingsJson)`。Provider payload 为 `defaultProviderId`、`providers[]`、`roles[]`，实例字段为 `id`、可选 `originalId`、`templateKind`、`wireProtocol`、`connectionMode`、`name`、`baseUrl`、`bearerToken`、`defaultModel`、`customModels[]`；model 字段为 `slug/displayName/reasoningEfforts/baseInstructions`。空 bearer token 保留已存 secret；重命名用 `originalId` 关联原实例。所有 typed save 成功后必须用返回的 canonical config 更新 providers、roles、instructions、skills、MCP servers、permission mode 和 config 状态。

General 页现有单一 settings group 内展示应用版本和稳定更新状态：最新时显示当前版本与
“检查更新”，可升级时显示目标版本、Release Notes 与“下载并安装”，下载时显示进度，
失败时显示可重试错误。活动 turn 或 task 期间安装按钮禁用并说明原因；安装动作在 Bridge
侧再次检查 busy，避免 UI 状态与 runtime 状态竞争。固定更新文案必须进入中英文 i18n。

Provider typed save 返回的 canonical config 必须同步 `defaultProviderId`。保存默认 provider 后列表、卡片和状态栏应立即从 store 反映新默认值，不能等下一次 bootstrap 或页面重载。

Flutter Provider 页采用页面栈式互斥视图：列表页、详情页、新增页和编辑页不得同时显示。列表页提供可搜索 provider 卡片、刷新用量、选择默认 provider 和新增入口；点击详情或编辑进入当前 Provider tab 内的独立页面，顶部提供返回列表和保存/取消操作。新增 preset、协议、模式、凭证标签、模型和 suggested model 全部来自 catalog；相同 preset 可创建多个不同 ID 的实例。多个模式使用动态选项卡，单模式显示只读项；未知 preset/model/icon 使用通用组件。Provider 列表必须显示当前默认 provider、credential 状态、模型数量、当前路由模型、usage 摘要和可用模型 chip。保存成功后以 bridge 返回的 canonical snapshot 归一化刷新 Flutter store，而不是只更新本地 draft。

Settings 不作为悬浮 modal、popover、fixed overlay 或右侧嵌入页展示。Studio shell 采用页面栈语义：chat 页面和 settings 页面互斥，打开设置时压入 settings 页面并替换整个窗口，包括左侧项目/会话栏；设置页顶部提供返回聊天入口，返回后恢复当前会话的 sidebar、timeline、状态栏和 composer。设置页不得模糊、遮罩或覆盖聊天背景，而是作为独立页面参与导航。

Provider 设置支持搜索、刷新用量、选择默认 provider、新增/编辑/删除 provider、切换 provider template、编辑 base URL/API key/default model，以及追加/删除 custom model。Provider 卡片必须消费 `load_provider_usages` 的 typed 结果展示查询状态：打开 Providers 页时自动进行一次过期刷新；全局刷新和单卡刷新都走同一 store action，单卡刷新只在该卡展示 busy/retry 状态，保存 provider 配置后要重新刷新用量，并同步触发 MCP health 刷新。默认 provider 身份来自 config/settings payload 的 `defaultProviderId`，不得用当前详情页、编辑页或列表焦点状态推断。DeepSeek 显示余额与赠送/充值拆分，Zhipu Coding Plan 显示 5 小时、周额度和 MCP 额度的剩余进度、重置时间与完整工具明细；缺 key、失败、不支持、未查询、更新时间和重试入口都必须在卡片内可见。保存 Zhipu Coding Plan token 后，内置 Zhipu MCP 列表和状态栏应随 `mcpHealthChanged` 立即进入 checking/available/unavailable，而不是等待下一轮 prompt。Role 设置固定展示 explorer/planner/executor/reviewer 四个角色，下拉选择后立即写回；provider/model 删除或不可用时规范化到可用 provider/model/effort。MCP 设置支持 stdio 和 streamable HTTP，保留 built-in/locked server metadata，只允许可编辑 server 修改身份；内置 server 的 endpoint 只读、启用开关可用，inline 修改即时保存，内置 server 的启用开关也通过 typed MCP save 写入并立即影响 runtime 暴露，新增或完整编辑 server 若进入独立页面则使用保存/取消模型。Instructions、Security、Skills 和 General 设置不能绕过 store 直接写 UI-only 状态。

Security 页是紧凑的权限配置页，不使用与 provider/MCP 相同的大卡片网格来填充空间。权限模式应作为单个设置组展示：标题、当前状态、三项可选模式和简短说明保持在可扫描的窄宽度内，避免大面积空白。

## 5. 验收目标

- `pure-studio` 可在 Windows 上 `flutter analyze`、`flutter test`、`flutter build windows`，并通过 FRB 调用 `pl-core` runtime。
- `messagePartDelta` 可以实时显示 text/reasoning/tool/plan 中间输出。
- terminal snapshot 清除 overlay，snapshot/replay 与 live terminal UI 收敛。
- 用户一次输入只出现一条用户消息。
- reasoning part 保持各自稳定身份和 revision；Flutter 仅把同一 assistant message 内连续相邻的
  reasoning part 投影为一个稳定展示组，不跨 tool、text、plan、agent 或消息边界合并，也不
  发生“新思考更新到旧 part 上”。
- 真实 UI 回归通过：项目/会话侧栏、输入、流式输出、停止、切换 session、Plan 确认、tool approval、user input、状态栏和全部设置页均可用。

## 6. 视觉与组件约定

聊天页面保持双栏布局：左侧项目/大会话栏，右侧当前 agent 工作区。设置页面是页面栈中的
全窗口页面，不保留聊天侧栏。不得新增常驻环境信息栏；Todo 仅在存在 snapshot 时作为可收放
右侧面板或 drawer，模型、上下文、MCP/LSP 继续由状态栏和弹层承载，权限模式由 composer
中的权限选择器承载。主聊天区采用居中阅读流，底部状态栏和 composer/dock 跟随当前 agent。

Pure Studio UI 采用低对比、紧凑、可扫描的桌面工具风格：侧栏背景浅于主内容区，列表项单行截断，当前项目/会话用轻量底色和状态点标识；聊天正文优先可读性，减少装饰性卡片。计划正文在 timeline 中作为计划卡展示，卡片只承载计划内容；计划确认仍属于 footer dock，不从 timeline 自行推断操作。

Flutter 端使用 Material 3 的工具型界面表达同一信息架构：`NavigationRail`/紧凑侧栏承载项目和会话，主区承载 timeline、状态栏和 composer/dock，Settings 作为全窗口页面替换聊天页。Provider、Instructions、Skills、Roles、MCP、Security、General 以 tab 或分段导航组织；Security 页保持紧凑设置组，Provider/MCP 页允许更密集的表单和状态卡。图标按钮优先使用 Material Icons，按钮内文字必须在桌面和窄屏约束下不溢出。

Flutter 主聊天界面视觉应靠拢 Codex 桌面版的工作台气质：中性色浅色主题、低对比侧栏、白色阅读面、单一聚焦 composer 托盘和轻量状态信息行。Timeline 中普通 assistant 正文不使用卡片背景；plan、agent 等结构化 part 使用轻边框面板，reasoning 与 tool 使用默认折叠的低对比内联摘要，避免高频活动形成连续卡片。用户消息使用窄宽度浅色气泡，避免大面积品牌色。状态栏默认只展示当前模式、planner 模型、上下文、费用与活动能力摘要，不重复显示已在模型选择控件中的 runtime model；高频或诊断信息通过 tooltip/popover 承载。

Timeline 尾部最多突出一个当前活动位。等待批准或运行中的 tool 优先于仍未终态的 reasoning；
没有对应 part 时才按当前 turn phase 展示紧凑阶段占位。活动 reasoning 只显示最新 part 的单行
摘要并随 delta 原地刷新；活动 tool 显示当前工具、命令、搜索词或路径。活动完成后使用同一稳定
身份沉入低对比历史组，不复制第二行。连续 reasoning 历史组折叠为一行，摘要最多展示三个
reasoning 段标题并标记剩余数量，展开后按原 part 顺序显示完整非空 Markdown。reasoning 和 tool
活动行不使用 assistant 头像、卡片背景或常驻状态 pill；失败、拒绝、审批和预算受限仍明确展示
结构化原因。

Flutter shell 的二级视觉层级继续收敛：顶部 Header 明确分为两层，第一层只放大会话标题，
第二层放项目末级名称、分支、Task 阶段、保存/同步状态和唯一 `n agents` compact 状态项；
完整项目路径只放 tooltip，不常驻占据标题区，也不放 Todo 按钮。`n agents` 使用 Material 3
`MenuAnchor`，菜单按父子层级与创建顺序列出状态点、名称、角色、短状态、attention 和选中
标记，支持点击、约 250ms hover、focus 与 Enter/Space。菜单宽高和 anchor alignment 依据
窗口可用区域约束，不使用固定 `360×560` 或负 offset。不得再出现
Planner/Executor/Reviewer 分类横条或第二套 agent 切换控件；底部状态栏显示当前 agent 本地
身份，但不重复 agent 数量。

Studio 采用紧凑控制台密度，但紧凑不等于堆叠入口。面板圆角不得超过 `8px`，阴影只用于 Composer、interaction dock 和 popover；普通设置分组、timeline 结构化行与 Provider 列表使用单层边框，不在卡片中继续嵌套卡片。聊天阅读流、状态区和 Composer 共享同一内容宽度，侧栏与设置导航使用统一布局 token，窗口变窄时按可用宽度切换为 icon rail。

状态栏使用无边框、无常驻底色的紧凑控件和读数。模式、Planner 模型与 reasoning effort 是可点击选择器，只在 hover/focus 时显示轻背景；context 使用无文字圆形进度环，费用、能力与 phase 使用文字读数，并通过 popover 展示详情。Skills、MCP 与 LSP 只保留一个当前 agent 能力摘要入口；agent 目录只保留标题区 `n agents` 入口。header 不再重复显示 phase 或 busy spinner。权限模式仍可在 Composer 快捷切换，Security 页提供完整配置说明，但不得再用第二张“当前模式”卡片重复同一状态。

设置页保持 Providers、Instructions、Skills、Roles、MCP、Security、General 七个领域入口。普通设置使用单层 group + divider；重复实体才使用紧凑列表。Provider 使用紧凑单列列表，整行进入详情，不再额外显示“打开”按钮，默认、刷新、编辑、删除统一进入 row overflow menu。列表只承载可扫描摘要，完整模型、凭据和工具明细留在详情页；Zhipu Coding Plan 例外地在列表中直接按 `fiveHour -> weekly -> mcpMonthly` 展示三条细进度，包括剩余比例和重置时间，缺失的 quota 不得伪造。

Flutter 交互组件优先使用 Material 3 原生控件，按业务领域组织：shell 负责双栏、header、footer 与侧栏，status 负责状态栏 select/readout/popover，interaction 负责不同 pending interaction 的 dock，timeline 负责消息 part 与计划卡。`MaterialApp.router` 只负责路由和顶层主题组合，业务 wiring 由 Riverpod controller 与 feature widget 承担。

视觉参考以 `output/design` 中的 Pure Studio chat 状态图为准：默认聊天、流式响应、计划确认、环境弹层、select 菜单与窄屏响应式。实现时必须保持低对比侧栏、居中阅读流、底部同宽状态栏与 dock、计划卡渐隐预览、以及窄屏 icon rail，不得新增常驻右侧环境信息栏。

聊天输入框中的权限模式是可交互设置项，使用 Flutter/Material 的紧凑菜单控件调用 `saveRuntimePermissionMode(mode)`，不得退化为静态提示文字，也不得在状态栏重复放置权限选择。状态栏的上下文、费用和能力 readout 使用 Flutter hover/focus popover 或 tooltip 展示详情，鼠标或焦点离开触发器和浮层后必须自动关闭；readout 本身不显示下拉箭头。点击选择只保留给模式、模型和 reasoning effort 这些真正的状态栏菜单控件；agent 切换由标题区 Material 3 `MenuAnchor` 独立负责。

Flutter 窗口 resize 时 UI 不应持续触发昂贵测量。Timeline 的贴底逻辑只在新内容、会话切换和少量后续 layout settle 帧内测量，不能长时间逐帧调用列表 measure 或反复写入 scroll offset。

计划确认 dock 的固定文案语义为“实施此计划？”：主操作是实施计划，次操作是从同一卡片内输入并提交调整要求，忽略动作保持弱化展示。所有固定 UI 文案必须走 i18n；模型名称、provider 名称、tool 名称、agent 路径、reasoning effort 等领域值仍按原始字符串透传。

用户选择“实施此计划”后，当前 session 保持 Task 模式并进入 coordinator 实施阶段。状态栏和活动详情展示 task phase、当前分支、agent 交付、merge、冲突和 review 状态；模式选择在任务非终态期间禁用。
这些信息通过 `StudioSessionRuntime.task` 的 typed projection 进入 Flutter，不读取 raw JSON。
活动摘要只显示本地化 task phase；弹层按 coordinator、work unit、merge/conflict 和 review
分区展示 worktree、commit、来源与实际读取的 design 引用。存在 durable task 快照时不再
重复叠加内存 agent 详情面板；长列表在单一、限高的滚动区内展示，760px 窗口不得溢出。
活动摘要中的 durable agent 数只统计 `queued | running | waitingForDelivery`，内存 agent
只统计 `queued | running | waiting`；详情的 durable agent
分区展示全部历史 outcome，包括角色、来源 call、summary、error 和交付 commit，确保摘要
中的活动代理都能在弹层中定位。

Web Search 作为 General 设置中的 typed 配置组展示 mode、context size、allowed domains、country、region、city 和 timezone，并展示后端自动解析的 provider/model 与 `available | disabled | missingCredential | unsupportedModel` 状态。configured mode 与 effective mode 必须同时保留：缺少凭据时显示不可用原因，但不能把用户保存的 `cached/indexed/live` 覆盖为 `disabled`。

timeline 中 `tool.name == "web_search"` 在工具组展开后使用搜索专用详情，不新增数据库 part 类型；折叠态与其他工具保持相同的低对比摘要。搜索详情根据结构化 action 展示搜索词、打开 URL、页内查找 pattern、进行中/完成/失败状态以及 results 中可识别的链接；未知 results 字段保持不透明，不由 Dart reducer 重写。独立搜索与 hosted 搜索必须投影成相同 UI 形状。
