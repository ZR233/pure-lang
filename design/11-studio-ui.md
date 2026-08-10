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

Flutter canonical state 不保存 authoritative `AgentWorkspace`、绝对 worktree path、workspace
boundary 或 mutability。它们是 Rust turn/runtime 根据 durable owner 解析的执行边界；UI 只消费
Task DTO 中经过脱敏的相对 locator 和可展示状态，不能缓存后参与工具路由。

`ThreadWorkspace` 包含 Thread、最近 Turn 与有序 Item、pending Interaction 和
`ThreadRuntimeSnapshot`。authoritative snapshot 总是整体替换一个 workspace；不得把 snapshot
与旧 runtime、旧 Turn 或旧 Item overlay 混合。

Thread directory 唯一拥有 mode、role、title、关系、status 与 updatedAt。`ThreadWorkspace`
中的 Thread 是归一化后的 directory 引用，不是第二份事实源：thread snapshot 只能替换 Turn、
Item、Interaction 和 runtime，并把该引用重绑到当前 directory entry，不能覆盖 directory。

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

snapshot 直接覆盖对应 workspace 的实时内容，但保留 product stream 已确认的 Thread directory
元数据。只接收相同 ThreadId 和 generation 的通知；旧订阅迟到内容直接丢弃。Item/Turn terminal
与 transcript delta 是 lossless；收到 `Lagged`、断流或未知 revision 后重新订阅取 snapshot，
不维护 durable cursor 或 replay journal。

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
存在模型 usage 时，上下文读数同时显示该 Thread 的缓存命中率；详情展示缓存命中、未命中、
写入、reasoning token、inference 数、按币种估算费用、缓存节省和部分未定价提示。读数只来自
canonical `ThreadRuntimeSnapshot`，不在 Flutter 建立逐 inference 或计费副本。
root 通过 typed mode selector 切换 Simple/Task，并展示对应角色模型；Bridge 返回的 canonical
Thread 状态确认切换结果。活动 Task 期间 selector 保持可见但禁用。child 只读展示实际运行模型。
root-only 和活动 Task 锁定同时由 StudioRuntime 校验，不能只依赖 Widget 或 Controller 拦截。
模式切换只允许 actor idle 且没有 pending input；StudioRuntime 必须先持久更新 ThreadActor role，
再提交 mode/role 目录记录，任一步失败时补偿回旧 role，不能留下数据库与进程内身份分叉。

agent directory 只在 header 的单一菜单中展示 root/child 层级、role、status 和 attention。
child 的 timeline 不复制到 root，父 Thread 的 agent control tool 只作为自己的 toolCall Item。

Turn phase 只在 timeline 尾部的一个活动块显示：preparing、thinking、responding、planning、
runningTool、waitingInteraction、persisting。终态后移除活动块；失败和中断由 Turn 终态展示。

## 11.7 Product stream 与恢复

product stream 只负责 Project、Thread directory、Task、settings 和 health。选中 Thread 的高频
内容只来自 thread stream，两种 stream 不共享 sequence，也不能按到达顺序互相覆盖。directory
更新可重绑 workspace 的 Thread 引用，但不能改写 workspace 的 Turn、Item、Interaction 或 runtime。

启动发现 SQLite 版本、结构或 fingerprint 不兼容时，store 先关闭检查连接，精确删除配置的
数据库/WAL/SHM 并创建空 canonical schema。重建成功后 Bridge 返回正常空 snapshot，GUI 直接
进入可用空状态；只有数据库文件被占用、删除/初始化失败，或 SQLite/Bridge 仍无法提供 canonical
snapshot 时才是应用级致命错误并显示可重试错误页。单个 Project、Task 或 worktree 故障是
typed recovery issue：健康内容继续可用，故障项显示错误与安全清理入口。

清理必须 preview → 用户确认 → 执行时重新验证。UI 展示 path、branch、dirty、ahead、变更
数量和 expected revision；不得在确认前写入，也不得允许清理用户主工作区。

## 11.8 设置与视觉

Settings 是独立页面，覆盖 Providers、Instructions、Skills、Roles、MCP、Security、General。
所有保存采用 typed command，并用 bridge 返回的 canonical settings snapshot 替换本地状态；
secret 使用 preserve/replace/clear enum，不解析错误消息或 raw JSON 控制流程。
Skills 页在每次变为 active tab 时自动重新发现项目技能列表，用最新快照替换缓存而非累加。

聊天页保持低对比双栏桌面布局：左侧 Project/root Thread，右侧当前 Thread workspace；窄屏改为
icon rail。普通 agent 正文无卡片背景，plan 使用轻边框，reasoning/tool 默认折叠。Composer、
状态栏和阅读流同宽。设置页替换整个聊天页，不作为悬浮层。

侧栏底部在宽布局和 icon rail 中都提供“新会话”操作；只有选中了无阻断恢复问题的 Project 时
可用。该命令创建一个 Simple 模式 root Thread，并采用 Bridge 返回的 canonical product snapshot
原子选择新 Thread。每个健康 root Thread 都提供“归档会话”操作；目标 root Thread 或其 child
Thread 存在活动 Turn、pending input 或活动 Task 时必须拒绝归档。归档保留完整 Turn/Item 历史，
同时归档该 root Thread 的 child Thread；Bridge 在同一 canonical snapshot 中回退到仍可用的 root
Thread，没有剩余 root Thread 时按产品默认规则创建并选择空会话。故障 Thread 的同一 trailing
位置继续展示恢复清理操作，不能绕过 recovery issue 门禁。

## 11.9 验收

- Item timeline、reasoning、tool grouping、Composer revision 和 interaction dock 有 widget test；
- 新建 root Thread、归档 root Thread、活动会话禁用以及宽侧栏/icon rail 操作有 widget test；
- root/child 切换时 canonical workspace 与 UI ephemeral 状态均正确隔离；
- lag、断流和旧 generation 不污染当前 workspace；
- Flutter analyze、widget/integration tests 通过；
- Skills 页 active 时自动重新发现有 widget test 覆盖再次进入与快照替换；
- Windows native Driver 使用真实 Bridge，关闭 frame sync，验证输入 read-back、SQLite 状态、
  绝对路径截图和零 runtime error。
