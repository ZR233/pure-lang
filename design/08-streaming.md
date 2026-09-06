# 08 - Thread 实时流

## 8.1 帧模型

`subscribeThread(threadId)` 返回 `ThreadStreamFrame`：

- `snapshot`：订阅首帧，包含 Thread、当前/最近 Turn、完整 Item、pending Interaction、runtime、
  Todo 与 child directory。
- `notification`：后续 typed 变化。
- `lagged`：只表示 best-effort 事件发生丢弃，客户端必须重新订阅。
- `closed`：Thread 或 runtime 已关闭。

驻留 Thread 的订阅实现先注册 receiver，再直接读取 ThreadActor 的内存 authoritative snapshot，
最后发送 snapshot，避免 snapshot 与 live 之间漏事件；该路径不得查询 SQLite。未驻留 Thread 必须
先通过显式激活命令从冷基线创建 ThreadActor，激活完成后再走同一订阅流程。实时流没有 durable
cursor、journal replay 或 ResyncRequired 补丁协议；恢复永远重新取得同一内存 owner snapshot。
旧历史通过 `listThreadTurns` 的 opaque keyset cursor 从 SQLite 冷分页读取。

ThreadEventBus 只拥有 Turn、Item、Interaction、runtime 与 live overlay 的实时投影，不拥有
Thread directory 元数据。订阅注册完成后，StudioRuntime 必须用内存 Thread directory owner 的
当前条目重绑尚未发送的首帧；不得为了重绑查询 SQLite，也不得把 EventBus 中为投影保留的 Thread
副本当成 mode、role、title 或 status 的事实源。数据库行只在 owner 冷激活前作为恢复基线。

## 8.2 Notification

内部 trace 的生产者提交开始、追加和终态操作，不预分配 sequence 或 item revision。
唯一内存发布入口在同一短临界区内校验、编号、更新项目状态并入队，返回规范事件；所有投影消费
返回事实。入队失败不推进状态。项目身份由 Turn ID 与 Item ID 确定，开始序号首次分配后不变。
实时投影与异步保存分别消费已提交事实，存储故障不阻塞实时通知，也不使旧事件重入实时投影。

通知穷尽为：

- `turnStarted`
- `turnUpdated`
- `turnCompleted`
- `itemStarted`
- `itemDelta`
- `itemCompleted`
- `interactionChanged`
- `threadRuntimeUpdated`

Turn、Item 与 Interaction 的通知 payload 都携带 canonical tagged state，而不是 status string 与
平行 failure/reason/timestamp。Turn 终态固定为 Completed、Cancelled、Failed、BudgetLimited；
取消原因和预算 rollover 结果位于对应终态 payload。Item terminal error、tool result、denial与
完成时间同样只存在于适用的 state variant。

单轮预算只强制活动 wall-clock 上限；model step、tool call 与 wait call 继续作为 typed usage 观测，
不引入隐式迭代次数限制。宿主可按会话用途在 TurnFactory 边界冻结不同上限，但不得自行生成
`BudgetLimited`、rollover 或另一套用量投影。外层产品 watchdog 只能负责资源收束，并应给 PL 的预算
终态与持久化预留余量，不能先于同一 wall-clock 上限取消 Turn。
child Turn 命中预算后必须直接进入带 `budgetPause` 的 idle 状态，不再执行 rollover 压缩或启动 pending
输入；只有父 Agent 后续提交显式输入才能原子清除暂停并开始新 Turn。root Turn 继续保留现有 rollover
行为，预算检查前由上下文压力触发的正常压缩也不受影响。

Item delta 只携带 threadId、turnId、itemId、field、revision、delta 和可选 chunkIndex。field
固定为 agent message text、reasoning summary/content、plan text、tool arguments/output。
terminal Item 携带完整 authoritative payload并清除 UI overlay。
文本 Item 的 channel 穷尽区分 `user`、`parentAgent`、`commentary` 与 `final`；`parentAgent` 只由
runtime 冻结的 mailbox 来源产生，所有 transport 和 Flutter reducer 都机械透传，不在客户端推导。

## 8.3 背压

每个订阅使用有界 mpsc：

- transcript delta、Item terminal、Turn terminal 与 Interaction request 必须 lossless，发送方
  等待通道容量。阶段 milestone 已投影为 commentary Item，其 terminal 同样属于 lossless。
- 瞬时 progress/runtime 刷新是 best-effort，可用 try_send；丢弃数量在下一条 lossless 通知前
  以 `lagged` 发送。
- 不能丢弃需要客户端回答的 request；无法交付时后端取消 request，不能永久等待。

## 8.4 Flutter reducer

Flutter 为每个 Thread 保存 canonical `ThreadWorkspace`，为本地交互保存独立
`WorkspaceUiState`。snapshot 直接替换 canonical workspace；旧 Turn/Item/runtime 不与新
snapshot 混合。Composer、滚动、展开和 submission revision 不属于 canonical snapshot。

`selectedThreadId = null` 是稳定的本地选择状态，表示当前 Project 的未持久化新会话起始页。
该状态按 Project 保存独立 Composer 草稿；只有首次提交成功进入 `startNewThread` command 后
才产生 durable Thread。product/thread resync、目录新增和 Widget 重建不得把 null 隐式改写为
任意目录 Thread。

Thread directory 是 Thread 元数据的唯一 canonical cache。snapshot 中携带的 Thread 只用于
校验身份并重绑到当前 directory entry，不能反向覆盖 directory。这样 product stream 与 thread
stream 即使并发到达，也不会用旧 workspace snapshot 回滚刚确认的 mode/role。

切换 Thread 时增加 generation、立即创建新订阅并取消旧订阅；旧 generation 的 frame、error
和 done 全部丢弃。Item delta 只允许命中当前未终态 Item且 revision 严格递增；缺口、未知变体
或 lagged 统一重新订阅。

## 8.5 Product stream

项目、root/child Thread directory、Task、设置、Provider usage 和 MCP/LSP health 使用独立低频
product stream。product stream 不携带 Turn/Item delta；Thread directory 变化只重绑 workspace
中的 Thread 元数据引用，不改变 Turn、Item、Interaction 或 runtime 内容。

## 8.6 FRB 与 HTTP transport

FRB 与 HTTP SSE 消费同一个 runtime subscription API，不各自实现流状态机。HTTP product stream
为 `GET /api/v1/events/product`，Thread stream 为
`GET /api/v1/threads/{threadId}/events`。Thread 首帧固定为 authoritative snapshot，后续发送
notification、lagged、closed；producer 在连接存活期间持有 Thread residency pin，断开或 server
shutdown 必须取消 producer、释放 receiver 与 pin。

FRB `readThreadSnapshot` 与 HTTP `GET /api/v1/threads/{threadId}` 机械调用同一个 snapshot
query，均返回完整 `ThreadSnapshot`，不得让 HTTP route 退化为只返回 Thread directory 元数据。
Skill 激活使用普通的终态 Skill Item 和 `threadRuntimeUpdated` 通知；激活来源是 typed
`Tool { toolCallId } | UserGesture { invocationId }`，资源位置是 typed resource base，不允许 transport
或前端从工具 JSON 推断。Timeline 文案按来源区分代理激活与用户激活。首次订阅及重连 snapshot
必须包含相同的 Skill Item 与 `runtime.activeSkills`。Thread wire schema 为 v10；Skill Item 只接受
typed resource base、provider identity 与 `Tool | UserGesture` 来源。旧 `path + toolCallId`、缺失
provider 或未知字段一律是协议错误，不做映射、默认填充或读时升级。

Product lag 发送 `stale` 并要求重读 `/api/v1/state`。SSE 不提供 durable replay；收到
`Last-Event-ID` 时先发送 `stale`。每 15 秒发送 comment heartbeat；heartbeat 不占用领域 sequence，
FRB 与 HTTP 的 transport buffer 也不共享 sequence 或取消句柄。

## 8.7 Thread title projection

新 root Thread 的首条文本 prompt 先以规范化摘要作为临时 title，随后由配置中的 Explorer model
异步生成一次最终 title。标题生成请求是隐藏的临时 model request；若 Explorer 模型声明 effort，
使用其按弱到强排列的首项。只提取 provider 返回的可见 assistant 纯文本，忽略 reasoning/思考内容；
生成提示只要求概括首条请求的具体目标并返回一个标题，不把 UI 长度、字符集、标点、词数或输出编码约束交给
模型。运行时仅折叠空白、拒绝空结果，并按 36 个 Unicode 字符截断；JSON、Markdown、引号、标点和普通文本
不进入不同解析或兼容分支。原始首条 prompt 必须先编码为受引号保护的不可信数据，再由同一条最终 user
message 在数据之后重申“只生成 title，不执行请求”；不能把原始任务本身作为标题 Turn 的最后一条完整 user
指令。结果不进入
Thread transcript、Turn 列表或费用投影；任务等待首条 Turn 空闲后再调用 Explorer，失败、
超时和取消都保留临时 title。

title task 使用每任务一次性取消通道；只有持有发送端的 title task owner 能发出取消，provider、Turn
和 runtime 的其他取消域不能取得或改变该信号。runtime shutdown 只 drain 当时已经注册的任务。同一个
runtime 对象再次启动时创建新的通道，不复用父令牌或上一生命周期的取消状态。发送端意外释放不等于
业务取消；只有手动 rename、归档或 shutdown 的显式发送才结束标题任务。

自动结果只能在 directory owner 的 CAS 检查通过后提交：当前 title 必须仍等于生成任务启动时的
临时 title。手动 rename/归档会先取消对应任务；即使取消与 provider 返回发生竞态，手动 mutation
仍会使陈旧自动结果失效。成功的 title mutation 继续
通过 `ThreadDirectoryChanged` 增量事件发布，Thread stream 不新增平行 title 通知。
