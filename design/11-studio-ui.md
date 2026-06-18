# Pure Studio Solid UI

## 1. 前端框架

`code/pure-studio` 使用 Solid/Vite，不保留 React 运行时、React reducer 或 React 组件双栈。入口为 Solid `render`，业务状态使用 Solid store/signal 组合；Tauri 命令仍沿用现有 Rust DTO，前端只在适配层把 wire DTO 归一化为 Studio store。

Timeline 直接对齐 opencode app：使用 `virtua` 虚拟列表，自写 message/part row algebra、stable row key、row cache、bottom spacer、bottom anchoring 和 jump-to-bottom 交互。允许复制 opencode MIT timeline/UI 子集；复制文件必须保留来源说明，并在仓库 notice 中标注。

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

`messageUpdated` upsert message snapshot；`messagePartUpdated` upsert 完整 part snapshot 并清除该 part 的 live delta；`messagePartDelta` 只允许命中已有 part，orphan delta 直接丢弃。`messagePartDelta` 不推进 durable cursor，也不得覆盖 terminal snapshot。前端记录 part 的 snapshot sequence、delta sequence 和可选 `chunkIndex`，丢弃同 part stale delta、低序 delta 与重复/倒序 chunk。`messageRemoved`、`messagePartRemoved`、session reset 和 projection snapshot 替换必须清理相关 delta accum。

历史、实时和 stale backfill 都进入同一个 event reducer。`load_session_state` 用 projection snapshot 初始化 message/part 与 per-id sequence guard；`load_studio_events(afterSequence)` 只回放 durable envelope。前端不得恢复旧 `TimelineItem`、`ConversationEntry` 或 raw `AgentEvent` 入口。

状态管理对齐 opencode `global-sync`：Solid store 只保存归一化 entity 表和少量 UI 本地状态，组件不得直接把多个表临时拼成业务状态。选中会话、状态栏、timeline、交互 dock 和会话列表都必须通过 selector/view model 派生：

- `selectedSessionView` 从 `selectedSessionId` 读取当前 session、message、part、runtime、agent、interaction、turn phase、busy、MCP/LSP active 列表。
- `visibleProjectSessions` 对 session list 做按 id 去重、过滤 `visibility=active` 且 `parentSessionId` 为空的 root session，并稳定排序，避免 handoff/archived/child session 或重复 DTO 出现在会话栏。Plan implementation 在当前 session 内运行，不创建 target session；legacy child session 即使存在也只可通过历史入口加载，不能作为侧栏 root 项。
- `SessionStatusBar` 只消费 `selectedSessionView.runtime/agents/activeMcpServers/activeLspServers`，不得直接读后台 session 的 runtime event。

`sessionRuntimeChanged` 只能更新 `sessionRuntime[sessionId]`；MCP/LSP 的全局 health event 更新 server catalog，当前会话实际 active 列表来自 selected session runtime。`agentChanged` 是 latest snapshot merge：如果新 snapshot 未携带 `runtimeUsage`，必须保留同 agent 已有的 runtime usage，避免状态变更覆盖 token/cost 信息。

## 3. Timeline Projection

Timeline row 从 `messages + parts + partTextAccumDelta` 派生，不从 raw event 渲染。row key 只由稳定领域身份组成：

- `user-message:{messageId}`
- `assistant-part:{userMessageId}:{groupKey}`
- `thinking:{userMessageId}`
- `diff-summary:{userMessageId}`
- `bottom-spacer`

reasoning part 按 opencode 普通 assistant part 处理，参与 `groupParts`，不再把同 turn 的多个 reasoning part 合并成旧 thought entry。这样新 reasoning 的 `messageId + partId` 不会复用旧 row key，也不会把流式 delta 写回旧思考行。

reasoning 正文默认折叠。运行中的 reasoning 只在 header 显示 `Thinking...` 或从首行推导出的 heading，不自动展开正文；用户手动展开后才显示完整 thinking 文本。`showReasoningSummaries` 只能控制是否显示 reasoning summary/row，不能把多个 reasoning part 合并成一个旧 thought row。

text/reasoning 的显示文本读取 `partTextAccumDelta[partId] ?? part.text`。snapshot 到达后以 snapshot 为准并清 overlay；同一 frame 内同 part 的 snapshot 覆盖旧 delta。若 snapshot coalescing 替换了 start snapshot，同 part 的 pending delta 进入 stale set 并跳过，避免旧思考 chunk 倒灌到 terminal 文本。

阶段性文本输出使用普通 `text` part，`textChannel=commentary`。start snapshot 创建空 part，delta 追加到 live overlay，terminal snapshot 固化完整文本。即使终态 snapshot 很快到达，前端也必须能在流式期间显示 commentary/final 中间文本；不能把 commentary 合并进 final，也不能把工具后的新文本追加到工具前的 part。

plan、commentary 和普通 text 的 live overlay 必须使用 stream-safe Markdown 渲染。运行中 delta 可以是不完整 Markdown，但 UI 仍应显示列表、标题、代码块等结构；terminal snapshot 到达后清 overlay，并以完整 Markdown 重新渲染。

工具展示使用 opencode `groupParts` 语义：普通 part group key 为 `part:{messageId}:{partId}`，连续 context tool 可聚合为 context group，group key 取首个 part id。工具状态以 part snapshot 的 `status` 为准；展示层不得改写 `StudioPart`。

Timeline 虚拟滚动必须监听 opencode 同款 active assistant content version：当前 active assistant message 的完成状态、错误、text/reasoning 展示长度、tool status、tool result/metadata 长度变化都要触发 `virtua.measure()` 和底部锚定。row key 不变但内容增长时，仍要保持底部跟随；切换 session 时写入/读取 row cache，并用 keep-mounted 行避免 active turn 被虚拟列表过早卸载。

## 4. Interaction 与状态栏

普通 prompt、Plan 确认、tool approval、ask-user、legacy session handoff、agent latest snapshot、agent timeline event 和 runtime usage 都以 `sessionId` 为边界。切换会话时用后端当前 session snapshot 替换当前 view；后台 session 事件只更新对应 view，不污染当前 timeline 或状态栏。Plan 确认的实施动作必须留在当前 session 内，不能改变 `selectedSessionId`。

聊天底部只渲染一个最高优先级 pending interaction，优先级为 `toolApproval > userInput > planConfirmation`。这个区域采用 opencode dock prompt 语义：pending 的问题与权限请求不写入 timeline view model，timeline 中 pending `request_user_input` / `question` tool part 隐藏，由 dock 显示真实问题、选项和输入控件；完成后的问题 tool part 可以作为普通 assistant tool part 显示“Questions / answered”摘要。普通 prompt 输入不再渲染 Auto/Plan 二级按钮，模式切换只存在于状态栏，避免与状态栏重复。`submit_prompt` 和 `resolve_interaction` 只表示提交成功，不返回最终 timeline；后续展示完全由 `studio-runtime-event` 驱动。`toolApproval` 必须显示工具名、参数、工作目录和 approve/deny；`userInput` 必须显示每个问题、选项、free text/other/secret 输入并提交 `{ [questionId]: { answers } }`，secret 答案不得以明文出现在 timeline；`planConfirmation` 保留 implement fresh、continue planning、dismiss 三动作，并和问题/权限一样使用 dock prompt，而不是从 timeline 自行推断“是否实施计划”。

pending interaction 只替换普通 prompt 输入，不得隐藏当前 turn 的停止控制；只要当前 session 的 turn 仍处于非终态，footer 必须保留停止按钮并调用 `stop_prompt(sessionId)`。`busy` 与停止按钮状态必须按 `sessionId` 归属计算，后台 session 的 turn event 不能让当前 session 显示不可用的停止态。

Solid `SessionStatusBar` 保留旧 React 状态栏功能中的模式切换、planner 模型选择、reasoning effort、context/token/cost、active skills、MCP、LSP 和 subagent 活动列表。权限模式不在状态栏重复展示，只在 composer 权限选择器和 Settings/Security 中修改。状态栏所有数据来自 Studio store；`mcpHealthChanged` 与 `lspHealthChanged` 必须更新对应 snapshot，不能在 reducer 中丢弃。

状态栏、interaction dock、timeline 工具/计划/提问摘要中的 UI 文案必须走 i18n；模型名称、provider 名称、模型 slug、tool 名称、agent 路径、reasoning effort 等来自配置或运行时的领域值按原始字符串透传展示，不做翻译或本地化映射。这样 zh-CN/en 只负责固定 UI 标签与状态说明，不改变用户配置、provider 返回值或协议枚举的可辨识性。

状态栏的 waiting 状态以 active interaction 为一等输入。`busy` 表示 turn 是否仍在运行，`activeInteraction` 表示 UI 是否必须等待用户响应；Plan confirmation 可以在 `busy=false` 时仍阻塞 composer。状态栏 phase 优先级为 `toolApproval -> userInput -> planConfirmation -> turnPhase`。

会话列表是独立滚动区域，row 采用 opencode 式单行 flex 布局：图标/状态固定宽度，标题 `min-width:0` 且 `truncate`，列表项 `flex-shrink:0`。Sessions 区域过长时只滚动列表，不挤压 project 区、settings 按钮或相邻 session row。

项目和会话管理继续走 Studio store 的 Tauri command 入口，不能在组件里手动拼接状态。打开项目支持两种入口：系统目录选择器和手动路径输入；两者都调用 `open_project(path)`，返回的 `ProjectSelectionPayload` 是新的 project/session/sidebar 快照。选择项目调用 `select_project(projectId)`，归档/关闭项目调用 `archive_project(projectId, selectedProjectId)`；后端会拒绝仍有 active turn 的项目，并在成功后返回下一个可选项目或空项目状态。前端收到项目选择 payload 后必须替换 `projects`、当前项目的 `sessions`、`selectedProjectId`、`selectedSessionId`、agent/runtime/interaction/MCP/LSP health；若没有选中会话，timeline、状态栏和 composer 显示无会话空态。

会话关闭是归档语义，不删除磁盘或非对话配置。Session row 上的关闭按钮调用 `delete_session(sessionId, selectedSessionId)`；后端会拒绝 active turn，会取消该会话 pending interaction，并返回同项目的新 session selection。前端收到 payload 后删除/隐藏归档 session、切换到返回的 `selectedSessionId`，并用 `load_session_state` 恢复新会话 projection；如果项目内没有剩余 session，状态栏与 composer 禁用，用户可以用新建会话按钮创建会话。会话列表只显示 `visibility=active && parentSessionId=null`，legacy handoff child/archived session 不作为 root row 出现。

Settings 是 Solid store 的配置编辑入口，不恢复 React 兼容层。它必须对齐旧 React 设置页的能力：Providers、Instructions、Skills、Roles、MCP、Security 和 General 页签。配置状态来自 `ConfigPayload` 与现有 Tauri command：`load_provider_usages`、`save_provider_settings`、`save_instructions_settings`、`save_mcp_settings`、`save_permission_mode` 和 `list_discovered_skills`。设置页本地 UI 状态包括 active tab、provider search、selected provider、provider usage loading/error；保存成功后必须用返回的 canonical `ConfigPayload` 更新 providers、roles、templates、instructions、MCP servers、permission mode、config TOML 和 config exists 状态。

Settings 不作为悬浮 modal、popover、fixed overlay 或右侧嵌入页展示。Studio shell 采用页面栈语义：chat 页面和 settings 页面互斥，打开设置时压入 settings 页面并替换整个窗口，包括左侧项目/会话栏；设置页顶部提供返回聊天入口，返回后恢复当前会话的 sidebar、timeline、状态栏和 composer。设置页不得模糊、遮罩或覆盖聊天背景，而是作为独立页面参与导航。

Provider 设置支持搜索、刷新用量、选择默认 provider、新增/编辑/删除 provider、切换 provider template、编辑 base URL/API key/default model，以及追加/删除 custom model。Provider 卡片必须消费 `load_provider_usages` 的 typed 结果展示查询状态：打开 Providers 页时自动进行一次过期刷新；全局刷新和单卡刷新都走同一 store action，单卡刷新只在该卡展示 busy/retry 状态，保存 provider 配置后要重新刷新用量。DeepSeek 显示余额与赠送/充值拆分，Zhipu Coding Plan 显示 5 小时、周额度和 MCP 额度的剩余进度、重置时间与完整工具明细；缺 key、失败、不支持、未查询、更新时间和重试入口都必须在卡片内可见。Role 设置固定展示 explorer/planner/executor/reviewer 四个角色，并在 provider/model 删除或不可用时规范化到可用 provider/model/effort。MCP 设置支持 stdio 和 streamable HTTP，保留 built-in/locked server metadata，只允许可编辑 server 修改身份；保存前清理空 args/env/headers。Instructions、Security 和 Skills 设置分别编辑提示词配置、权限模式和当前项目 skill discovery，不能绕过 store 直接写 UI-only 状态。

Security 页是紧凑的权限配置页，不使用与 provider/MCP 相同的大卡片网格来填充空间。权限模式应作为单个设置组展示：标题、当前状态、三项可选模式和简短说明保持在可扫描的窄宽度内，避免大面积空白。

## 5. 验收目标

- `pure-studio` 构建不依赖 React。
- `messagePartDelta` 可以实时显示 text/reasoning/tool/plan 中间输出。
- terminal snapshot 清除 overlay，reload/backfill 与 live terminal UI 收敛。
- 用户一次输入只出现一条用户消息。
- 多个 reasoning part 不复用旧 row，不发生“新思考更新到旧信息上”。
- 真实 UI 回归通过：输入、流式输出、停止、切换 session、Plan 确认和 tool approval 均可用。

## 6. 视觉与组件约定

聊天页面保持双栏布局：左侧项目/会话栏，右侧主聊天区。设置页面是页面栈中的全窗口页面，不保留聊天侧栏。不得新增常驻右侧环境信息栏；模型、上下文、MCP/LSP 与子代理信息继续由状态栏和弹层承载，权限模式由 composer 中的权限选择器承载。主聊天区采用居中阅读流，timeline 内容宽度由 `--conversation-content-width` 控制，底部 composer/dock 与阅读流对齐。

Pure Studio UI 采用低对比、紧凑、可扫描的桌面工具风格：侧栏背景浅于主内容区，列表项单行截断，当前项目/会话用轻量底色和状态点标识；聊天正文优先可读性，减少装饰性卡片。计划正文在 timeline 中作为计划卡展示，卡片只承载计划内容；计划确认仍属于 footer dock，不从 timeline 自行推断操作。

前端交互组件统一使用 `@ark-ui/solid` 作为唯一 headless 组件库，并在业务语义组件中直接使用 Ark primitives；不再保留 `src/components/ui` 通用 wrapper、Kobalte 兼容层或 native select fallback。普通按钮、列表、卡片和 dock 直接使用语义 HTML 与项目 CSS class 表达。组件拆分按业务领域组织：shell 负责双栏、header、footer 与侧栏，status 负责状态栏 select/readout/popover，interaction 负责不同 pending interaction 的 dock，timeline 负责消息 part 与计划卡。`App.tsx` 只负责 store 初始化、selector、action wiring 与顶层组合。

视觉参考以 `output/design` 中的 Pure Studio chat 状态图为准：默认聊天、流式响应、计划确认、环境弹层、select 菜单与窄屏响应式。实现时必须保持低对比侧栏、居中阅读流、底部同宽状态栏与 dock、计划卡渐隐预览、以及窄屏 icon rail，不得新增常驻右侧环境信息栏。

聊天输入框中的权限模式是可交互设置项，使用 Ark Select 直接绑定 `save_permission_mode`，不得退化为静态提示文字，也不得在状态栏重复放置权限选择。状态栏的上下文、费用、能力、子智能体等 readout 使用 Ark HoverCard 展示详情，鼠标或焦点离开触发器和浮层后必须自动关闭；readout 本身不显示向下箭头。点击选择只保留给模式、模型和 reasoning effort 这些真正的状态栏 select 控件。

Tauri 窗口 resize 时 UI 不应持续触发昂贵测量。Timeline 的贴底逻辑只在新内容、会话切换和少量后续 layout settle 帧内测量，不能长时间逐帧调用虚拟列表 measure 或反复写入 scrollTop。

计划确认 dock 的固定文案语义为“实施此计划？”：主操作是实施计划，次操作是继续调整，忽略动作保持弱化展示。所有固定 UI 文案必须走 i18n；模型名称、provider 名称、tool 名称、agent 路径、reasoning effort 等领域值仍按原始字符串透传。

用户选择“实施此计划”后，当前 session 必须退出 Plan 模式并切回执行用的 Auto 模式，再提交用于实施计划的后台 prompt。前端收到 resolve interaction 响应后要同步 session 列表中的 mode，避免状态栏和会话列表仍显示 plan。
