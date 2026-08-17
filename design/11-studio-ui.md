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

桌面窗口的可取消关闭请求必须等待 typed bridge `shutdownRuntime` 完成后才允许引擎
销毁。`detached` 和 widget `dispose` 只是不可等待的兜底信号，它们复用同一个幂等
shutdown future。该 future 顺序关闭 Agent、MCP 和 LSP，并等待所有后台子进程树退出；
不得用未等待的 Dart callback 作为正常 GUI 退出路径。

关闭请求先呈现不可关闭的关机阶段 overlay：订阅 typed `subscribeShutdownProgress` 进度流，
按阶段（停止订阅、停止 Turn、保存会话、挂起任务、关闭 MCP、关闭 LSP）更新本地化文案与
进度指示；`FlushingPersistence` 阶段必须等待 pending 落库归零后才进入下一阶段，关机完成的
判定以进度流到达 `Stopped` 为准。updater 安装更新触发的 idle 关机复用同一进度流与 overlay。

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

Thread directory 唯一拥有 mode、role、title、关系、status 与 updatedAt。GUI 内 Thread directory
是有界分页窗口：只保存已加载页的条目、`nextCursor` 与 `hasMore`；侧栏触底通过
`listThreadsPage` keyset cursor 继续加载，目录增量事件按 ThreadId 原位合并（新会话前置、
归档移除），未加载条目的增量直接忽略。

`selectedThreadId` 是显式状态机，只在三种情况下变化：用户显式动作（选择/进入新会话起始页/
跨项目切换）、归档 command 返回的选择建议、bootstrap。`null` 表示当前 Project 的未持久化
新会话起始页，是稳定且 authoritative 的 UI 选择；目录窗口 resync、目录新增、Widget 重建和
lag 恢复都不得把 null 隐式改写为任意 root Thread。bootstrap 和显式跨 Project 切换可以选择
该 Project 最近的健康 root Thread；不存在 root 时进入起始页。

Timeline items 使用单一合并规则（live 帧、snapshot、历史页共用）：身份 = itemId + threadId +
turnId + kind；同 id 时仅当 incoming revision >= existing 才替换；新 id 插入后按
`(ordinal, id)` 全序排序。ordinal 是 Rust 事件总线一次性分配的不可变顺序事实，不参与身份
比较；替换载荷时防御性地保留已加载 ordinal。`ThreadWorkspace`
中的 Thread 是归一化后的 directory 引用，不是第二份事实源：thread snapshot 只能替换 Turn、
Item、Interaction 和 runtime，并把该引用重绑到当前 directory entry，不能覆盖 directory。

以下都是 UI 本地状态，放入按 Thread 隔离的 `WorkspaceUiState`：

- Composer draft、提交阶段和 submission revision；
- 滚动、bottom-following 与未读计数；
- reasoning/tool/Todo 展开状态；
- subscription generation 与临时 delta overlay。

新会话起始页的 Composer 同样是 UI 本地状态，但按 Project 隔离；它不伪造 ThreadId 或
`ThreadWorkspace`。首次提交成功后，草稿和 submission revision 一次性转移到 Bridge 返回的
新 Thread workspace，失败则留在起始页并恢复草稿。

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

历史窗口遵循「窗口状态从已加载内容派生」的单一模型，只有 `hasOlder / isLoading /
epoch` 三个标量；回源锚点永远从 `items.first.turnId` 现场派生，GUI 不保存 cursor 栈或
页计数，items 与窗口状态不可能漂移。订阅/重订快照是窗口的重建点：wire 快照只携带最近
400 条（整 Turn 对齐截断）与 `historyCursor`（窗口首 Turn 的 id）；快照落地时 epoch 递增，
跨重建的在途历史响应整体丢弃。`listThreadTurns` 的 cursor 是 Turn id 的 before 语义锚点；
历史页幂等合并进窗口，更旧方向是否还有内容由页响应的 nextCursor 决定。窗口超过 500 条
时从最旧方向裁剪并把 `hasOlder` 置回 true，被裁内容按裁剪后新首条的锚点回源重取——
内存未命中一律从数据库读取，GUI 不保留第二份完整历史。rolledBack 恢复标记只来自 DB
历史查询，按 id 覆盖窗口内同 id 条目，不受 revision 门槛约束。

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

receipt 已接受后，Composer 必须继续关联该 Turn，不能在首次 `TurnStarted` 时丢失 identity。
对应 Turn 若随后 failed，Composer 解除 pending 并显示 typed failure message（缺失时回退到 Turn
reason）；该规则也适用于 failure 晚于首次 in-progress 通知到达的情况。失败 Turn 的 terminal
trace 还必须作为 durable Timeline error Item 投影，确保重启或历史加载后错误仍然可见。

root Thread 可提交普通输入；child Thread 默认只读，但 pending interaction 与停止操作仍可用。
footer 同时最多显示一个 pending interaction。优先级为 tool approval、user input、plan
confirmation。计划正文在 timeline 的 plan Item 中，确认 dock 只承载实施、继续调整和忽略。

只要当前 Turn 未终态，停止按钮始终可用。`busy` 只由当前 Thread 的 active Turn 决定；pending
plan confirmation 可以在 `busy=false` 时继续阻塞普通 Composer。

## 11.6 状态栏与 agent directory

状态栏只读取当前 workspace 的 owner、模型、context、skills、MCP、LSP 和 Todo。上下文条目只
保留进度圆环；缓存命中率、token 明细与费用不在状态栏直接展示，统一进入点击圆环弹出的详情：
context/total token、缓存命中、未命中、写入、reasoning token、inference 数、按币种的实际花费、
缓存节省和部分未定价提示。读数只来自 canonical `ThreadRuntimeSnapshot`，不在 Flutter 建立逐
inference 或计费副本。

费用只展示按币种聚合的实际花费：货币符号 + 金额，多币种用 ` + ` 连接（如 `￥1.2 + $2.6`），
不做汇率换算；已知币种 CNY/USD 显示 `￥`/`$`，未知币种回退为币种代码前缀。

LSP 活动指示是状态栏的运行时状态条目：任一 LSP server activity 非 idle 时显示活动摘要
（如“正在索引 40%”），详情列出各 server 的活动类型与 title/message；数据取自产品级
`LspStateChanged`/`readLspState` 投影，不隐式触发 probe。窄宽度下直接条目收进溢出菜单，
活动摘要与详情仍从菜单可达；详情内容超出高度约束时滚动展示。demo 构建按确定性周期推进索引
活动（递增 revision 的 `LspStateChanged` 事件），供 GUI demo 与 Driver 验收动态显示。
root 通过 typed mode selector 切换 Simple/Task，并展示对应角色模型；Bridge 返回的 canonical
Thread 状态确认切换结果。活动 Task 或会话运行（Thread 非 idle）期间 selector 保持可见但禁用。
child 只读展示实际运行模型。
root-only、活动 Task 与会话运行锁定同时由 StudioRuntime 校验，不能只依赖 Widget 或 Controller
拦截。模式切换只允许 actor idle 且没有 pending input；StudioRuntime 持 lifecycle 临界区后单次
原子持久化 mode/role 目录记录，再尽力同步进程内 actor 角色，失败只告警——提交 prompt 时的
reconcile 与 Turn 构建时的 mode 派生保证不会留下行为分叉。

agent directory 只在 header 的单一菜单中展示 root/child 层级、role、status 和 attention。
child 的 timeline 不复制到 root，父 Thread 的 agent control tool 只作为自己的 toolCall Item。
UI 展示固定 role 时按当前 locale 使用本地化名称（中文为探索者、计划者、执行者、审查者），
但 Thread、Agent 和设置协议仍保存稳定的 role key。未知扩展 role 保留原始值作为展示回退，
不能因本地化映射缺失而隐藏或改写身份；空 role 使用本地化的通用 Agent 名称。

Turn phase 只在 timeline 尾部的一个活动块显示：preparing、thinking、responding、planning、
runningTool、persisting。终态后移除活动块；失败和中断由 Turn 终态展示。“等待用户交互”
不是 Turn phase，而是由 Thread 上挂的 pending Interaction 派生的 UI 状态——交互 dock
出现时 composer 锁定，Interaction 消失时解锁。

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

## 11.8 启动时序

Bridge 只暴露一个 `startStudioRuntime` 启动 command。它完成 SQLite、ConfigRuntime、durable
recovery、Thread framework/MCP owner 和 system Skills 的明确初始化，随后发布 runtime ready。
Flutter 再调用纯查询 `readStudioState`，本地选择健康 Project/root Thread，并通过一次
`activateProject` 执行该 Project 的 LSP membership/probe 与 Skills discovery。

启动后的任何页面刷新、Widget 重建、窗口恢复和 product/thread lag resync 都只读取最新
canonical snapshot，不触发 reconcile、probe、discover、actor ensure 或默认 Thread 创建。
完整 CQS 与 owner 合同见 `20-studio-state-runtime.md`。

## 11.9 设置与视觉

Settings 是独立页面，覆盖 Providers、Instructions、Skills、Roles、MCP、LSP、Security、General。
所有保存采用 typed command，并用 bridge 返回的 canonical settings snapshot 替换本地状态；
secret 使用 preserve/replace/clear enum，不解析错误消息或 raw JSON 控制流程。
Skills 页进入时只读取当前 catalog，并把返回的 canonical snapshot 应用到 GUI 状态；列表内容跨
标签切换与页面重建后保留。“重新发现”仍是唯一显式发现入口；只有用户点击“重新发现”或
Project 激活 command 才执行发现。
MCP/LSP 页进入和“刷新”只读取各 owner 的 last-known snapshot；MCP 单 server“重新连接”、
经确认的“全部重置”，以及 LSP probe、typed repair 和 reset 都必须调用各自的明确 command。
这些操作使用稳定 `ValueKey`，其 command response 仍按领域 revision 应用，不能覆盖更晚事件。

聊天页保持低对比双栏桌面布局：左侧 Project/root Thread，右侧当前 Thread workspace；窄屏改为
icon rail。普通 agent 正文无卡片背景，plan 使用轻边框，reasoning/tool 默认折叠。Composer、
状态栏和阅读流同宽。设置页替换整个聊天页，不作为悬浮层。

侧栏底部在宽布局和 icon rail 中都提供“新会话”操作；只有选中了无阻断恢复问题的 Project 时
可用。该操作只清空 Thread 选择并进入按 Project 隔离的未持久化起始页，不创建空 Thread；首次
提交调用 `startNewThread`，由 Bridge 在同一生命周期临界区创建 Simple root Thread、提交首个
Turn，并返回 Thread 与 receipt。每个健康 root Thread 都提供“归档会话”操作；目标 root Thread
或其 child Thread 存在活动 Turn、pending input 或活动 Task 时必须拒绝归档。归档保留完整
Turn/Item 历史并事务性归档整棵 Thread 树；Bridge 返回 removed ids，以及同 Project 完整排序中
优先下一项、否则上一项的 root Thread。没有剩余 root Thread 时 `selectedThreadId` 保持 null 并
展示新会话起始页，绝不创建兜底 Thread。故障 Thread 的同一 trailing 位置继续展示恢复清理
操作，不能绕过 recovery issue 门禁。

Task failure 是 canonical runtime 的一部分。fatal failure 在根会话状态栏显示红色
`error_outline` 与“任务失败”，Task detail 和 agent 菜单同时展示来源 role/agent、脱敏原因、
provider kind、code 和 HTTP status；即使 assistant Item 正文为空，Timeline 也必须回退渲染
Item error 或 Turn failure message。recoverable failure 使用警告视觉与“可继续”语义，不伪装成
完成或自动重试。fatal Task 不提供同 Task 恢复按钮；用户修复 Provider 后新建 Task。

`task_complete` 被结构门禁拒绝时，tool block 展示稳定 code 和用户可读 message；成功完成不展开
冗余结果详情。任何 UI 投影都不得显示 API Key 原文；Bridge 已提供的 agent error/reason 在
directory 增量更新和 selected agent 重建时必须保留。

## 11.10 验收

- Item timeline、reasoning、tool grouping、Composer revision 和 interaction dock 有 widget test；
- 新建 root Thread、归档 root Thread、活动会话禁用以及宽侧栏/icon rail 操作有 widget test；
- 零 Thread、新会话起始页、未持久化草稿、首次发送创建与归档最后 Thread 回到起始页有
  widget test 和 Flutter Driver 验收；
- 侧栏分页窗口、触底加载与目录增量合并有 widget test；
- 时间线窗口三迁移（快照重建、历史页扩展、上限裁剪）、锚点派生回源与跨代际响应丢弃有 widget test；
- 关机阶段 overlay、pending 归零与全部关闭 hook 共享幂等 shutdown future 有 test；
- root/child 切换时 canonical workspace 与 UI ephemeral 状态均正确隔离；
- lag、断流和旧 generation 不污染当前 workspace；
- 空正文 provider failure、fatal/recoverable Task 状态、agent directory 错误保留与
  `task_complete` 门禁拒绝 message 有 widget test；
- Flutter analyze、widget/integration tests 通过；
- Flutter Driver 验收覆盖侧栏翻页到底、时间线驱逐回源与关机阶段序列；真实 runtime harness
  在隔离 `PURE_STUDIO_HOME` 下验证 write-behind flush、pending 归零、二次启动数据完整与
  进程树清理；
- Skills 页进入不扫描目录，只读取当前 catalog 并有 widget test；“重新发现”使用明确
  command 并整体替换 catalog；启动 bootstrap 对健康选中 Project 恰好调用一次
  `activateProject`，阻断 recovery issue 时不调用；
- MCP/LSP 页刷新无副作用，reset/probe/repair 只由对应稳定控件触发并有 widget test；
- Windows native Driver 使用真实 Bridge，关闭 frame sync，验证输入 read-back、SQLite 状态、
  绝对路径截图和零 runtime error。
