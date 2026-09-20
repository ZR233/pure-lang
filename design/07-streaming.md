# 07 - Thread 实时流

## 7.1 帧模型

订阅命令（subscribeThread，参数为 threadId）返回四种帧：

- `snapshot`：订阅首帧，包含 Thread、当前/最近 Turn、完整 Item、pending Interaction、runtime、
  Todo 与 child directory。
- `notification`：后续 typed 变化。
- `lagged`：只表示 best-effort 事件发生丢弃，客户端必须重新订阅。
- `closed`：Thread 或 runtime 已关闭。

驻留 Thread 的订阅实现先注册 receiver，再直接读取 Thread owner 的内存 authoritative snapshot，
最后发送 snapshot，避免 snapshot 与 live 之间漏事件；该路径不得查询 SQLite。未驻留 Thread 必须
先通过显式激活命令从冷基线创建 Thread owner，激活完成后再走同一订阅流程。实时流没有 durable
cursor、journal replay 或 resync 补丁协议；恢复永远重新取得同一内存 owner snapshot。旧历史通过
`listThreadTurns` 的 opaque keyset cursor 从 SQLite 冷分页读取。

实时事件总线只拥有 Turn、Item、Interaction、runtime 与 live overlay 的实时投影，不拥有
Thread directory 元数据。订阅注册完成后，Studio 运行时必须用内存 Thread directory owner 的
当前条目重绑尚未发送的首帧；不得为了重绑查询 SQLite，也不得把事件总线中为投影保留的 Thread
副本当成 mode、role、title 或 status 的事实源。数据库行只在 owner 冷激活前作为恢复基线。

## 7.2 Notification

内部 trace 的生产者提交开始、追加和终态操作，不预分配 sequence 或 item revision。唯一内存
发布入口在同一短临界区内校验、编号、更新项目状态并入队，返回规范事件；所有投影消费返回事实。
入队失败不推进状态。项目身份由 Turn ID 与 Item ID 确定，开始序号首次分配后不变。实时投影与
异步保存分别消费已提交事实：存储故障不阻塞实时通知，也不使旧事件重入实时投影。

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
取消原因和预算 rollover 结果位于对应终态 payload。Item terminal error、tool result、denial 与
完成时间同样只存在于适用的 state variant。

TurnFinished 是最终校验与 worker 失败的终态投影边界：非正常完成必须在 TurnCompleted 前发布
对应 typed Turn Item，使实时与持久化历史均能显示失败、取消或预算原因；已有同轮终态错误 Item
时不重复插入；模型正文生成完成不等同于最终校验通过。准备阶段保留必要进度；正文后不追加
"正文已完成"或"本轮已完成"评论，避免重复最终回答或提前宣告最终校验成功。

执行步数策略由宿主在 Turn 开始时冻结，明确区分无限制与有限步数。Studio 主会话不设置累计调用
次数或总时长预算；子会话每个 Turn 达到 256 步后以 stepLimit 暂停，等待显式续接。
新 Turn 从零计数并获得完整额度，不累计上一 Turn 的消耗；同一 Turn 内的模型与工具往返不刷新
额度。模型与工具调用计量继续
保留，用户停止、关闭、交互和实际失败独立生效。历史预算终态保持可读，不自动续跑已停止的
Turn；上下文压力触发的正常压缩不属于停止预算。

Item delta 只携带 threadId、turnId、itemId、field、revision、delta 和可选 chunkIndex；field
固定为 agent message text、reasoning summary/content、plan text、tool arguments/output。
terminal Item 携带完整 authoritative payload 并清除 UI overlay。文本 Item 的 channel 穷尽区分
`user`、`parentAgent`、`commentary` 与 `final`；`parentAgent` 只由 runtime 冻结的 mailbox 来源
产生，所有 transport 和 Flutter reducer 都机械透传，不在客户端推导。

直接父代理的初始任务与后续 inbox 消息均参与同一 Timeline 投影。消息准入后即使尚未消费
也可见；持久消息身份用于去重，准入事实确定时间和稳定顺序，消费事实将消息关联到实际
Turn，关联更新不改变 Item 身份。实时、历史分页与冷恢复共用此规则；内部通知和非直接
父代理来源不会显示为 `parentAgent` 对话。消息正文保持原样，不从当前模型上下文重建。

## 7.3 背压

每个订阅使用有界 mpsc：

- transcript delta、Item terminal、Turn terminal 与 Interaction request 必须 lossless，发送方
  等待通道容量；阶段 milestone 已投影为 commentary Item，其 terminal 同样属于 lossless。
- 瞬时 progress/runtime 刷新是 best-effort，可用 try_send；丢弃数量在下一条 lossless 通知前以
  `lagged` 发送。
- 不能丢弃需要客户端回答的 request；无法交付时后端取消 request，不能永久等待。

## 7.4 Flutter reducer

Flutter 为每个 Thread 保存 canonical 工作区状态，为本地交互保存独立 UI 状态。snapshot 直接
替换 canonical 工作区；旧 Turn/Item/runtime 不与新 snapshot 混合。Composer、滚动、展开和
submission revision 不属于 canonical snapshot。

`selectedThreadId = null` 是稳定的本地选择状态，表示当前 Project 的未持久化新会话起始页。该
状态按 Project 保存独立 Composer 草稿；只有首次提交成功进入 `startNewThread` command 后才产生
durable Thread。product/thread resync、目录新增和 Widget 重建不得把 null 隐式改写为任意目录
Thread。

Thread directory 是 Thread 元数据的唯一 canonical cache。snapshot 中携带的 Thread 只用于校验
身份并重绑到当前 directory entry，不能反向覆盖 directory——product stream 与 thread stream
即使并发到达，也不会用旧 workspace snapshot 回滚刚确认的 mode/role。

切换 Thread 时增加 generation、立即创建新订阅并取消旧订阅；旧 generation 的 frame、error 和
done 全部丢弃。Item delta 只允许命中当前未终态 Item 且 revision 严格递增；缺口、未知变体或
lagged 统一重新订阅。

## 7.5 Product stream

项目、root/child Thread directory、Task、设置、Provider usage 和 MCP/LSP health 使用独立低频
product stream。product stream 不携带 Turn/Item delta；Thread directory 变化只重绑 workspace
中的 Thread 元数据引用，不改变 Turn、Item、Interaction 或 runtime 内容。

## 7.6 FRB 与 HTTP transport

FRB 与 HTTP SSE 消费同一个 runtime subscription API，不各自实现流状态机。HTTP product stream
为 `GET /api/v1/events/product`，Thread stream 为 `GET /api/v1/threads/{thread_id}/events`。
Thread 首帧固定为 authoritative snapshot，后续发送 notification、lagged、closed；producer 在
连接存活期间持有 Thread residency pin，断开或 server shutdown 必须取消 producer、释放
receiver 与 pin。Runtime subscription 在恢复 owner 前取得 pin，并持有至订阅释放；重叠订阅与
临时激活分别计数，任一 guard 释放不影响其余 pin。

FRB `readThreadSnapshot` 与 HTTP `GET /api/v1/threads/{thread_id}` 机械调用同一个 snapshot
query，均返回完整 Thread snapshot，不得让 HTTP route 退化为只返回 Thread directory 元数据。
Skill 激活使用普通的终态 Skill Item 和 `threadRuntimeUpdated` 通知；激活来源是 typed 的
`Tool { toolCallId } | UserGesture { invocationId }`，资源位置是 typed resource base，不允许
transport 或前端从工具 JSON 推断；Timeline 文案按来源区分代理激活与用户激活。首次订阅及重连
snapshot 必须包含相同的 Skill Item 与 runtime 的 activeSkills。Skill Item 只接受 typed
resource base、provider identity 与 `Tool | UserGesture` 来源；旧 `path + toolCallId`、缺失
provider 或未知字段一律是协议错误，不做映射、默认填充或读时升级。

Product lag 发送 `stale` 并要求重读 `/api/v1/state`。SSE 不提供 durable replay；收到
`Last-Event-ID` 时先发送 `stale`。每 15 秒发送 comment heartbeat；heartbeat 不占用领域
sequence，FRB 与 HTTP 的 transport buffer 也不共享 sequence 或取消句柄。

Thread title 的生成与提交流程是领域生命周期话题，见 [18](./18-studio-state.md)；流层只保证
成功的 title mutation 通过 `ThreadDirectoryChanged` 增量事件发布，不新增平行 title 通知。
