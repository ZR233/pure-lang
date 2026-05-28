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

主区保持单线 timeline 语义，数据来源统一为：

- 历史 `messages`
- 持久化 `timelineItems`
- 运行中增量 `AgentEvent` 投影
- append-only `agentEvents`

约束：

- 空 assistant 不渲染
- 工具调用使用紧凑行密度
- 子代理使用行内状态，不做嵌套大卡片
- 子代理内部 text delta、thinking delta、tool call 和工具输出不进入父会话 timeline；父会话只展示 agent 生命周期状态与最终摘要
- agent timeline 与 agent latest snapshot 必须分离；timeline 渲染 append-only `agentEvents`，状态栏渲染 latest `agents`
- 同一个 agent 的 spawn、wait、message、close、final status 必须保留为多条 timeline 事件，不能按 agent id 覆盖成一条
- `AgentStateChanged` 只更新 latest snapshot，不直接作为 timeline 数据源
- 用户与 assistant 正文按 Markdown 渲染，支持标题、列表、引用、代码块、行内代码、强调和链接
- 自动跟随最新内容以“用户是否停留在底部”为准；高频 timeline 刷新时仍应在 layout 阶段滚动到最新，用户手动上滚后暂停跟随

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

旧字段别名与旧 payload 解析逻辑在方案乙中删除。

## 5. 选择器与派生数据

从 reducer state 派生：

- `chatItems`
- `selectedProject`
- `selectedSession`
- `activeSubagentCount`
- `turnElapsedMs`

组件通过 selectors 读取，不直接拼装跨域状态。

## 6. 验收目标

- `App.tsx` 目标降至壳层，业务状态迁出
- 事件监听不再直接 `setState`
- 高频事件下 timeline 无断流和错序
- 停止行为稳定收尾为 `interrupted`
