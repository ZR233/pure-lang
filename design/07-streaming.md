# 07 - Thread 实时流

## 7.1 帧模型

订阅命令（subscribeThread，参数为 threadId）返回四种帧：

- `snapshot`：订阅首帧，只包含 Thread 当前执行状态、active Turn、pending Interaction、runtime、
  Todo 与 child directory，不包含完整 Item 历史。
- `notification`：后续 typed 变化。
- `lagged`：表示客户端不能证明增量连续，必须从数据库刷新当前窗口；重新订阅只恢复当前状态，
  不能补回历史 Item。
- `closed`：Thread 或 runtime 已关闭。

每个驻留 Thread 有唯一的实时投影发布者：Thread 观察任务持有该 Thread 唯一的 live projection，
在**没有 GUI 的情况下也**消费 owner 发布的每条已成立 effect，把内容发布进共享会话并广播同一组
typed 变更。订阅只是这份投影的读者：先注册投影帧 receiver，再读取 Thread owner 的小型
authoritative snapshot，最后发送 snapshot，之后只转发投影帧，自己不投影、不读 effect window、
不发布会话内容；注册与快照之间提交的 effect 仍会投递，其中已被快照覆盖的帧按提交序号丢弃。
内容只有一个 owner：每条 Item 正文由该 Thread 唯一的共享内容窗口（`ChatWindowStream` / FRB
`BridgeChatView`）持有并发布，订阅状态流既不携带内容帧也不拼装正文。窗口在打开或重连时立即
返回内存中已成立的 pending/流式正文，**不等待** history writer 或 checkpoint 水位；已保存历史
只在窗口内部按 cursor 分页读取 `history.sqlite`，磁盘写入与保存水位都不进入状态流。普通滚动
分页不得查询 owner 或触发 flush。未驻留 Thread 只有执行或读取当前状态时才显式激活；单纯历史
查询直接读取 `history.sqlite`。
实时流没有 durable replay，缺口通过数据库 cursor 重同步，不通过完整 snapshot 或内存 journal
补丁恢复。

实时事件总线只拥有 Turn、Item、Interaction、runtime 与 live overlay 的实时投影，不拥有
Thread directory 元数据。订阅注册完成后，Studio 运行时必须用内存 Thread directory owner 的
当前条目重绑尚未发送的首帧；不得为了重绑查询 SQLite，也不得把事件总线中为投影保留的 Thread
副本当成 mode、role、title 或 status 的事实源。数据库行只在 owner 冷激活前作为恢复基线。

## 7.2 Notification

内部 trace 的生产者提交开始、追加和终态操作，不预分配 write sequence 或 item revision。唯一
内存发布入口在同一短临界区内校验、编号、更新当前状态并形成规范 effect；Item ordinal 在开始时
首次分配后不变。实时投影与 history writer 消费同一 effect。writer 受理失败必须可观察并暂停新的
执行准入，已成立事实保留重试；它不能通过重放旧 effect 再次进入实时投影。

每条通知由版本化封套承载：`epoch` 标识生产端一次连续广播生命周期（重订阅、owner 重建或数据库
重同步后递增），`base_revision` 是应用本通知之前的状态水位，`revision` 是应用之后的水位。客户端
只有当 `epoch` 相同且 `base_revision` 恰好等于本地已知水位时才能拼接；否则视为缺口并重同步，
不把迟到帧接到新生命周期上。

通知穷尽为：

- `turnStarted`
- `turnUpdated`
- `turnCompleted`
- `interactionChanged`
- `threadRuntimeUpdated`
- `activityChanged`
- `storageChanged`
- `lagged`

状态流**不含内容帧**：`itemStarted`/`itemDelta`/`itemCompleted` 不再存在，条目与流式正文只由该
Thread 的内容窗口（`ChatWindowStream` / FRB `BridgeChatView`）交付；过滤发生在生产端，客户端不靠
末端丢弃内容帧来去重，也不会因此制造 revision 空洞。Turn 与 Interaction 的通知 payload 仍携带
canonical tagged state，而不是 status string 与平行 failure/reason/timestamp。Turn 终态固定为
Completed、Cancelled、Failed、BudgetLimited；取消原因和预算 rollover 结果位于对应终态 payload。

`activityChanged` 携带 typed `ThreadActivity` 小摘要（阶段、稳定 activity 身份、并行工具数量与
最近开始摘要），`storageChanged` 携带 typed `ThreadStorageState`（故障类别、代数、accepted/
durable 水位、执行阶段、`pressurePaused`、`resumeRequired`、`canResume`）；两者都只在真变化时
补发，`None` 分别表示没有活动、没有可报告的存储事实。`canResume` 是“现在显式继续真的会被接受”
的 typed 就绪事实（闩仍 owed、无硬故障、无未上交批次、无字节阈值、恢复 fence 已 durable），
界面只据此启用继续入口，后端仍会重查防竞态。

`turnUpdated` 承载**运行中 Turn 的 canonical 阶段**，而不只是 Turn 记录的修订：Task 启动/结束
与模型 attempt 提交都会改变由运行事实派生的阶段，实时投影在每个 effect 之后重新推导运行中 Turn
的阶段，变化就补发一帧，不等待下一次 Turn 记录提交，也不依赖客户端窗口行。阶段帧与被同一
effect 投影的 Item 使用同一提交序号作为 revision；Turn 终态提交后不再有更晚的阶段帧覆盖它。
阶段帧本身不携带工具身份、关键词或并行工具数量；活动行的这些细节来自同一 owner 发布的
`activityChanged`（见 [18](./18-studio-state.md) §18.8.1），完整详情按活动身份按需读取。

Turn 的最近终态必须对慢消费者与重订阅可恢复，而不是只对当时在场的订阅者可见：`turnCompleted`
只广播一次，快照又只携带**活动** Turn，所以唯一 owner 同时保留它投影出的最新终态 Turn 作为自己的
typed 事实。订阅建立时若快照没有活动 Turn，就以同一水位在该快照之后补发这条 `turnCompleted`，
该帧与被广播帧来自同一个 Turn 投影，因此最近终态、原因与身份在实时与重订阅之间一致，不
需要客户端从窗口正文反推 Turn，也不需要缓存它曾见到的 `running` 状态。活动 Turn 存在时不补发，
迟到的终态帧也不会把已终结的 Turn 重新标成运行；同一 Turn 的终态只交付一次。Item 仍然没有
durable replay：缺口依旧靠数据库 cursor 重同步，这里补发的只是 owner 作为当前状态持有的**最新
Turn 事实**，不是历史条目。

owner 首次安装或进程回收后冷恢复时，这份保留事实不能是空：live owner 在安装路径（本就读取
history 的唯一冷读点）从既有 durable Turn 记录做一次有界、不读条目正文的读取，取得最近终态 Turn
并按执行序号播种；正在投影的更新只按更大的执行序号/修订向前合并，正常实时提交路径仍不回读刚写
SQL。retired 子会话没有 owner 可保留，其订阅在权威快照之后以同一 `turnCompleted` 契约补发一次
durable 最近终态 Turn，因此有无 owner 的前端都从同一 Turn 事实读 `lastTurn`。这仍不是 Item 的
durable replay，缺的条目依旧由数据库 cursor 重同步。

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

内容窗口的字段变化只携带 canonical 身份与 typed 字段（`ChatField`，例如 agent message text、
reasoning summary/content、plan text、tool arguments/output），正文按 `Append`/`Replace`/`Remove`
交付，一次 `UpdateItem` 只提交一次内容 revision；未终态的流式正文只驻留窗口内存，不形成
history 写入。terminal Item 携带完整 authoritative payload 并随同一 `ThreadEffectBatch` 交给唯一
history writer；窗口按 `ChatLifecycle` 收敛执行终态、保存确认只翻 `saved`，不等待 SQL 确认，也不
存在第二份 overlay 与 SQL 行合并。文本 Item 的 channel
穷尽区分 `user`、`parentAgent`、`commentary` 与 `final`；`parentAgent` 只由 runtime 冻结的
mailbox 来源产生，所有 transport 和 Flutter reducer 都机械透传，不在客户端推导。

模型失败或主动取消时，失败事实携带最后一次已发布的完整 `ModelProgress`，投影按原 Item
身份及已预留 ordinal 提交失败/取消终态正文。未终结期间的 progress 仍只驻留内存；进程异常退出
的未终结正文允许丢失，重启恢复产生的中断终态不伪造先前前缀。

未终结期间的 `ModelProgress` 每帧只把新 chunk 追加到本帧变化的那一条观察身份上，身份与内容
版本稳定不变，正文仍按共享 prefix 链增量传递。投影自身不持有、也不在每次事件重新克隆整份 part
列表：列表是发布快照里的分块容器 `ModelParts`——已填满的 chunk 与仍在增长的 tail 各自共享，
watch 仍持其唯一引用时原地更新；一旦观察者克隆过某一帧，一次编辑只按 copy-on-write 复制所编辑的
那一个 chunk，另外在编辑落在已填满 chunk 时（以及 tail 转为 filled 的那次追加）复制一次 chunk
索引，其余 chunk 不复制，因此被持有帧保持不可变。`filled` 的每块与 `tail` 的长度上界都是一个
chunk。包含大量 part 的响应按事件追加一个 chunk，**整表复制被消除**；逐帧持有快照的消费者仍可能
每帧触发一次其共享 chunk 的复制，本机制不承诺实测性能。快照版本、稳定身份与终态拒迟到语义不变；
可靠输出计量不变，但被拒绝的编辑仍会前进一次版本并发一次通知（内容保持已接纳的部分），不宣称
全部语义完全一致。

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
done 全部丢弃。正文不再走状态流：内容只由该 Thread 的内容窗口（`ChatWindowStream` / FRB
`BridgeChatView`）交付，状态流自身不产生也不转发 Item/Delta 帧。窗口的内容 delta 只允许命中当前
未终态 Item 且该 item 的 `expected_revision` 相等，正文变化按 `ChatField` 字段身份输出
`Append` / `Replace` / `Remove` / `Unchanged`；一次 `UpdateItem` 只允许一次内容 revision 提交，
不存在把新字节追加到已失效前缀的分支。缺口、未知变体或 lagged 统一重新订阅。
新代的首个 authoritative snapshot 重置该代通知水位，即使旧代的实时通知 revision 更高也必须
采纳；旧代未终态预览作废，尚待 SQL 同身份确认的终态条目继续保留至分页确认。只允许同一代的
后续通知按 `base_revision` 连续拼接。

Timeline workspace 的正文只有一个 owner：这份单列视图就是该 Thread 的内容窗口
（`ChatWindowStream` / FRB `BridgeChatView`）本身，不维护独立实时尾部、第二份 item cache，
也不把 SQL 行与内存 overlay 归并成两条来源。窗口建立时立即返回内存中已成立的 pending/流式
正文，因此打开或重连即可见，**不等待** history writer 或 checkpoint 水位；已保存历史只在窗口
内部按 cursor 分页读取 `history.sqlite`，首窗与翻页都走同一条 view 内部路径，不做状态流
`Latest` 首窗加 overlay 合并，磁盘写入与 flush 也不进入状态流。

core `pl-core::chat` 窗口是这一单列视图的唯一内容窗口：它以稳定的 `ChatField` 为内容身份，
`Body`、带 provider part 身份的 `Part` 与宿主定义的 opaque `Host` key（例如工具 arguments 与
result 两个不同域）都只是字段身份，core 不从正文推断协议字段。正文是共享不可变 `ContentBlock`，
向宿主产出 typed 结果：同一 item 的全部字段变化合批为一次 `UpdateItem`（`expected_revision`
一次基线校验、`revision` 一次版本提交，`omitted_bytes` 与保存水位 `saved` 随批一次提交），结构
变化为 `Splice`；字段删除、仅版本推进、仅保存确认、同版本预览升级为完整正文都有完整可应用语义，
宿主不向 JSON 字符串追加字节、也不在每 token 解码或编码全文。四个事实互不替代：item 内容
`revision`、窗口 `version`（patch 的 `from`/`to`）、保存水位 `saved`、执行终态 `ChatLifecycle`。
`confirm_saved` 只翻 `saved`，不改变身份、顺序、revision、正文或执行终态，所以终态条目可以尚未
保存、已保存条目仍可继续增量；终态拒绝迟到预览只依赖执行终态事实，不依赖 writer 速度。窗口只
保留有界终态标记，超过上限时淘汰最小顺序并把该顺序折入单调顺序水位，顺序槽严格递增且从不复用，
因此标记淘汰后同一身份仍被顺序水位拒绝；持久化历史始终是内容最终 owner，冷历史读取不受该水位
限制。

窗口对宿主暴露廉价、可 clone、取消安全的锁外 readiness 等待：Session 发布的事实只有窗口
`version` 与边界水位线 `urgent` 两个数；`ChatView::version()` /
`ChatView::watch()->ChatWatch::wait(seen)` 只读这两个数，不构快照、不算差量、不持锁，level-triggered
且取消安全，取消不消耗版本。`wait` 只在有待交付边界（`urgent > seen`）时立即返回，只有已开始普通
文本才持有一个**固定**合帧 deadline，且该 deadline 锚定在首次观察处、后续 delta 不重启；无变化则
阻塞。宿主因此在锁外等就绪、在短临界区用同步 `ChatUpdates::take()` 相对本消费者已交付 baseline
**只算一次**要交付的差量并原子推进基线（无 await）。消费者持有两个互不混用的水位：
`ChatUpdates::delivered_version()` 是客户端真正收到过内容的版本（`take()` 只在实际交付一帧时推进
它，`Patch::from` 永远等于它），`ChatUpdates::observed_version()` 是已经看过/跳过的唤醒水位（对每个
新版本都推进）。等待必须用 `observed_version()` 入 `ChatWatch::wait(seen)`，`take()` 的差量则从
delivered baseline 计算：这样既不会为已跳过的版本反复唤醒，也不会让下一次可见 Patch 的 `from`
落在客户端从未收到的版本上。`next()` 复用同一 `wait` 与 `take` 语义，HTTP 与 FRB 共用一套合帧规则，
不重复构 diff、不二次合帧。窗口按权威事实给出更新优先级：新身份首字、执行终态准入、保存确认、
字段删除、权威替换与结构变化是立即交付的边界，合帧等待被任何边界更新打断，首字与终态立即交付。
历史窗口遇到无关 live 更新时可见正文不变，只在 `has_newer` 首次翻起时通知一次新内容，之后的无关
更新不产生空 Patch、不 Reset、不重建历史；此时只推进 observed 唤醒水位，delivered baseline 停在
客户端最后一个 `to`，因此后续窗口内可见变化（含按身份取回完整正文）仍从该版本接起，等待侧也不会
空转；显式回到 `Latest` 立即以一帧恢复权威内容。

运行中的 Item 在窗口里始终给出完整正文，已持久化历史在窗口里只保留有界预览，宿主按身份请求完整
正文并拿到同一窗口 revision 的权威正文；完整历史正文至多冷读一次，保留到该身份离开窗口为止，
读取不在锁内等待存储，不静默截断助手正文，也不为观察层保留第二份全文。按身份取回完整正文同时是
该 view 对那个身份的**持续完整正文意图**：之后每个新 revision 都继续沿同一共享链把完整正文作为
typed `Append` 交付，可见正文不再回落到有界预览、也不再发生第二次存储读取；该意图只在该身份离开
该 view 时随 overlay 释放，身份离窗后重新请求才再读一次存储，且不会把已释放的正文复活进窗口。
宿主因此不需要（也不允许）用"先预览、再取全"的时序自己猜何时该继续跟随。
按身份取回完整正文若改变了
窗口**可见**正文（例如同 revision 的有界预览升级为完整正文），由 core 推进窗口 `version` 并通知
watcher（不是运行时捏版本）；`revision` 不变，`saved` 与 `ChatLifecycle` 也不受影响，窗口的完整
正文缓存只承载正文，`saved`、`lifecycle`、`turn_id`、`meta` 与顺序始终以时间线为准，缓存 overlay
只在同 revision 且更完整时升级正文，绝不回退独立状态事实（保存确认或终态准入不会被先前读取的缓存
body 覆盖）；迟到读取在身份离窗后不写回、不推进版本。reconcile 只在 revision 完全相等时合并，绝不
产出跨 revision 的混合 item：窗口已推进到更高 revision 时，迟到的旧正文被丢弃并返回权威最新 item；
正文比窗口槽更新时按自身事实保留。窗口缓存也不被迟到的旧 revision 覆盖较新的缓存条目（只在
`revision >= 缓存 revision` 时写入）。

运行中工具调用的输出不走"每来一个 chunk 就把累计全文再报一遍"的路径。生产者（命令执行等）持有
共享不可变 `ContentBlock` 链，只把**变化的部分**交给 Thread：一个生产者自选的 opaque 输出身份
（例如 `command-output`，core 不解析该 key、不从正文推断、也不与 provider 的 part 身份混用）加上
`Append{chunk}` 或 `Replace{text}`。core 的 owner 先按"应用这份增量之后的总字节数"向该调用的可靠
输出预留计费，再接受增量，因此被拒的增量只取消该调用并保留已接受内容，绝不悄悄丢弃已发布的字节。
实时观察窗口另有自己的上限（`MAX_TOOL_PROGRESS_BYTES`、`MAX_TOOL_PROGRESS_PARTS`）：到达上限时
生产者必须显式 rollover 成 `Replace(有界后缀)`，把"头部已被丢弃"如实表达为整段替换，而不是让观察
层把它当成对旧正文的 append；真正的增量由共享链血缘（`appended_since`/`increment_since`）验证，
基线不再是祖先就一定按替换交付。这个窗口是**实时观察窗口**，不是调用的可靠输出预算：调用的
canonical 结果与归档输出由各自的 cap/文件机制负责，可靠保存额度由 owner 的预留与写者记账，两者
不同名同义、也不互相冒充。live 投影按该身份的 `version` 去重并直接共享最新 block，不再从累计
字符串重建正文；只有 canonical/DTO 交付与详情展开才把当前有界窗口物化一次文本。

模型工具调用的参数与正文共享同一可靠输出计量：`ToolInputStarted/Delta/Completed/ToolCallReady`
在把参数复制进 canonical 累积器**之前**按调用身份的已计费高水位计入 `MAX_OUTPUT_BYTES`，等长权威
`completed` 不重复扣、替换后继续追加不漏扣；参数迟到或失败只保留已接收 partial receipt 供诊断，
**不执行**未闭合参数、也不构造不完整 JSON。命令 operation 的 stdout/stderr 合流捕获另有独立上限
（`MAX_CAPTURE_BYTES`）：两流各自只按读取序把 chunk 接纳进**同一个有界有序修复计划**并通知
operation 生命周期内的**唯一** writer 任务，reader 永不等待磁盘，plan 满即拒绝新 chunk。首个 append
失败后不再接纳新 chunk，但失败前已接纳/排队的两流 chunk 全部按序保留在**同一**计划里，不被后续失败
或首类别覆盖；重放以 backend 确认的已提交长度为截断点（先截回该长度再按同 framing 幂等重放），绝不
把重新读到的 fragment 长度冒充 offset，也不静默截断后称成功。终态快照由 `capture_drained` 门控：
只有 writer 把已接纳字节写盘、或把未落盘者作为同一计划保留，且两流都已关闭后，operation 才发布
`Final`（终态不早于 writer 收束，不假成功）。捕获写/flush 或归档未落盘这类**可靠输出存储故障**另带
一个 `OutputRetryObligation`：producer 在失败当下经运行中调用的可靠通道上报，立即阻断并行
model/tool 准入；显式重试按稳定身份保存**同一份**已接收字节，只有同一 `fault_generation` 的目标与
`fence` 都落地才允许继续，纯预算超限没有该义务。重试成功后补充的 durable 资源引用按**原 call 身份**
落到那条已提交的工具结果上：只把引用追加到同一身份的交付与条目资源元数据，正文、顺序、终态状态与身份
都不移动，也不重跑命令或重复交付；引用早于承载它的提交到达时保持 pending。修复不依赖有界 live 窗口：
身份仍在窗口内时 live 投影补一帧实时修正；身份已离窗（未打开 GUI、慢消费者或窗口滚动淘汰）时由同一个
单投影 owner 先按原 call 身份读回**已经提交**的正文并折回自身表，再补充交付；canonical 条目与内容版本
始终由该 owner 决定，writer 只做纯保存确认（只补 durable delivery 事实，不发明条目版本、不回读正在写的
effect 正文）。因此窗口淘汰不会把已提交身份判成缺口，也不让观察窗口决定一次可靠保存是否成立。

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
query，均返回不含历史 Item 的当前 Thread 状态，不得让 HTTP route 退化为只返回 Thread directory
元数据。FRB 与 HTTP 另有同一 HistoryReader 支持的 Timeline page API；HTTP route 为
`GET /api/v1/threads/{thread_id}/timeline`。
Skill 激活使用普通的终态 Skill Item 和 `threadRuntimeUpdated` 通知；激活来源是 typed 的
`Tool { toolCallId } | UserGesture { invocationId }`，资源位置是 typed resource base，不允许
transport 或前端从工具 JSON 推断；Timeline 文案按来源区分代理激活与用户激活。首次订阅及重连
从 HistoryReader 取得相同的 Skill Item，状态 snapshot 只携带 runtime 的 activeSkills。Skill Item 只接受 typed
resource base、provider identity 与 `Tool | UserGesture` 来源；旧 `path + toolCallId`、缺失
provider 或未知字段一律是协议错误，不做映射、默认填充或读时升级。

Product lag 发送 `stale` 并要求重读 `/api/v1/state`。SSE 不提供 durable replay；收到
`Last-Event-ID` 时先发送 `stale`。每 15 秒发送 comment heartbeat；heartbeat 不占用领域
sequence，FRB 与 HTTP 的 transport buffer 也不共享 sequence 或取消句柄。

Thread title 的生成与提交流程是领域生命周期话题，见 [18](./18-studio-state.md)；流层只保证
成功的 title mutation 通过 `ThreadDirectoryChanged` 增量事件发布，不新增平行 title 通知。
