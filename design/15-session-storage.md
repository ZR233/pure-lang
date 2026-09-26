# 15 - 会话状态、历史与调用记录

本文是 Thread 当前状态导出、会话历史写入、调用记录、保存屏障与恢复的唯一权威源；Studio
数据布局与迁移协调见 [17](./17-studio-storage.md)，core 内核契约见
[16](./16-core-contracts.md)。

**实施状态：**core 的 `Session`/`ChatView`、Studio 的流式/历史投影、FRB 窗口与原生 GUI
使用同一条目身份和会话内顺序分配器。旧会话转换不进入 v2；供应商配置单独迁移。所有
实现改动及本地压力场景须经过统一门禁和人工验收，不能把模拟场景通过视为真实供应商验证。

## 15.1 事实源分离

同一 Thread 的持久化分为三个互不替代的事实源：

| 数据 | 活动事实源 | 持久化 | 读取路径 |
| --- | --- | --- | --- |
| 当前执行状态、当前上下文、待处理输入与交互 | Thread owner | `state.toml` | Thread 激活时完整读取一次 |
| 已提交 Turn、Item、任务身份与完整工具交付 | history writer | `history.sqlite` | core ChatView 合并历史存储适配、内存尾部和未保存事实；任务按身份读取 |
| 模型/工具调用统计、用量、耗时和诊断 | 可丢观察投影 | 全局 `calls.sqlite` | CallReader 独立分页与统计；缺失不代表零 |

Thread owner 不保存已持久化历史，也不执行分页。Session 的 TimelineState 由 core 管理，和
Thread owner 共享的是稳定条目身份及变更通知，不共享执行队列；ChatView 通过按需的存储能力
直接读历史，查询不能排队等待正在调用模型的 owner。GUI snapshot 不包含完整 Timeline；
冷历史读取不激活 Thread、不加载模型/工具，也不重放 journal。

“运行时不存历史”不表示丢弃模型上下文。下一次请求真正需要的 `ContextSnapshot`、provider
私有 continuation、当前运行事实与未完成状态仍由 owner 持有；用于展示和审计的历史记录具有
独立生命周期。上下文可以压缩、替换或裁剪，已提交历史不因此删除。

允许存在的临时数据只有：当前运行 Item 的内存正文与输出组装、尚未持久化的可靠批次、
每会话有界尾部缓存、按需打开的有界 ChatView、一次有界 SQL 查询结果。缓存命中和持久化
责任独立：同一正文可共享不可变引用，离开缓存或关闭窗口不能释放未保存事实。writer 确认固定
写入水位后释放对应可靠 effect payload，但最新条目仍可依尾部缓存策略短暂驻留。

### 本地优先的聊天窗口

对外由 Session 打开 ChatView；Session 管执行和会话资源，ChatView 管阅读位置、分页及订阅，
同一会话的多个窗口共享尾部缓存与可靠待保存集合，窗口的聚焦、加载与滚动锚点彼此独立。
`Latest` 跟随尾部；`Around(item_id)` 锚定旧条目，不把后续实时输出插入旧窗口，只发布有新消息
提示。`focus(Latest)` 返回尾部。关闭窗口只解除观察，不取消 Turn 或保存任务。

默认会话尾部最多 100 条，首帧显示最近 32 条，分页每次最多 32 条，单窗口最多 96 条；
另行限制条目正文、页面和窗口的字节预算。冷会话列表只读摘要，不为每个会话预先加载缓存；
打开会话时至多填充最近 100 条，超大正文给同身份预览和按需正文游标。
尚未提交的可靠正文始终由独立待保存集合保留完整版本；超过 256 KiB 的进行中推测性预览
另设每会话 16 MiB 的临时全文读取预算，超出预算会淘汰旧推测性全文，读取不到时明确报错，
不能把这份临时缓存当作可靠保存责任。
这些数值是初始产品预算，压力验收可在保持有界的前提下调整。**待保存集合和在途结果另计可靠
预算，不受 100/96 条窗口淘汰约束。**

Session 统一给条目分配稳定 `item_id` 与顺序键；输入被该 Thread 的唯一投影 owner 接管后立即本地
显示（受理路径本身不投影、不发布会话内容，`admit` 只把不可变批次放进可靠通道），`send` 的成功只
表示受理，不表示磁盘提交或执行完成。相同 `request_id` 重复提交命中同一受理结果。内容生成、
输入执行和保存状态分别表达；保存确认只更新同一条目的保存状态，不再做“删除实时行、查询 SQL、
重新插入历史行”。history/calls writer 只按它本次事务提交的 **identity 与 revision** 回执确认，
不回读正文再发布：正文的唯一权威发布者是该 Thread 的实时投影 owner，writer 只写投影 owner
已交回的那一份不可变批次，因此投影发布必然先于保存确认成立，不存在“保存先于投影”的交错，
会话里也不需要隐藏身份表去桥接迟到的确认。多个展示条目可以共用 `turn_id`，但不得把整个 Turn 当成一条无限增长的
Widget 或正文。模型 Context 独立于展示 ChatItem，缓存淘汰不修改模型输入，Context 压缩不删历史。

排序键只使用**每会话内存递增序号**，不是时间戳或 SQLite 自动主键。冷打开时只读一次已提交
序号的最大值，结合会话内未保存/在途序号初始化分配器；首次可见前为条目身份预留，
同一身份的流式更新与终态重用该序号，writer 在事务中原样写入，不再分配。空内容放弃预留时
留下的间隙不回收；强杀后尚未提交的事实本就不保证恢复，重新打开只从耐久最大值继续。
执行存储受理是同步内存边界：会话在激活时完成这一次顺序水位读取，受理新输入时不再等待
SQLite 编号或提交。只有当可靠 effect 已被 owner 或历史通道持有，才将同身份待保存条目纳入
窗口；队列满时 owner 留住未入队批次，同一个身份在重试中复用顺序号，不能产生第二条消息。
`item_id` 是独立于排序的稳定事实身份（输入/模型块/工具已有确定性身份不得因重新编号改变），
`created_at` 只供展示。provider 原始流上的每个独立展示条目在 `watch` 合并前分配序号；
合并后的预览可以跳过中间帧，但终态仍复用全部原始条目的分配。v2 不使用 SQL 预留表，
未形成持久事实的预览只能在终态历史提交后清理。

流式内容使用稳定 `(item_id, part_id)` 的开始、追加、结束及**严格递增的内容 revision**；结束的 part 不可重用，
工具活动按 `tool_call_id` 定位。连续文本与进度在一个交付周期合并，初字、交互、保存故障及
终态立即通知；普通增量建议按一帧（约 16-33 ms）合并。大正文按有界存储块追加或封口写入，
不得每个 token 重写累积全文；正文读取的游标由 core 定义，不将 Rust 字节偏移当 Dart 字符索引。

TimelineState 内部用短临界区维护尾部、活动条目及未保存引用，锁内不做 SQL、正文编码或
Markdown 解析。`watch` 仅发布轻量版本以合并唤醒；`subscribe` 在同一同步边界取得首帧与
后续版本，交付 `Reset(snapshot)` 或 `Patch(changes)`。按帧合并只发生在这一处：补丁以**该订阅
者自己的 baseline** 为基准 diff，因此一次交付可以跨任意多个被合并的窗口版本，`from` 始终等于
客户端已知版本，客户端不要求 `to == from + 1`；只有补丁词汇表达不了的变化（focus 改变、
`has_older` 边界改变、版本回退或版本未推进）才 Reset。窗口版本严格递增才构成连续补丁：窗口版本
没推进就说明没有任何内容 revision 推进，不能用一个空补丁把缺失的内容版本掩盖过去。索引只相对于
该窗口、该版本有效；`Splice` 更新可见
集合，`AppendText` 按 `(item_id,part_id)` 落到该 part 的文本字段，只在该 part 的内容 revision
**严格递增**时生成，`expected_revision` 是客户端已知的内容版本；一个内容 revision 未推进的
流式预览以同身份整条 `Splice` 交付，终态 payload 同样以同身份整条替换并推进 revision。
迟到消费者若基线过期直接 Reset，不能转发错过的全部 token；跨 FFI 的未确认批次数也有界。
没有窗口就不构建 GUI 差量或解析 Markdown，正在阅读旧窗口时新输出只更新尾部和活动状态。

ChatView 的 `load(Older/Newer)`、`focus(Around/Latest)`、`read_body` 与 `report_viewport`
是 core 入口；查询按顺序键定位、合并尾部缓存、未提交和 SQLite 结果，以 `item_id` 去重、
取最新 revision。并发同方向加载合并，切换 focus 的旧查询不得覆盖新窗口；数据库错误必须
返回错误而非“到头”。查询先捕获所需内存引用，再开启短读事务；保存时出现的重叠可去重，
不可出现 DB 提交与内存释放之间的历史空洞。

## 15.2 Core 状态与输出事实

`pl-core` 的 Thread 类型分为：

- `ThreadSnapshot`：当前内存执行状态，只包含恢复当前逻辑状态及活动观察所需的事实。
- `ThreadCheckpoint`：可序列化的当前状态 DTO，包含 schema、Thread 身份、状态 revision、
  `history_fence`、当前上下文、未完成输入/交互/任务/交付、当前扩展和子 Thread 引用。
- `ThreadEffectBatch`：一次原子状态提交产生的不可变输出事实。它用于实时投影、history/calls
  writer 和产品观察；不是恢复日志，持久化后不由 owner 保留。

checkpoint 保存：

- 当前 Turn 与 attempt 的逻辑状态；
- 当前有效上下文、压缩结果、运行事实和可持久化 model 私有状态；
- 未消费输入、pending interaction/permission、未结束 task 和尚未交付结果；
- 当前扩展、模型 route、workflow 状态、资源引用和子 Thread ID；
- 当前状态 revision 及其依赖的 `history_fence`。

checkpoint 不保存：

- 全部历史 attempt、已结束 Turn、已交付工具结果和已完成交互；
- 每次请求的完整旧输入上下文、所有上下文替换前内容；
- 完整 effect/journal、GUI items、运行句柄、锁、future 或取消令牌。

大型正文与二进制通过稳定 blob 引用保存；不透明 payload 保留 `format`、`version` 与原始 UTF-8
正文，不在 checkpoint 边界解析再编码。

### effect window 与 durable 权威

owner 用一个瞬态 effect window 向活动观察者提供"最近提交但尚未 durable"的 `ThreadEffectBatch`。
窗口只保留尚未被固定写入水位确认的批次：任一 commit 一旦 durable 就立即释放，不再作为第二份
历史缓存。窗口不序列化进 `state.toml`，也不自带字节计量——一次提交只是克隆一个 `Arc`，发布不
阻塞在编码完整正文上，窗口也没有任何按批次数的淘汰。

窗口释放不会丢掉仍要投影的事实，因为 durable 水位只能越过**已被该 Thread 唯一实时投影 owner
接管并交给可靠保存通道**的提交：owner 把同一个不可变 `ThreadWrite` 交给现有可靠保存通道，投影
owner 消费同一份受理事实、只投影一次、把内容发布进共享会话，writer 只消费这份已准备批次并确认
identity/revision/保存水位。writer 不自行投影：队首事实尚未被投影接管时它不写、不推进水位，于是
"窗口释放了某条 effect"蕴含"该 effect 已被投影并发布"。未接管的批次仍按原序号留在可靠通道里，
其编码字节由现有的 per-Thread / 进程字节预算（`pending_thread_bytes`、`process_bytes`）界定；触顶
时暂停新模型/工具准入，而不是静默丢弃。慢 GUI 不进入这条门控：消费者是该 Thread 自己的观察
任务，不是 GUI 订阅者。

接管与准入的先后由投影 owner 自己收敛：Thread 初始化提交的准入可能先被存储压力推迟，晚于
观察安装才落到通道里，这类事实仍然比投影的播种点更老、不会被当成实时帧重放。投影 owner 因此
以可靠通道自己的水位变化为信号重跑接管，把"更老但尚未交回"的提交补齐；它等待的 durable 目标
也是它自己交回的**接管票据**（已交回的最新提交序号），不是绝对 commit 水位——本进程新激活的
Thread 会拿到一个全新的可靠通道，其早先提交在上一进程里已经 durable，用绝对水位等待只会等一个
再也不会移动的值。票据为 0 即"本通道没有需要等待的交接"。

因此 `state.toml` 不含历史正文：已结束 Turn、工具交付正文与已完成交互只存在于 history，
checkpoint 只引用其水位。

产品观察直接消费这些内存 effect 投影计费、目录与终态事实，**不逐批 flush，也不回读历史**：
每条 effect 由该 Thread 唯一的实时投影 owner 投影一次，终态报告与目录摘要都取自这份可靠内存
投影结果，因此一个跨多条 effect 的长 Turn 会带上它此前提交的正文，而不只是收束它的那一条。
报告只带**已提交**的条目身份：流式预览是展示事实，未被提交收束的预览正文不进入父级报告或目录
摘要，同一 `(item_id, part_id)` 的提交条目替换预览条目而不是追加第二份。
只有该 Turn 的条目无法由内存投影证明完整时才按 `items_for_turn` 读 history 重建：这只有两处
冷读——投影安装时一次性播种该 Thread 运行中及最新 Turn 已提交条目（线程在其 Turn 运行中被重新
激活或中途接管），以及没有驻留投影的冷恢复。热路径（提交与预览）因此不再读 SQL。这些冷读仍是
单 Turn 的有界切片，并保留必要的 durable 屏障（投影起点可以晚于 durable 水位，两个来源各自都
不足以覆盖该 Turn）；内存投影从 Turn 起步折起时，终态报告与目录摘要完全来自内存，不 flush、
不读 SQL。

实时投影为报告常驻的不是整轮正文，而是**有界的报告累积器**：按 identity 归并的终态文本、有上限
的可见 commentary 兜底、最新工具条目，以及已提交条目身份集合；同一 identity 的更高 revision
替换条目而不追加，因此累积器不随 token 数或 revision 数增长。可变正文只在它还能被改写时驻留
（未终态条目与最近工具条目），其余条目只保留放置元数据（ordinal/revision/created_at/turn_id），
正文由共享会话的有界窗口与已保存历史承担；投影最多保留最近 `LIVE_ITEM_WINDOW` 条身份，超出按
canonical ordinal 取最新的一批，而不是回读 SQL 或逐批 flush。窗口只收敛**可选**历史：运行中 Turn
仍会引用的身份（它消费的输入，以及每个后续 effect 都会重新投影的 Turn 条目本身）始终留在放置元
数据里，否则一个产出多于窗口的 Turn 会在终态重投影时把输入当成空洞、或让 Turn 条目从 1 重编内容
版本而与已落库行冲突。累积器与驻留条目都是真实内存，因此它按绝对字节水位进入同一可靠预算（见下
节），而不是声称有界却把正文留在预算之外。

窗口释放同样不再等待显式 `flush`：存储后端在自己的 pressure 报告里带一个 typed durable 水位
（`StoragePressure::durable_sequence`，0 表示尚无已确认提交），owner 在每次提交后的内存安全边界
（`admit_cold` / `refresh_storage_pressure`）把它折进 `durable_sequence` 并释放被覆盖的 effect
window 批次，因此正常保存的 Thread 不会把整段会话的批次留到最后一次显式保存。owner 命令队列中的
`Flush` 只在 owner 空闲时被处理，正在执行的 Turn 会让 owner 停在 Turn future 里，逐批 flush
因此会把实时产品事实拖到整个 Turn 结束；正常观察路径不再依赖它。

原始 effect 的慢消费者仍可能收到 `lagged`，但 ChatView 的普通客户端不需自行填补历史与实时
的缺口：由 core 以同一条目的内存/存储合并生成连续 Patch，过期则 Reset。可靠队列压力只暂停
新模型/工具准入；已提交结果、取消、关闭与交互收束仍由可靠保存集合负责。

## 15.3 Effect 发布与背压

owner 在串行提交边界同时完成：

1. 校验候选状态；
2. 分配递增 `state_revision` 与 `write_seq`；
3. 发布新的当前 snapshot；
4. 将同一 `ThreadEffectBatch` 受理到可靠 history 通道，统计作为独立可丢观察；
5. 向实时订阅发布增量事件。

状态提交不等待磁盘 IO。每会话 `HistoryChannel` 由会话管理器强持有，独立于写任务和 GUI 订阅；
`try_publish` 失败时返还原批次，owner 保留至可重试，串行发布最多一个未入队批次。writer 只能
借用队头，历史事务提交成功后才能按 `write_seq` 确认并释放；异常退出不转移或丢弃所有权。
已经完成的实际结果、取消、关闭和交互收束仍可进入保存队列，不能为满足内存阈值丢弃已成立事实。
writer 只写该 Thread 唯一实时投影 owner 交回的**已准备不可变批次**：队首事实尚未被投影接管时它
既不写也不推进 durable 水位，所以"某条 effect 已被 durable 释放"必然蕴含"它已被投影并发布"；
保存成功只按本次事务提交的 identity/revision 确认共享会话（`confirm_saved`），不回读正文再发布，
也不为迟到的确认保留隐藏身份表。已准备的批次是本队列唯一一份正文，它的保留字节计入同一 per-Thread
/ 进程预算，条目 durable 后随队列释放。这份字节计量覆盖条目的**全部可变正文**，不是只算文本与
推理：工具参数、流式输出与终态结果、附件元数据、opaque JSON 载荷、agent/inference 诊断、Skill
与文件事实都按实际驻留长度求和，只有不驻留的附件 blob 不计。实时投影 owner 自己的表（仍可被改写
的条目、放置元数据、报告累积器）在批次 durable 之后仍然驻留，因此它同时向同一通道发布一个**绝对**
驻留字节水位；Thread 预算与状态把这份水位和队列、未入队、写入中、在途预留一起计入，释放这些表
的同一步就下调水位。报告累积器的正文（终态文本、可见 commentary 兜底、最新工具条目）因此也在
预算之内：正常所需结果不被丢弃或静默截断，压力表现为暂停新模型/工具准入的安全间隙。

历史通道分别限制待保存批次数（4096）、每会话字节（64 MiB，覆盖队列 effect、已准备批次、实时
投影与报告累积器以及仍在途的预留）和进程字节（256 MiB，只覆盖队列 effect 的编码字节与仍在途的
预留：已准备批次与投影持有的是队列 effect 已计过费的同一份正文，计进进程总额会把一份正文算两遍，
让上限在真实内存一半处触发）。owner 侧尚未受理的批次由 core 在安全间隙同时加到 per-Thread 与进程
总量上再判阈值，因此"已提出但尚未受理"的事实同样在预算内。普通正文单次预留上限 16 MiB。
模型/工具启动前必须预留其有界输出和终止记录所需空间，未知或无限输出先补可停止的上限契约。
一次模型调用的可靠输出是**同一个**总域：正文、reasoning 与工具调用参数合计受 `MAX_OUTPUT_BYTES`
约束，`ToolInputStarted/Delta/Completed/ToolCallReady` 在把参数复制进 canonical 累积器**之前**按每个
调用身份的已计费高水位先计费，等长权威 `completed` 不重复计费、替换后继续追加也不漏计，因此一个响应
不能同时无限堆积正文和工具参数。工具参数在 `ToolInputCompleted` 之后迟到或失败时，只保留已接收的
partial receipt 供诊断，**不执行**未闭合的参数、不构造不完整 JSON；可靠队列已满则以 typed `QueueFull`
拒绝新准入。一次命令 operation 的 stdout/stderr 合流捕获也单独受限（`MAX_CAPTURE_BYTES`，与 core 单
operation 输出上限同量级）：共享额度按字节先接纳再落盘，触顶或落盘/flush 失败即终止该 operation 的
进程树，**保留**已写部分和有界观察视图，并以 typed 输出压力失败报告而不是静默截断后称成功。两个流各自
只把 chunk 按读取序接纳进同一个有界有序修复计划并通知唯一 writer，reader 不等待磁盘；writer 是
operation 生命周期内的任务，只有它把已接纳字节写盘、或把未落盘者作为同一修复计划保留后，operation 才
发布终态（终态不早于 writer 收束），首个 append 失败后不再接纳新 chunk，但此前已接纳/排队的两流 chunk
全部按序保留、不被后续失败覆盖，重放以 backend 确认的已提交长度为截断点，绝不读取 fragment 长度冒充
offset。磁盘无法继续时通过生产者自己的 typed 错误（`OutputStorageFault`）在 owner 现有
`fault_generation`/恢复门控下暂停新准入，不由后续 writer 重试把它当作已恢复；core 只读取该 typed 类别，
绝不解析诊断文本。受控字节归档沿用同一原则：`retain` 以有界 chunk 流式复制，超过资源上限返回 typed
失败而不是先把无界文件读入内存或磁盘副本；本地归档 IO 失败映射为 typed 存储故障，远程**传输**失败仍是
普通工具错误。
两类停滞必须区分：纯可靠预算超限只保留已接收片段并留下可停止事实，落盘屏障满足后即可继续；生产者
**可靠输出存储故障**（capture 写/flush 失败、归档未落盘）必须携带一个 `OutputRetryObligation`——它按
稳定身份重新保存**同一份**已接收字节（不是重跑工具、不构造不完整结果），显式重试成功且同一
`fault_generation` 的保存目标与 `fence` 均落地后才允许 `canResume`。健康 writer 把该调用的失败结果写
durable **不能**证明失败归档已保存，因此不能被当作恢复；重试幂等，不丢/不重复已接收内容。producer 在
捕获失败当下就经运行中调用的可靠通道上报 typed 故障，使并行 model/tool 准入立即阻断，不必等 `execute`
返回。命令归档上限只作用于命令捕获路径：不给通用 `retain_file`/`retain_bytes`（附件、导入等用户资源）
新增限制；远程捕获在**任何字节传输前**按捕获已知上限拒绝。重试成功后，补充的 durable 资源引用以
`ThreadEffectBatch.delivery_repairs` 按**原 call 身份**随同一 effect 发布：live 投影与 history writer 把
引用补到那条已提交工具结果的资源元数据上，并补齐该交付（`history_tool_tasks` 的 delivery 事实）的引用，
使 UI 与冷恢复看到同一份完整 output；正文、顺序、终态状态与条目身份均不移动，不重跑命令、不重复交付。
修复不依赖任何有界观察窗口：身份仍在窗口内时单投影 owner 直接在内存里补充；身份已离窗时同一 owner 先按
原 call 身份读回**已经提交**的正文，把它折回自己的表再补充并交付，canonical 条目及其内容版本始终由这个
唯一 owner 决定，writer 只做纯保存确认（只按原 call 身份补 durable delivery 事实，绝不发明条目内容版本、
也不回读正在写的 effect 正文）；因此窗口淘汰不会把已提交身份判成缺口，也不让观察窗口决定一次可靠保存
是否成立。
保存故障、容量满、写者不可用和有待写记录时无进展均为持续的类型化状态：后端把**不得解析错误
文本**得到的 typed 类别（`queueFull`/`writeFailed`/`writerUnavailable`/`noProgress`/
`checkpointFailed`/`blobFailed`）连同故障代次与 accepted/durable 水位放进 pressure 报告，owner
把它镜像进自己的存储状态，再经独立 `watch` 与 `ThreadStorageState` 通知 GUI。

短暂压力与硬故障的收束语义不同。队列/字节预算短暂触顶只是背压：owner 在安全间隙等待，自动恢复，
不改变持久化事实。保存真正失败则把新模型/工具准入闩住（core 的 `resume_required`，协议报为
`ThreadStorageState::resume_required`）：已在途结果在预留预算内收束，无法再接纳输出就取消该操作并
保留已产出内容，之后不再开始新的模型/工具工作。恢复是两个显式动作——
`retryThreadHistory(thread_id, fault_generation)` 只重试保存并等到该代次的固定保存目标落库，
`resumeThreadHistory(thread_id, fault_generation)` 才在 owner 重新核验代数、故障已消失、每个已发布
批次都已上交**并且故障时固定的 durable 目标真的落库**之后解除闩。故障发生时 owner 就从自己已发布的
事实上固定这一次恢复要到达的 durable fence（已上交的最新提交与存储尚未接管的队首批次中的较大者），
该 fence 在这一次故障代次被解除前不再移动；后端只是"接受了批次"或"不再上报错误"都不足以解除闩，
未到达 fence 的请求以 typed 拒绝（`ThreadError::StorageRecoveryPending`）返回，而不是在调用里无界等待，
以便调用方重试保存后再来确认。故障代次由后端命名时，owner 还要求**该后端自己核验过**的恢复代次正是
当前代数（pressure 里的 `recovered_generation`，不是"当前没报错"），否则以 typed 拒绝
（`ThreadError::StorageRecoveryUnverified`）返回——旧代次 writer 的成功不能替新故障作证。显式继续
走 owner 的**有界 mailbox**，因此在 Turn 仍暂停在存储安全间隙、只服务 mailbox 时同样可达；它不
经过只在 owner 空闲时才会被轮询的外层命令队列。空闲的 owner 也不等待下一条命令：后端 watch 是它
select 里的一等唤醒源，因此恢复发生后就绪性也会被及时发布，而闩仍只由这一次显式继续解除。
代数过期、仍有未上交批次或仍有压力都会被拒绝。checkpoint/blob
保存失败同样暂停；统计失败不影响准入，也不加入可靠保存屏障。

#### 在途输出的可靠预留

准入预留不只是"批次入队时再算一次编码字节"：模型 completion 累积器与工具进程输出本身也是
真实驻留内存，可能在提交前就超过进程预算。因此可靠后端向 owner 暴露一份**在途操作输出配额**
（`ColdStore::reserve_operation_output` / `charge_operation_output` / `release_operation_output`）：

- 模型/工具调用开始前，owner 先按 `MAX_OPERATION_OUTPUT_BYTES` 申请配额；后端从**同一** per-Thread /
  进程字节预算中授予实际可负担的上限（可能小于申请值），owner 把这份上限放进该操作的
  `OutputBudget` 并登记到 per-Thread 在途表。进程预算无余量时准入在安全间隙等待（mailbox 仍可达），
  而不是启动一个其流式结果留不下的调用。
- 生产者按"**已经接收**"的字节数单调计费；配额容不下的增量在写入前就被拒绝。模型侧把这条拒绝
  返回为流式错误，工具侧则以该配额拒绝替换过大的预览。
- 拒绝是真实截断：已接收的部分结果保留（模型 partial failure receipt 直接物化这份预览，工具保留
  已接收的进程输出与上一次预览），被拒绝的新片段不进入驻留事实。owner 以类型化
  `StorageFaultKind::QueueFull` 记录这次截断、递增故障代次并置 `resume_required`，之后不再开始新的
  模型/工具工作；语义上它不是"窗口版本回退"，与普通压力背压区分。
- 故障标记在**拒绝当刻**发布，不等到调用返回：生产者在流式任务上计费，不在 owner 栈上，因此
  `OutputBudget` 把首次拒绝通过 owner 自有的 `OutputRefusal` watch 报回；owner 的 mailbox
  等待循环监听它并立刻闩住类型化故障。工具在慢收束期间也必须能看到并阻止后续准入，不能把故障
  藏到它返回为止。owner 是该故障的唯一发布者，这不是第二份事实源。
- 预留不是"与结果并列再计一次"：**该操作自己产出的事实**接过这份预留。core 提交时用
  `ThreadWrite::output_claim` 指出产出该事实的操作，可靠通道在 `admit` 的同一临界区里把该操作预留
  中被这份事实覆盖的字节转成事实自己的计费：事实按 `min(该事实编码字节, 该操作仍预留的额度)` 接过
  预留，只有没被覆盖的差量才需要新余量，因此同一份内容只计一次——事实出现之前是预留，之后是事实
  本身。操作的中间事实（任务转 `running`、取消请求、attempt 进入 `running`）与终态结果同属这一次
  调用，因此同样带上该操作的 claim，共享同一份预留。进程预算被并行预留占满时（例如并发工具各占
  一份额度）结果仍能交接：否则每个结果都要额外空间、全部无法入队，而预留只在队列排空时释放，
  Thread 会永久卡在自己的队列之后，显式恢复也无用。
- 转移不允许未记账空隙，也不允许超预算接纳：接管与新增余量在同一个临界区一次完成，未被预留覆盖
  的字节仍要真实余量，装不下就是类型化 `QueueFull`（事实整体保留、稍后重试），绝不静默丢批次，
  也绝不为了"看起来排空"提前归还。预留**按操作归属**，不是一份可池化的 Thread 额度：只有事实自己
  指名的那一个操作能兑换它的预留，没有归属的事实（受理的输入、Turn/生命周期提交等）必须自己装进
  真实余量。否则一条无归属事实会花掉仍在途的另一个操作预留的额度，把"预留就是该操作在途输出上限"
  变成无界超预算接纳。
- 调用结束时剩余多少就归还多少：调用没产出事实即归还整份预留；已产出事实被接管的只归还未被覆盖
  的余量。`granted - accepted` 会把已接收字节永久留在进程预算里（而承载同一份事实的 effect 批次
  还会再计一次），归还整份原始授予又会丢掉事实自己的计费，两者都不对。若交接尚未完成（`admit` 因
  背压/故障未入队），owner 把剩余部分的释放推迟到队列真正排空的那个边界；期间被保留的完成体
  （未提交工具结果、未受理批次）仍占用它的预留。因此同一份内容**永不双计**，多轮成功调用后水位
  回到基线。

在途表按操作身份一条，数量等于并发操作数，不随历史增长；被推迟释放的集合同样只按并发操作数增长，
并在队列排空时清空。

**非流式结果的边界**：不主动调用 `progress` 的工具终态输出、以及模型终态无进度的大返回，不经过
上述增量计费。它们作为**原子结果**走正常 effect 准入：能装下就入队，装不下就是类型化
`QueueFull`，结果整体保留（未提交完成体或未受理批次）且仍占预留，Thread 暂停而非截断。也就是说
输出语义不依赖生产者调用 `progress`：流式片段在写入前被拒绝，原子结果则要么被接纳、要么让 Thread
带着完整结果暂停等待显式继续。

实时事件通道是有界观察通道，不是可靠日志。core ChatView 让慢消费者从当前窗口状态 Reset，
不让 GUI 重放 token 或自行拼接数据库；底层 effect 观察出现缺口仍须由 core 从有序存储及
未保存引用恢复，不能向 owner 请求已释放的历史 effect。

### 幂等回执与 by-id 可读条件

owner 的串行提交边界同时产出可持久化的幂等回执，使重复命令
跨窗口、跨重启返回同一结果而不重放执行：

- **输入回执**：`submitPrompt` 的稳定 `inputId` 以最小身份写入 history 身份索引，并与该
  effect 的 items/Turn 行在同一事务提交；重复提交先从该索引返回原回执，不重新受理、不重放
  历史。
- **交互/权限回执**：终态 interaction/permission 事实的 `(item_id, revision, digest, payload)`
  与产生它的 effect 在同一 history 事务写入；重复 resolve 只在回应与变更均相同时返回同一回执，
  冲突载荷明确失败，不重复授予执行权限或重放 effect。
- **消息去重**：消息以稳定 ID 幂等受理；已消费消息身份以有界账本（固定条数）驻留，超窗的重复
  由 history 中已接受的受理事实回答，账本不随历史增长。
- **任务/调用 by-id 权威**：任务状态与工具交付的权威在每会话 `history.sqlite`。按
  `(thread_id, call_id)`（或 `task:{call_id}`）主键可读取任务身份与状态而不加载正文；终态工具
  交付按同一身份读取完整正文。可读条件是事实已 durable 且属于该 Thread：非终态、跨 Thread
  或未知身份返回"无此事实"，调用方必须显式报告，不能当作已完成结果或新工作受理。

在 owner 仍驻留时，重复命令由 effect window 中最新提交直接回答；一旦对应批次 durable 并离开
窗口，同一查询改由上述 history 身份索引 / 终端事实回执与任务 by-id 读取回答，两者语义一致，
不因释放窗口而改写终态或重放副作用。

## 15.4 `state.toml` 保存

每个已加载 Thread 维护 dirty revision。正常情况下每秒捕获一次最新 checkpoint；状态未改变时
不写。单 Thread 最多存在一份正在保存和一份最新待保存 checkpoint，中间 revision 可合并，旧
写入不能覆盖新 revision。Turn 终态、停止、关闭、应用退出、重要 ownership 变化和显式恢复
检查点请求提前保存。

捕获不可变 checkpoint 后立即退出 owner 临界区；TOML 编码、文件同步和原子替换在持久化任务中
完成。统一原子写入流程为：

```text
编码完整对象
→ 写同目录临时文件
→ sync 临时文件
→ 原子替换 state.toml
→ sync 目录
```

`history_fence` 只能指向已由唯一 history writer 固定确认的 effect 水位；未终态正文没有
history 事实，也不能成为 checkpoint fence。发布新 checkpoint 前必须先：

1. `history.flush_through(checkpoint.history_fence)`；
2. 等待 checkpoint 引用的新 blob 已持久化；
3. 原子替换 `state.toml`，并将上一份有效文件保留为 `state.prev.toml`。

因此磁盘上可见的 checkpoint 不会引用尚未保存的历史事实或 blob。owner 不持锁等待保存屏障。
中间快照可合并；保存故障保留候选并暂停新模型/工具，不能把恢复所需状态视为可丢统计。

### checkpoint schema 2：外置正文

`state.toml` 的当前 schema 是 `ThreadCheckpoint.schema_version = 2`。当单条正文超过
`CHECKPOINT_BODY_THRESHOLD_BYTES = 64 KiB` 时，它离开 TOML，改以一条
`CheckpointExternalBody`（`slot` + `body` + 版本化引用）记入 `externalBodies`；引用是
`reference_version + digest("sha256:<64 hex>") + byte_len`，物理文件落在
`sessions/<storage-key>/blobs/checkpoint/<前两位十六进制>/<sha256>`。可外置的 slot 覆盖当前
上下文内容与工具调用参数、未消费输入/message、pending interaction/permission、未交付
delivery、扩展与运行事实、以及未结束 attempt 的诊断正文。

发布顺序被强制为：外置 → 写 blob（内容寻址、幂等、不覆盖已存在文件）→ 逐个校验 blob
字节与引用一致并 fsync 文件与目录 → 才原子替换 `state.toml`（并保留 `state.prev.toml`）。
激活时在把 checkpoint 交给任何调用方之前，按引用回读每个 blob 并校验 digest/length；缺失、
损坏或身份不符一律 fail closed，绝不当作空正文或截断正文。schema-1（全内联）文件仍按原样
可读；未知未来 schema 显式拒绝。checkpoint 恢复不依赖 `calls.sqlite`，且不重放旧 journal。

## 15.5 会话历史数据库

每个 Thread 使用独立 `history.sqlite`，面向稳定条目和 keyset 分页，不保存完整执行 journal。
该文件位于应用 home 下的 `~/.anywork/v2/sessions/<storage-key>/history.sqlite`，`<storage-key>`
是 Thread id 的 SHA-256 十六进制摘要（布局见 [17](./17-studio-storage.md) §17.1）。
最小逻辑 schema：

```sql
CREATE TABLE history_meta (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version    INTEGER NOT NULL,
    database_id       TEXT NOT NULL,
    thread_id         TEXT NOT NULL,
    applied_write_seq INTEGER NOT NULL
);

CREATE TABLE history_items (
    ordinal         INTEGER PRIMARY KEY,
    item_id         TEXT NOT NULL UNIQUE,
    turn_id         TEXT NOT NULL,
    kind            TEXT NOT NULL,
    revision        INTEGER NOT NULL,
    lifecycle       TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    payload         TEXT NOT NULL CHECK (json_valid(payload)),
    last_write_seq  INTEGER NOT NULL
);

CREATE INDEX history_items_by_turn            ON history_items(turn_id, ordinal);
CREATE INDEX history_items_by_kind_lifecycle  ON history_items(kind, lifecycle, ordinal);
CREATE INDEX history_terminal_turns_by_write_seq ON history_items(last_write_seq, ordinal)
    WHERE kind='turn' AND lifecycle='terminal';
CREATE INDEX history_items_by_kind            ON history_items(kind, ordinal);

-- 终态 Turn 首次提交的 last_write_seq 是通知身份，后续修订不得改写该序号。

CREATE TABLE history_turns (
    turn_id         TEXT PRIMARY KEY,
    first_ordinal   INTEGER NOT NULL,
    last_ordinal    INTEGER NOT NULL,
    revision        INTEGER NOT NULL,
    payload         TEXT NOT NULL CHECK (json_valid(payload)),
    last_write_seq  INTEGER NOT NULL
);

CREATE INDEX history_turns_by_last_ordinal ON history_turns(last_ordinal);

CREATE TABLE history_input_identities (  -- submitPrompt 跨重启幂等身份
    item_id        TEXT PRIMARY KEY,
    ordinal        INTEGER NOT NULL,
    revision       INTEGER NOT NULL,
    digest         TEXT NOT NULL,
    request_digest TEXT,
    payload        TEXT NOT NULL CHECK (json_valid(payload)),
    last_write_seq INTEGER NOT NULL
);

CREATE TABLE history_fact_receipts (     -- 终态交互/权限回执（与 effect 同事务）
    item_id        TEXT NOT NULL,
    revision       INTEGER NOT NULL,
    kind           TEXT NOT NULL,
    digest         TEXT NOT NULL,
    payload        TEXT NOT NULL CHECK (json_valid(payload)),
    last_write_seq INTEGER NOT NULL,
    PRIMARY KEY(item_id, revision)
);

CREATE TABLE history_message_identities ( -- 已受理消息最小身份；digest 可空
    message_id     TEXT PRIMARY KEY,
    item_id        TEXT NOT NULL,
    sequence       INTEGER NOT NULL,
    digest         TEXT,
    last_write_seq INTEGER NOT NULL
);
```

`ordinal` 在条目开始时分配，后续完成顺序不改变它；`revision` 是同一条目内容版本，
`last_write_seq` 是保存协调水位。更新必须满足：

- 相同 item/revision/内容幂等；相同 revision 不同内容冲突；
- 旧 revision 不覆盖新 revision；
- item 的 Thread、Turn、ordinal 和 kind 不可改变；
- terminal item 拒绝迟到的草稿或 delta。

投影侧对第一条负有义务：同一个 `(item_id, revision)` 必须是**逐字节相同**的 canonical 载荷。
内容版本由投影自己的计数器给出（与 effect 提交序号、保存水位无关），只有 Turn 绑定、正文、结构或
终态形状真的变化才递增；重复投影若这些都没变，就沿用上一次交付的放置与时间戳，只把每次投影都重新
打的时间戳排除在"内容变化"之外。否则同一 revision 会写出两个不同 payload，被上面的幂等/冲突规则
（正确地）拒绝成真实保存故障。

后台工具可以在 Turn 结束、owner 已裁剪该轮模型 attempt 后才交付结果。此时结果 effect
仍携带工具任务与交付事实；投影必须从已提交的同一 `tool` item 回读原始调用身份、参数和
Turn，再更新其状态，不能要求 checkpoint 留住已结束 attempt，也不能推测或重新生成调用。
实时视图先从会话窗口读取此身份，必要时回读历史库；历史 writer 按 `write_seq` 先保存
调用后保存结果。若两处都找不到已受理调用，保存失败并保留结果以供恢复，不能丢失或
将其当成新的工具调用。

payload 是条目自身的 JSON 正文，不含独立 `payload_format`/`payload_version` 列：需要一个无法解释
的原始载荷时以 `kind = 'raw'` 的条目保存，格式与版本随 `ThreadRawPayload` 存在 payload JSON 内
（见 [16](./16-core-contracts.md)）。`history_meta.thread_id`、`database_id` 与
`applied_write_seq` 是游标与水位身份，`history_message_identities.digest` 可空，仅用于迁移回填
留下的“身份已知、正文不可验证”行（读取方 fail-closed）。

core 在接管输入或收到 provider 的原始展示条目时分配顺序键；writer 消费已分配的键，
首帧显示不等待 SQLite 写事务。`watch` 可以合并中间预览，但不能跳过原始条目的顺序预留。
流式 delta 只更新内存组装与 GUI，不写入 `history_items`，也不因时间间隔保存中间草稿。
成功、失败或取消的终态事实随
同一 `ThreadEffectBatch` 受理，由该 Thread 的唯一 history writer 写入同 item 的最终 revision；
输入与消息身份回执仍与该 effect 在同一事务生效。结果不持久化 token 事件日志，checkpoint
必须继续等待 writer fence。

## 15.6 SQL 分页与 cursor

HistoryReader 提供 `Latest`、`Before`、`After`、`Around`（`TimelineQuery`，`item_id` 既接受
canonical item identity 也接受 opaque cursor token），全部使用 `ordinal` 索引和短读事务。
禁止全量读取后切片，也禁止用大 `OFFSET` 或最终时间排序。条目游标是版本化、自校验的
`TimelineCursor`：

```text
version + thread_id + database_id + ordinal + item_id + applied_write_sequence
```

数据库替换、跨 Thread、水位回退或身份不匹配 cursor 明确失败。`TimelinePage` 返回 items、本页
涉及的 turn 摘要、双向 cursor、首尾 item ID、`database_id`、`watermark`（=applied write
sequence）、`truncated` 与 `previews`。查询同时限制条目数、整页序列化字节（2 MiB）和单条预览
（`TIMELINE_ITEM_PREVIEW_BYTES = 256 KiB`）；因字节预算提前结束时，cursor 指向实际返回的最后
一条，不跳过内容。

超大条目以 `TimelineItemPreview` 返回：它对同一 item identity/ordinal/revision 只给预览，附
`total_bytes`/`preview_bytes`/`omitted_bytes`；完整正文另经 `TimelineItemQuery{item_id}` →
`TimelineItemRead{thread_id,database_id,watermark,ordinal,item}` 按身份回读，不激活 owner。

Turn 页同样是有界 keyset：按 `history_turns.last_ordinal` 倒序（`history_turns_by_last_ordinal`
索引、无 OFFSET/排序），受行上限与整页字节预算约束；一条超大 Turn 以连续窗口返回，其
`next_cursor` 也是同一个版本化 `TimelineCursor`（绑定 database/水位/条目），续传停在 Turn 内部
的真实位置。

唯一刻意的例外是按身份的整条回读 `TimelineItemRead`：它只取被点名的那一条正文，为取回超预算
条目而**不设字节上限**，且不缓存进 `HistoryStore`；遍历历史必须走 `page`/`turn_page` 的有界窗口，
不能靠反复 `read_item` 物化整段历史。

`history_turns` 只服务展示，不参与运行恢复。当前 Turn 的权威是 owner；恢复依据是
`state.toml`；历史 Turn 的展示依据是 `history.sqlite`。

## 15.7 调用记录

全局调用库位于应用 home 下的 `~/.anywork/v2/calls/calls.sqlite`（正文 blob 在
`~/.anywork/v2/calls/blobs/`，见 [17](./17-studio-storage.md) §17.1），按调用身份保存模型/工具
调用的冻结 binding、开始/结束时间、结果类别、用量、价格、请求/响应诊断和关联 Thread/Turn/Item
ID。大正文使用内容寻址 blob 引用。另有一张 `call_watermarks(thread_id, admitted_write_seq,
durable_write_seq)` 记录每个 Thread 的调用队列水位，供 `flush_through` 固定目标与续跑对齐。
调用记录不参与 Thread 恢复或 Timeline 排序；Thread 关闭不等待可丢调用队列，更不等待
其他 Thread 的统计。统计行缺失必须明确标记，不能显示为零。

逐 Thread 的调用已接纳/已落库水位均使用该 Thread 的 effect 序号，不混用全局调用队列
ticket；已落库水位只在调用库事务成功后推进，并在重新激活时从调用库恢复。队列明确
拒绝的 ticket 不是已落库事实。调用库提交后独立通知诊断观察者，不能依赖后续 history
effect 才刷新最后一次调用的水位。

同一调用身份重试写入幂等，冲突明确失败；未结束调用可以更新为终态，但终态不可被较旧观察
覆盖。计费和性能统计从调用库或其明确产品投影读取，不扫描会话历史。
effect 窗口缺口恢复计费时先等待已受理写入的固定 ticket，再只查询缺少 `billing_ref` 的
调用事实；已有计费正文不可从摘要列重构后再次投递，否则同身份的有损正文会触发冲突。

## 15.8 SQLite 与关闭

SQLite 默认启用：

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

单库单逻辑 writer 批量执行短事务；分页不跨用户滚动持有事务。`flush_through(ticket)` 只等待
调用时固定目标，不等待整个系统空闲。

Thread 关闭先封闭准入并收束当前工作，再保存实际终态 effect；history 达到固定水位后
保存最终 checkpoint。全部成功才释放 owner。保存失败保持 `Closing`、owner、未保存事实和重试
入口。会话数据库连接与 writer 按需创建，空闲且无订阅、无执行、无待保存数据时释放。

关闭与显式保存的控制命令对"owner 正在运行或暂停在 Turn 里"同样可达。`close` 先在发送关闭命令
前触发中断信号（取消在途模型/工具并让暂停的安全间隙返回），因此 Turn 会安全收束，关闭命令随后在
外层循环处理，最终固定水位保存不因暂停而无限等待。显式 `flush` 则走 owner 的**有界 mailbox**
（与显式继续同一条可达路径）：暂停的 Turn 只在安全间隙服务 mailbox，若 flush 只在外层命令队列里
等，它等的正是它想要变得 durable 的那个 Turn。flush 的目标在 owner 处理请求时固定为当时的
admitted 水位，之后的新准入不会把屏障推远；它不启动新工作，也不解除暂停。

## 15.9 恢复与格式演进

激活 Thread 时只读取并验证当前 `state.toml`；无效时可验证 `state.prev.toml` 并发布显式恢复
诊断。恢复将遗留 Running 状态收束为 Interrupted，不重建模型、工具、外部进程或取消令牌，
不执行历史副作用。需要继续执行时由 Studio 装配当前服务实例。

本次大版本不迁移旧会话；原有 `ThreadCommit` journal decoder、完整 replay 与共享
`sessions.sqlite` 不是 `v2/` 正常启动或聊天窗口的入口。旧数据原位保留，配置与凭据关联按
[20](./20-config.md) 升级；正常启动、查询、激活和订阅不能调用旧 replay。

未知未来版本、损坏数据、缺失迁移路径和未知必需 producer 格式均失败并保留原始字节；未知但
仅影响历史展示的载荷以 raw 历史条目保存。迁移不能用清空、默认状态或只有备份没有转换来替代。
