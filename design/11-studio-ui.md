# Pure Studio Timeline UI（方案乙）

## 1. 状态架构

前端状态从分散 `useState` 重构为 reducer 状态机。`App.tsx` 仅保留页面编排，不直接写业务状态。

reducer 分域：

- `bootstrap`
- `session`
- `turn`
- `interaction`
- `settings`
- `timeline`

所有 Tauri 事件只负责 `dispatch(action)`。

## 2. Timeline 视图

主区保持单线 timeline 语义，数据来源统一为 item-first timeline：

- 持久化 `timelineItems`
- 运行中 `TimelineItemStarted`
- 运行中 `TimelineItemDelta`
- 运行中 `TimelineItemCompleted`
- 运行中 `TimelineItemFailed`

约束：

- 空 assistant 不渲染
- `plan` item 渲染为独立计划卡片，正文按 Markdown 展示，不与普通 assistant 消息混排
- 主 timeline 以用户与 assistant 正文为主；工具调用使用紧凑行密度，连续 tool items 可在 selector 中聚合为派生展示项，例如 `Read 3 files · Edit 1 file`，但原始 `timelineItems`、`timelineOrder` 和 reducer 仍保持 item-first append-only 语义
- 工具聚合只属于 selector 展示层：同一 turn 内的 `thinking` 和隐藏 `inference` 不结束当前工具组；assistant `text`、`agent`、`turn` trace 或跨 turn 的 tool item 才开启新的工具展示段。`thinking` 内容仍作为 thought entry 渲染。
- 同一 turn 内连续模型 step 产生的多个 `thinking` item 必须在 selector 中合并为一个 thought entry；tool item 和隐藏 `inference` 对 thought 聚合透明，避免工具循环后在主 timeline 连续出现多张思考卡片。
- 模型调用、普通 turn 状态和 token 用量不作为主 timeline 内容展示；usage 和费用归属底部状态栏及其 popover
- 失败、中断、预算受限等异常 turn trace 可作为低权重 notice 保留在主 timeline，避免关键错误被隐藏
- 子代理使用行内状态，不做嵌套大卡片
- 子代理内部 text delta、thinking delta、tool call 和工具输出不进入父会话 timeline；父会话只展示 agent 生命周期状态、最终摘要、最终错误文本和压缩后的 runtime usage
- agent timeline 与 agent latest snapshot 必须分离；timeline 渲染 append-only `agentEvents`，状态栏渲染 latest `agents`
- 同一个 agent 的 spawn、wait、message、close、final status 必须保留为多条 timeline 事件，不能按 agent id 覆盖成一条
- `AgentStateChanged` 只更新 latest snapshot，不直接作为 timeline 数据源
- 用户与 assistant 正文按 Markdown 渲染，支持标题、列表、引用、代码块、行内代码、强调和链接
- 自动跟随最新内容以“用户是否停留在底部”为准；高频 timeline 刷新时仍应在 layout 阶段滚动到最新，用户手动上滚后暂停跟随
- 同一 session 内继续对话不会触发 `selectedSessionId` 变化；当前轮 `RunPromptResponse.timelineItems` 与实时 `TimelineItem*` 事件必须直接合并进本地 timeline
- 重复选择当前 session 必须视为 no-op，不能清空本地 timeline、agent snapshot、runtime 或 plan 状态；只有切换到不同 session、新建 session 或切换项目时，才可以重置当前会话本地视图并等待对应 timeline snapshot 加载
- 异步 `loadSessionTimeline` 只能替换不晚于本地状态的快照；如果加载结果落后于本地已收到的当前轮 item，只能作为历史补齐合并，不能覆盖当前 timeline
- Tauri 实时事件通道如果发生 broadcast lag，不能静默忽略。事件转发层必须向前端发出当前 session 的 timeline stale 信号；前端收到后立即重新加载该 session 的 timeline，并继续用 `mergeTimelineSnapshot` 的本地新鲜度规则保护已收到的 live item。
- 自动跟随只取决于用户是否停留在底部，不取决于 `isBusy`；命令完成时 `isBusy` 已变为 false，但 `RunPromptResponse.timelineItems` 仍可能追加最终内容
- 用户提交 prompt 后，前端 reducer 立即插入仅本地存在的 optimistic 用户消息 item 和 waiting turn item。waiting item 在 selector 中派生为轻量状态行，用于展示“正在等待模型响应”。这些 item 不持久化、不进入后端 DTO；真实 user text item 到达时只清理 optimistic 用户消息，不能清掉 waiting。waiting 必须持续覆盖 turn start、user text 和 inference start 到首个模型侧可见事件，例如 assistant text、thinking、plan、tool event、inference completed、terminal turn failure 或最终 `RunPromptResponse`。如果 run command 自身失败，waiting 应移除或替换为失败状态，不能残留。
- 用户提交 prompt 属于显式回到底部的动作；即使用户此前停在历史位置，发送后 timeline 也应立即跟随到底部，让 optimistic 消息和等待状态可见。后续 streaming delta 仍遵守“用户在底部才跟随”的规则，用户再次上滚后不抢占滚动位置。
- `thinking` item 在 selector 中派生为 thought entry，并携带 `status`、`startedAt`、`updatedAt` 与 `durationSeconds`。`started/streaming/running` 状态展示思考中动画；`completed` 状态根据 `createdAt/updatedAt` 展示耗时；`failed/interrupted/budgetLimited` 等异常状态展示对应异常语义。多个连续 thinking item 合并时，耗时取最早 `createdAt` 与最晚 `updatedAt`。
- timeline 渲染层可以在 `selectTimelineEntries()` 之后使用 headless 虚拟滚动库承载大列表；虚拟化只负责测量、滚动定位和 DOM 数量控制，不消费 raw `TimelineItem`，也不承载 tool 聚合、thinking 合并、trace 过滤或 plan 行为

前端 reducer 的 timeline 状态固定为：

- `timelineItems: Map<itemId, TimelineItem>`
- `timelineOrder: string[]`

`timelineOrder` 只在 item 首次出现时按 `sequence` 插入；后续 delta/completed/failed 不改变展示位置。组件不得再把 `messages`、运行中 tool map、agent events 和 trace items 临时拼接成主 timeline。工具聚合与普通 trace 过滤只能作为 `timelineEntries` 派生显示层逻辑，不能写回 reducer 状态或后端 DTO。

同一组 `TraceEvent` 在 Tauri snapshot fold 和前端 live reducer 中必须收敛到相同 `TimelineItem`。fold 规则需要能处理 start/delta/completed 的轻微乱序：delta 先到时可以创建最小 item，后续 start/completed 必须补全 `tool/agent/inference/usage` 等结构化字段，不能丢弃已累积的 content、thinking chunks、tool arguments 或 tool result。

左侧 project 列表的每个项目行右侧提供 `X` 归档按钮。归档 project 不删除磁盘目录，也不修改项目文件；它只归档 Studio 中的 project 记录，并清理该 project 下的 sessions、messages、timeline、agent、runtime 和审批历史。用户重新打开同一路径时复用原 project 记录，但历史会话已经清空，应按新项目入口创建默认会话。归档当前 project 时切换到下一个未归档 project；没有剩余 project 时进入无项目状态。

主聊天 timeline 的滚动容器由独立渲染适配层负责。该适配层输入 `TimelineEntry[]`，输出现有 message、plan、thought、tool、tool group、agent 和 trace entry 组件；它必须保持稳定 key、支持可变高度内容重测量，并维持“在底部才自动跟随”的规则。用户上滚阅读历史时，实时 delta 只能显示跳到最新入口，不能抢占滚动位置。

Markdown 阅读样式属于 timeline 展示层：assistant 正文和 plan 卡片正文需要保留舒适内边距、最大阅读宽度、段落间距、列表缩进、引用和代码块层次；用户消息仍保持紧凑气泡，不套用 assistant 的大内边距。移动端和窄窗口下 Markdown 内容必须允许代码块横向滚动，并避免长链接、长路径和行内代码撑破聊天列。

## 3. Turn 生命周期

turn 展示语义固定：

- `idle`
- `running`
- `thinking`
- `tool`
- `agent`
- `approval`
- `stopping`
- `completed`
- `interrupted`
- `failed`

停止按钮触发真实 interrupt，最终状态必须收敛到 `interrupted` 或 `failed`，不能被延迟完成覆盖。

## 4. 命令契约

前端仅消费新 DTO：

- `BootstrapResponse`
- `ProjectSelectionResponse`
- `SessionSelectionResponse`
- `RunPromptResponse`
- `SessionTimelineResponse`

旧字段别名、旧 payload 解析逻辑、旧 text/thinking/tool delta reducer 分支在方案乙中删除。`RunPromptResponse.timelineItems` 与 `SessionTimelineResponse.items` 使用同一个 `TimelineItem` 结构。

## 5. 选择器与派生数据

从 reducer state 派生：

- `timelineEntries`
- `selectedProject`
- `selectedSession`
- `sessionRuntime`
- `activeSubagentCount`
- `turnElapsedMs`

组件通过 selectors 读取，不直接拼装跨域状态。

状态栏固定渲染在聊天底部。聊天顶部标题栏只展示当前会话标题，不展示项目完整路径，避免长路径撑宽主聊天列。左侧展示高频控制：`Auto / Plan` 模式、模型、推理强度和权限模式；右侧展示只读状态：上下文使用量、按货币分组的费用估算、能力数量和 agent latest snapshot。权限模式来自当前配置：请求批准、替我审批或完全访问。Skills 数量和列表表示当前会话已经成功 `skill_view`、内容进入上下文的 skills，不是配置声明，也不是可 discover 的全部 skills。MCP 数量和列表来自后端 MCP runtime registry 当前 `available` server，表示当前会话会向模型暴露的 MCP server。运行中收到 `SkillActivated` 后，前端必须通过后端返回的 `sessionRuntime` 更新当前会话 `activeSkills`；不得从 `skill_view` 的 tool result 文本反解析 skill 状态。设置页的 Skills 标签页则展示当前项目按 discovery 规则发现的只读 skills 列表，并显示每项 scope；该列表不代表当前会话已激活。运行中收到 `AgentRuntimeUpdated`、`SkillActivated` 或 `studio-mcp-health-updated` 后必须即时更新 `sessionRuntime`、MCP health 和对应 agent 的 `runtimeUsage`，不能等 `RunPromptResponse` 返回后才刷新。设置页作为全屏 overlay 打开时必须覆盖聊天状态栏与其 popover，状态栏不得浮到设置页之上。

Provider 设置页的供应商卡片以可扫读的运维状态为主：头部展示供应商名称、key、健康状态和默认路由；卡片信息区展示默认模型、模型数量和额度/余额状态，不展示 base URL。DeepSeek 卡片展示后端查询到的账户余额和币种明细；Zhipu Coding Plan 卡片展示 5 小时、7 天和 MCP 工具额度进度，并在有明细时展示网络搜索、Web Reader、ZRead 等工具用量。普通 Zhipu 和 OpenAI 卡片展示“暂不支持额度查询”的稳定占位，保持列表对齐。额度查询由 Tauri 后端读取当前配置和 API key 后完成，前端只接收脱敏结果；打开 Provider 设置页时刷新一次，用户也可以手动刷新，不能自动定时轮询。

状态栏在窄窗口下保留左侧高频控制，并把右侧只读状态按优先级收入“更多”菜单；更多入口固定跟随左侧控制组显示，避免右侧只读状态挤压时入口也被裁剪。由于桌面布局含左侧项目/会话栏，响应式必须优先按聊天 footer 自身宽度判断，并保留整窗宽度兜底：聊天 footer 约 `1040px` 以下收起能力和子代理，footer 约 `760px` 以下额外收起费用，footer 约 `520px` 以下额外收起上下文。整窗兜底在 `1320px` 以下直接收起费用、能力和子代理，避免不支持 container query 的 WebView2 环境按整窗宽度误判聊天列空间。更多菜单直接展示被收起状态的摘要和详情，不依赖悬浮 popover，必须支持点击、键盘聚焦、外部点击和 `Escape` 关闭，且不得被状态栏横向滚动容器或窗口边界裁剪。

聊天输入区提供 `Auto / Plan` 模式切换，当前值来自选中 session 的 `mode` 并通过后端命令持久化。新会话默认 `auto`。Plan 卡片展示计划正文和轻量状态徽标，不提供“实现计划”按钮。最新计划生成完成后，后端创建 `planConfirmation` interaction，底部普通输入框自动替换为计划确认 composer。确认 composer 必须默认展示可直接输入的继续讨论文本框，并提供三个动作：

- `清空上下文并实现`：通过 `resolve_interaction(interactionId, { type: "planConfirmation", decision: "implementFreshContext" })` 解析当前 interaction。后端创建显式 session handoff：新 Auto session 使用 Codex 风格 handoff prompt 加 plan markdown 作为 fresh context 的唯一意图来源；原计划 session 保持可恢复，但从活跃 session 列表隐藏，避免实施开始后同时出现计划 session 与实施 session。
- `继续讨论`：用户可直接在确认 composer 的输入框写入追问或调整内容；一次提交先通过同一命令解析为 `decision: "continuePlanning"` 且携带 `content`，后端记录 `continuedPlanning`，随后前端立即把同一 `content` 作为普通 prompt 发送，用于继续追问或修改计划，不自动切换到 `auto`。如果 resolution 失败，不发送 prompt。
- `取消`：解析为 `decision: "dismiss"`，后端记录 `dismissed(reason=dismissed)`，关闭确认 composer，恢复普通输入框，不提交任何内容。

计划确认是 `InteractionKind::PlanConfirmation`，不是计划卡片组件的局部状态，也不是前端 reducer 从 timeline 推断的临时 `planAction`。当前 live Plan turn 完成后，后端从本轮最新未处理 `plan` item 创建 pending interaction；历史 timeline 加载不得自动弹出旧计划。`planStates` 按 `planId` 记录后端生命周期 latest state；已有 `accepted`、`implementing`、`implemented`、`implementationFailed`、`continuedPlanning`、`dismissed` 或 `cancelled` 状态的计划不得重新创建确认 interaction。

运行中如果收到 `userInput` pending interaction，聊天底部普通输入框必须被 `AskUserComposer` 替换。该 UI 逐个展示结构化问题，用户可以在问题之间前进和返回；只有最终点击提交时才通过 `resolve_interaction` 一次性回答当前 interaction 并恢复普通 composer。每个问题支持选项和必要的自由输入，选项选择不会立即提交。ask-user 期间用户不能发送新的普通 prompt，但停止按钮仍可中断当前 turn。

设置页 Security 标签页提供会话级权限模式选择。`request-approval` 在 workspace 内直接执行，访问 workspace 外时创建 `toolApproval` interaction；`auto-review` 在 workspace 内直接执行，访问 workspace 外时由 reviewer 模型自动审批，前端不弹用户审批卡片，只通过工具结果展示已批准或已拒绝的事实；`full-access` 明确展示为会放宽 workspace 外文件路径和 shell cwd 边界并直接放行的模式。聊天底部只渲染一个最高优先级 interaction，优先级固定为 `toolApproval > userInput > planConfirmation`。

设置页 MCP 标签页提供结构化 server 配置。列表展示 server id、用户启用状态、实际可用性、来源、传输方式和主要 endpoint；新增和编辑用户 server 使用本地草稿，保存成功后即时写入 `~/.pure/config.toml`、刷新配置 payload 并触发后台健康检查。stdio 表单提供 command、args、env 和可选 cwd；Streamable HTTP 表单提供 url、bearer token 环境变量和 headers。启用且实际可用的 MCP server 被视为用户信任对象，其 tools 在 Auto 和 Plan Mode 中直接暴露，不额外触发审批弹窗。

MCP 标签页必须同时展示后端合成的内置 Zhipu Coding Plan MCP server。内置项显示“内置 / Zhipu Coding Plan”来源、配置状态和实际可用性；缺少 Zhipu Coding Plan 或 Zhipu provider token 时显示缺少 Key 且不开启，检测到 token 后自动恢复启用并进入后台探测。内置项不可删除，server id、transport、endpoint、headers、env 等身份字段只读；保留启用切换按钮，但后端保存后仍以 token 自动恢复开启策略为准。保存 provider API key 后返回的 `ConfigPayload` 必须立即刷新 MCP 配置状态并触发 health update，无需用户再保存 MCP 标签页。

Provider 标签页必须包含 Zhipu Coding Plan 模板。该模板在 UI 中作为独立供应商入口展示，但保存到配置时仍使用 `provider_kind = "zhipu"`，默认 base URL 为 `https://open.bigmodel.cn/api/coding/paas/v4`，默认模型列表与现有 Zhipu 模板完全一致。内置 Zhipu Coding Plan MCP server 的凭据优先使用该模板保存的 `bearer_token`，保存后返回的 `ConfigPayload` 必须立即反映 MCP 状态变化。

工具调用展示遵循 `design/13-tool-calling-runtime.md` 的生命周期语义。工具 entry 和工具组详情必须展示工具名称、状态和关键路径或命令摘要；静默文件工具的成功结果可以隐藏，但 failed、denied、interrupted、budgetLimited 等异常状态必须展示 result/error 详情。前端只做展示派生，不改变 raw `TimelineItem` 的状态或结果内容。实时状态文案必须以 `TimelineItem.status` 为准；`TimelineItemCompleted` 和 `TimelineItemFailed` 只承载最终终态，不能被用来表示 `approved` 这类执行前中间态。

agent latest snapshot、agent timeline event、session runtime 和审批状态必须以当前 `sessionId` 为边界。切换项目、切换会话或新建会话时，前端必须用后端返回的当前会话快照替换本地 agent 列表；运行中收到的实时事件如果属于非当前会话，不能更新当前状态栏、子代理 popover 或 timeline。`RunPromptResponse.agents` 是当前会话完成后的权威快照，不能与旧会话遗留 agents 合并。

实时 `AgentStateChanged` 若未携带 `runtimeUsage`，前端不得清空同一 agent 已有的费用快照；它只更新状态、摘要、错误和其他 latest snapshot 字段。项目/会话切换和 `RunPromptResponse.agents` 仍以当前会话后端完整快照替换本地 agent 列表。

子代理 popover 每行展示角色、任务、状态和该 agent 的费用摘要。存在 token 但缺少价格配置时显示未配置，不把未计价 token 混入任意币种。

失败子代理的 `error` 文本必须优先展示在 timeline 和 latest snapshot UI 中；`reason` 仅用于机器可读分类，不应作为用户可见失败说明的唯一来源。

当子代理因 provider `429` 失败时，UI 仍按失败子代理展示该记录和原始错误文本。429 只改变父 agent 的运行语义：父 agent 会收到可恢复工具结果并继续本地完成任务；Studio 不隐藏、不删除该失败记录。

## 6. 验收目标

- `App.tsx` 目标降至壳层，业务状态迁出
- 事件监听不再直接 `setState`
- 高频事件下 timeline 无断流和错序
- 停止行为稳定收尾为 `interrupted`
