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
- 工具调用使用紧凑行密度
- 子代理使用行内状态，不做嵌套大卡片
- 子代理内部 text delta、thinking delta、tool call 和工具输出不进入父会话 timeline；父会话只展示 agent 生命周期状态、最终摘要、最终错误文本和压缩后的 runtime usage
- agent timeline 与 agent latest snapshot 必须分离；timeline 渲染 append-only `agentEvents`，状态栏渲染 latest `agents`
- 同一个 agent 的 spawn、wait、message、close、final status 必须保留为多条 timeline 事件，不能按 agent id 覆盖成一条
- `AgentStateChanged` 只更新 latest snapshot，不直接作为 timeline 数据源
- 用户与 assistant 正文按 Markdown 渲染，支持标题、列表、引用、代码块、行内代码、强调和链接
- 自动跟随最新内容以“用户是否停留在底部”为准；高频 timeline 刷新时仍应在 layout 阶段滚动到最新，用户手动上滚后暂停跟随

前端 reducer 的 timeline 状态固定为：

- `timelineItems: Map<itemId, TimelineItem>`
- `timelineOrder: string[]`

`timelineOrder` 只在 item 首次出现时按 `sequence` 插入；后续 delta/completed/failed 不改变展示位置。组件不得再把 `messages`、运行中 tool map、agent events 和 trace items 临时拼接成主 timeline。

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

状态栏固定渲染在聊天底部，只展示当前 turn phase、模型、上下文、按货币分组的费用、能力和 agent latest snapshot。Skills 数量和列表表示当前会话已经成功 `skill_view`、内容进入上下文的 skills，不是配置声明，也不是可 discover 的全部 skills。运行中收到 `AgentRuntimeUpdated` 后必须即时用后端聚合快照更新 `sessionRuntime` 和对应 agent 的 `runtimeUsage`，不能等 `RunPromptResponse` 返回后才刷新。设置页作为全屏 overlay 打开时必须覆盖聊天状态栏与其 popover，状态栏不得浮到设置页之上。

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
