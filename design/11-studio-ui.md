# Pure Studio Timeline UI（方案乙）

## 1. 状态架构

前端状态从分散 `useState` 重构为 reducer 状态机。`App.tsx` 仅保留页面编排，不直接写业务状态。

reducer 分域：

- `bootstrap`
- `session`
- `turn`
- `approval`
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
- 异步 `loadSessionTimeline` 只能替换不晚于本地状态的快照；如果加载结果落后于本地已收到的当前轮 item，只能作为历史补齐合并，不能覆盖当前 timeline
- 自动跟随只取决于用户是否停留在底部，不取决于 `isBusy`；命令完成时 `isBusy` 已变为 false，但 `RunPromptResponse.timelineItems` 仍可能追加最终内容

前端 reducer 的 timeline 状态固定为：

- `timelineItems: Map<itemId, TimelineItem>`
- `timelineOrder: string[]`

`timelineOrder` 只在 item 首次出现时按 `sequence` 插入；后续 delta/completed/failed 不改变展示位置。组件不得再把 `messages`、运行中 tool map、agent events 和 trace items 临时拼接成主 timeline。工具聚合与普通 trace 过滤只能作为 `timelineEntries` 派生显示层逻辑，不能写回 reducer 状态或后端 DTO。

## 3. Turn 生命周期

turn 展示语义固定：

- `idle`
- `running`
- `thinking`
- `tool`
- `subagent`
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

状态栏固定渲染在聊天底部，只展示当前 turn phase、模型、上下文、按货币分组的费用、能力、权限模式和 agent latest snapshot。权限模式来自当前配置：请求批准、替我审批或完全访问。Skills 数量和列表表示当前会话已经成功 `skill_view`、内容进入上下文的 skills，不是配置声明，也不是可 discover 的全部 skills。运行中收到成功的 `skill_view` completed event 后，前端必须立即把该 skill 合并到当前会话 `activeSkills`；`RunPromptResponse.sessionRuntime` 仍作为 turn 完成后的最终校准。设置页的 Skills 标签页则展示当前项目按 discovery 规则发现的只读 skills 列表，并显示每项 scope；该列表不代表当前会话已激活。运行中收到 `AgentRuntimeUpdated` 后必须即时用后端聚合快照更新 `sessionRuntime` 和对应 agent 的 `runtimeUsage`，不能等 `RunPromptResponse` 返回后才刷新。设置页作为全屏 overlay 打开时必须覆盖聊天状态栏与其 popover，状态栏不得浮到设置页之上。

聊天输入区提供 `Auto / Plan` 模式切换，当前值来自选中 session 的 `mode` 并通过后端命令持久化。新会话默认 `auto`。Plan 卡片提供“实现计划”动作：点击后先把当前 session 切回 `auto`，再自动提交 `PLEASE IMPLEMENT THIS PLAN:\n\n{plan}`。v1 不提供 fresh thread、清上下文执行或 todo list。

设置页 Security 标签页提供会话级权限模式选择。`request-approval` 在 workspace 内直接执行，访问 workspace 外时使用现有 ApprovalOverlay 弹出用户审批；`auto-review` 在 workspace 内直接执行，访问 workspace 外时由 reviewer 模型自动审批，前端不弹用户审批卡片，只通过工具结果展示已批准或已拒绝的事实；`full-access` 明确展示为会放宽 workspace 外文件路径和 shell cwd 边界并直接放行的模式。

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
