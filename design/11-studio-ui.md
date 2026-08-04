# 11 - Pure Studio UI

## 11.1 UI 边界

Pure Studio 是 Flutter Windows 桌面应用，使用 Material 3、Riverpod、`go_router` 和 typed
FRB。UI 只能通过 bridge 访问 StudioRuntime，不读取 SQLite 或配置文件。

Flutter 分层保持简单：

- data：FRB service 和 DTO → domain 转换；
- domain：不可变 `ThreadWorkspace`、`WorkspaceUiState` 与纯 selector；
- UI：timeline、interaction、status、settings 等 feature widget。

Controller 负责命令和订阅生命周期，不承担 timeline 投影。Widget 只读取 view model，不直接
订阅 bridge 或拼接多个事实表。

## 11.2 Canonical state

Flutter canonical state 只有：

```text
ThreadDirectory
workspacesByThread: Map<ThreadId, ThreadWorkspace>
selectedThreadId
projects / tasks / settings / health
```

`ThreadWorkspace` 包含 Thread、最近 Turn 与有序 Item、pending Interaction 和
`ThreadRuntimeSnapshot`。authoritative snapshot 总是整体替换一个 workspace；不得把 snapshot
与旧 runtime、旧 Turn 或旧 Item overlay 混合。

以下都是 UI 本地状态，放入按 Thread 隔离的 `WorkspaceUiState`：

- Composer draft、提交阶段和 submission revision；
- 滚动、bottom-following 与未读计数；
- reasoning/tool/Todo 展开状态；
- subscription generation 与临时 delta overlay。

只保存 `selectedThreadId`。root、parent 和 child 关系从 Thread 字段派生。切换 Thread 时
timeline、Todo、状态栏、interaction 和 Composer 同帧切换；未加载目标时显示空 loading
workspace，不能残留上一个 Thread 的内容。

## 11.3 订阅与增量

选择 Thread 时增加本地 generation 并立即建立新订阅。服务端先注册监听，再返回
authoritative `ThreadSnapshot`，随后发送：

- `TurnStarted / TurnUpdated / TurnCompleted`
- `ItemStarted / ItemDelta / ItemCompleted`
- `InteractionChanged`
- `ThreadRuntimeUpdated`
- `Lagged`

snapshot 直接覆盖对应 workspace。只接收相同 ThreadId 和 generation 的通知；旧订阅迟到内容
直接丢弃。Item/Turn terminal 与 transcript delta 是 lossless；收到 `Lagged`、断流或未知
revision 后重新订阅取 snapshot，不维护 durable cursor 或 replay journal。

历史通过 opaque keyset cursor 调用 `listThreadTurns` 向前分页。历史页只补充更旧的 Turn，
不能覆盖 live snapshot 中相同身份的新 revision。

## 11.4 Timeline

Timeline 直接按 Item ordinal 排序。Item ordinal 首次插入后不可改变。可见类型为：

- user message；
- commentary 和 final agent message；
- reasoning；
- plan；
- tool call；
- file。

`contextCompaction` 和 provider 私有内容不进入 Flutter。Todo、usage、context、capability 和
progress 来自 runtime snapshot，不伪装成 timeline row。

工具相邻合并和连续 reasoning 折叠只属于 `TimelineRow` 视觉投影，不能改写 Item。commentary
是明确的 agentMessage Item；普通 runtime progress 只显示在状态区域，不生成或折叠伪 timeline
消息。text、plan、reasoning 和 tool delta 只写当前 Item overlay；terminal Item 到达后清除
overlay并以完整 payload 为准。

Timeline row key 使用 `threadId + itemId` 或稳定工具组首 Item identity。未知 Item union 变体
是协议错误；已知但内部不可见的变体由 Rust bridge 过滤。

Markdown 使用 `GptMarkdown` 容错渲染流式不完整内容。修复只处理换行和 fenced code 的显示，
不得在协议或 reducer 中改写正文。

## 11.5 Composer 与 Interaction

Composer 状态按 Thread 保存：`idle | submitting | pendingStart`。提交冻结当前 draft 并递增
revision；只有同 Thread、同 revision 的响应能清空或恢复 draft。服务端 Turn receipt 与订阅中
同一 Turn 对上后才解除 pending gate。

root Thread 可提交普通输入；child Thread 默认只读，但 pending interaction 与停止操作仍可用。
footer 同时最多显示一个 pending interaction。优先级为 tool approval、user input、plan
confirmation。计划正文在 timeline 的 plan Item 中，确认 dock 只承载实施、继续调整和忽略。

只要当前 Turn 未终态，停止按钮始终可用。`busy` 只由当前 Thread 的 active Turn 决定；pending
plan confirmation 可以在 `busy=false` 时继续阻塞普通 Composer。

## 11.6 状态栏与 agent directory

状态栏只读取当前 workspace 的 owner、模型、context/token/cost、skills、MCP、LSP 和 Todo。
root 额外显示 Simple/Task 模式与角色模型；活动 Task 期间禁止切换模式。child 只读展示实际
运行模型。

agent directory 只在 header 的单一菜单中展示 root/child 层级、role、status 和 attention。
child 的 timeline 不复制到 root，父 Thread 的 agent control tool 只作为自己的 toolCall Item。

Turn phase 只在 timeline 尾部的一个活动块显示：preparing、thinking、responding、planning、
runningTool、waitingInteraction、persisting。终态后移除活动块；失败和中断由 Turn 终态展示。

## 11.7 Product stream 与恢复

product stream 只负责 Project、Thread directory、Task、settings 和 health。选中 Thread 的高频
内容只来自 thread stream，两种 stream 不共享 sequence。

SQLite/schema/Bridge 无法提供 canonical snapshot 是应用级致命错误，显示可重试错误页。单个
Project、Task 或 worktree 故障是 typed recovery issue：健康内容继续可用，故障项显示错误与
安全清理入口。

清理必须 preview → 用户确认 → 执行时重新验证。UI 展示 path、branch、dirty、ahead、变更
数量和 expected revision；不得在确认前写入，也不得允许清理用户主工作区。

## 11.8 设置与视觉

Settings 是独立页面，覆盖 Providers、Instructions、Skills、Roles、MCP、Security、General。
所有保存采用 typed command，并用 bridge 返回的 canonical settings snapshot 替换本地状态；
secret 使用 preserve/replace/clear enum，不解析错误消息或 raw JSON 控制流程。

聊天页保持低对比双栏桌面布局：左侧 Project/root Thread，右侧当前 Thread workspace；窄屏改为
icon rail。普通 agent 正文无卡片背景，plan 使用轻边框，reasoning/tool 默认折叠。Composer、
状态栏和阅读流同宽。设置页替换整个聊天页，不作为悬浮层。

## 11.9 验收

- Item timeline、reasoning、tool grouping、Composer revision 和 interaction dock 有 widget test；
- root/child 切换时 canonical workspace 与 UI ephemeral 状态均正确隔离；
- lag、断流和旧 generation 不污染当前 workspace；
- Flutter analyze、widget/integration tests 通过；
- Windows native Driver 使用真实 Bridge，关闭 frame sync，验证输入 read-back、SQLite 状态、
  绝对路径截图和零 runtime error。
