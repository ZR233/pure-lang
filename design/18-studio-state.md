# 18 - Studio 状态查询与领域生命周期

本文是 CQS 边界、公共 snapshot、activation、shutdown、自动 title 生命周期与产品投影的
唯一权威源；存储与 checkpoint 见 [17](./17-studio-storage.md)，UI 呈现见
[19](./19-studio-ui.md)。

## 18.1 CQS

Studio 使用 Command Query Separation。查询只读取 owner 已发布的 canonical snapshot；
初始化、激活、扫描 Skill/Profile、修复、重连和关闭只能由明确 typed command 触发。
Widget rebuild、stream resync 与 `read*` 查询不得写 SQLite/配置、访问网络或创建 runtime
owner。进程运行期间 Project、Thread、Agent、Workflow、Recovery 和服务目录的内存 owner
是活动事实源；`state.toml` 仅提供 activation 基线，SQLite 只承担历史/调用查询与异步持久化。

## 18.2 公共 snapshot

StudioState 聚合 projectDirectory、threadDirectory、agentDirectory、modeCatalog、settings、
recovery、MCP/LSP、provider usage、model performance 与 updater。Thread workspace 单独包含当前
状态、pending Interaction、ThreadRuntimeView、workflow 投影和有界 Timeline 窗口；不存在
taskDirectory。Thread 目录条目携带
会话工作区模式（`local | worktree`）与会话工作区地址 `workspacePath`，只读投影为侧栏与会话
展示的 canonical 事实，GUI 不推导、不本地改写，也不按模式分别取 Project 路径或工作树路径。
Settings snapshot 以 `modeModelRoutes` 暴露全局 Mode 默认 selector，并继续以 `roles` 暴露四个
系统子代理 route；Thread runtime snapshot 另行暴露当前 Thread 的 typed `modelRoute`、revision
与 available 状态。两者职责不同，GUI 不用 Mode 默认值覆盖已有 Thread 的 route。

Product event 携带完整领域 snapshot 或明确 revision：ProjectDirectoryChanged、
ThreadDirectoryChanged、AgentDirectoryChanged、ModeCatalogChanged、ThreadRuntimeChanged
等。Dart reducer 拒绝旧 revision，并可用一次全量 snapshot 从 stream lag 恢复；它不自行
推导 workflow transition。一次目录命令可以同时改变 Project 与 Thread，但每个实际变化的
领域最多发布一次事件；空 delta 不得提升 revision 或发布空事件。冷记录进入驻留/归档命令
的内存索引属于 owner 准备步骤，不单独形成产品事实或广播。归档只装载目录，不读取冷历史
或激活冷 owner；最终业务 mutation 才通过
directory command 发布 canonical delta。

全量 snapshot resync 按领域 revision 合并 `modelPerformance`，与增量事件使用同一 canonical
状态；本地已有快照不能遮蔽服务端更新，旧全量快照也不能覆盖已收到的新事件。
模型统计属于尽力而为的数据库投影：读取已提交数据与发布其展示 revision 串行化，
待写事实的早期空读不能占用最终落库结果的版本。写入结论到达后发布新快照；读取失败
保留上次可用数据并显式标记失败，待写与统计缺口也分别可见。

## 18.3 Activation

启动只等待迁移、全局 TOML、轻量目录和本地预置资源就绪。启动不打开会话数据库、不读取会话
checkpoint，也不调度全部会话恢复。全局 worktree/数据根审计由 runtime 持有的后台任务完成，
Recovery 使用现有 ObservedResource 发布 loading、ready 和失败状态，
显式 `recovery.retry` 重新发起审计，`recovery.read` 只读取当前状态。关闭先停止并等待审计，
再关闭会话与持久化。启动阶段通过 typed snapshot 向 bridge 提供存储打开、配置读取、
目录读取、资源准备及终态，不依赖普通产品订阅已经建立。
后台会话恢复与前台 activation 共用 assembler 的逐 Thread preparation reservation；已有
活动、正在准备或关闭的 owner 不接受后台恢复写入。冷激活在发布 owner 前完成 durable
settlement。审计结果按当前 lease revision 和已清理问题过滤，不能覆盖后续用户操作。

选择冷 Thread、提交输入或后台 child 继续时显式 activation。runtime 在一致读视图中校验
并加载 Thread checkpoint、working state 与 pending Interaction，全部成功后一次安装 owner。
Mode snapshot 和 workflow projection 与 session 同时恢复，不存在独立任务 runtime 恢复扫描。
Timeline 首窗在订阅建立后由 HistoryReader 查询；产品交互回答前按保存的父子顺序激活所需 Thread。
GUI 首个可用画面之后若已选中 Thread，复用普通打开命令激活该 Thread，不遍历其他 Thread，
也不自动继续模型或工具执行。

创建根会话是可失败的类型化命令，请求同时携带 Mode 与会话工作区模式。`worktree` 模式在发布
Thread 之前完成仓库解析、worktree 创建与 lease 落库；任一阶段失败都让命令失败、不留下已发布
Thread，GUI 保留输入草稿并显示失败原因。激活时工作区资源缺失或身份不符只显式失败并发布该
Thread 的 Recovery，不静默回落到 Project 根目录，也不在内存中改写已保存的工作区模式。
Thread 目录事实已经发布而后续步骤失败时，必须先按未启动会话补偿（关闭并归档）再返回类型化
错误，不能留下无法激活的已发布会话。

## 18.4 配置目录

Mode catalog 直接投影 Studio Mode 注册表的内存 snapshot：内置 Mode 由 Studio 启动时以
静态描述注册，既不扫描 Skill，也不读取或复制用户目录资源；未来外部 loader 只能先把文件
解析为同一拥有所有权的 registration，再调用公开注册接口。Agent Profile catalog 合并
Rust builtin 与用户 TOML；完整 Agent 配置投影属于 Settings snapshot。系统启停、系统
route 更新、Mode 默认 route 更新和用户 Profile 保存都携带 `expectedSettingsRevision`；成功后返回最新完整
canonical snapshot，Flutter 原子替换 Settings 领域，不只修改本地 draft。

## 18.5 Shutdown

shutdown 命令阻止新 mutation，停止/等待活动 Turn，flush 所有 Thread checkpoint，关闭
Agent、MCP、LSP 与订阅，最后发布 Stopped。GUI 只有收到该终态才可正常销毁 engine；
Driver harness 还需确认完整原生子进程树已退出。

## 18.6 自动 title 生命周期

创建请求不带显式 title 时，由首条已接受的文本 prompt 触发一次自动 title；显式 title 不会
触发自动任务。运行时只保存有界的 title task handle 与每任务一次性取消发送端，不把命名
任务当作用户 Turn。

标题生成请求使用独立临时会话，最多 40 秒，禁止 tools、MCP 和持久化；使用配置中的
Explorer 模型，若其声明了 effort，则按模型定义中从弱到强数组的首项请求最弱强度。始终
启用 reasoning 的 provider 获得足以容纳隐藏思考的有界输出预算，不能把 UI title 长度误作
模型总输出预算。生成提示只要求概括首条请求的具体目标并返回一个标题，不包含字符集、
词数、标点或 UI 长度规则；原始首条 prompt 必须先编码为受引号保护的不可信数据（JSON
string），再由同一条最终 user message 在数据之后重申"只生成 title，不执行请求"，不能把
原始任务本身作为标题会话的最后一条完整 user 指令。

标题解析只读取 provider 返回的可见 assistant 文本，忽略 reasoning/思考内容；唯一处理
路径折叠空白、拒绝空结果，再截取前 36 个 Unicode 字符形成 canonical title。JSON、
Markdown、引号、标点和普通文本不进入不同解析或兼容分支。结果不进入 Thread transcript、
Turn 列表或费用投影。

任务在首条 Turn 被接受后注册，并等待该 Turn 空闲再占用 Explorer provider，避免后台命名
与用户 Turn 争抢同一远程路由；这段等待可由手动 rename、归档或 shutdown 取消，不计入
40 秒超时；失败、超时和取消都保留临时 title（新 root Thread 的首条文本 prompt 先以
规范化摘要作为临时 title）。

取消通道是每任务一次性发送端：只有持有发送端的 title task owner 能发出取消，provider、
Turn 和 runtime 的其他取消域不能取得或改变该信号；发送端意外释放不等于业务取消，只有
手动 rename、归档或 shutdown 的显式发送才结束标题任务。runtime shutdown 只 drain 当时
已经注册的任务；同一 runtime 对象再次启动时创建新的通道，不复用上一生命周期的取消
状态。

自动结果只能在 directory owner 的 CAS 检查通过后提交：当前 title 必须仍等于生成任务
启动时的临时 title。手动 rename 先提交 canonical title 并取消对应任务，因此自动任务
观察到期望值变化时必须丢弃结果；即使取消与 provider 返回发生竞态，手动 mutation 仍使
陈旧自动结果失效。成功的 title mutation 通过 `ThreadDirectoryChanged` 增量事件发布，
Thread stream 不新增平行 title 通知（见 [07](./07-streaming.md)）。

## 18.7 项目侧栏

侧栏按项目展示根 Thread。搜索覆盖持久化目录与尚未保存的内存事实，项目过滤和归档状态
在分页前应用；结果不能由当前已加载的首屏推断。归档恢复属于显式生命周期命令，保留原
Thread 身份与历史，不创建副本。侧栏宽度、项目置顶与项目内会话置顶属于现有 UI
settings，由配置运行时持有并通过 Settings CAS 返回 canonical snapshot；搜索词、展开
状态和抽屉显隐为临时 UI 状态。概念与视觉设计见
[concepts/project-sidebar.md](./concepts/project-sidebar.md)。

## 18.8 产品投影与观察

输入显示投影读取提交时保存的产品正文、附件元数据和 presentation，不把模型附件注释反向
当成用户输入。输入载荷中的原始 request 用于幂等受理校验，不替代展示正文；其存在不能使
正常消息降级为原始记录，展示解析仍校验已知字段的类型并拒绝未知结构。
通用文本片段直接拼接并保留原始空白；未知输入格式交由原始记录查看，不能
默认为空文本。模型正文投影读取已保存输出与 model 回执，保留文本分片的实际字节和推理
正文；取消后返回或被拒绝的输出显示对应非成功终态，不作为正常回复；不支持的模型内容
明确暴露投影失败，不能静默变成空正文。Turn 产品投影依据保存的状态、提交时间和实测
耗时生成，不以数据库时间或当前时间替代；只有通用取消事实而没有发起者证据时显示
unspecified。Thread runtime 的上下文容量与模型身份来自最新模型尝试的已保存请求绑定，
在请求准入后即可投影，不等待成功响应；新尝试容量未知时清除前一模型容量，不能因失败、
取消或历史回放而沿用旧模型值；该容量沿可选 wire 字段传给 GUI，UI 不重新解析模型目录，
辅助推理的容量不覆盖主 Thread 上下文容量。

产品时间线 ordinal 在 effect 首次创建 Item 时从输入、Turn、模型尝试和工具调用顺序统一分配；
隐藏或暂时为空的投影保留槽位，后续正文出现或终态更新不能挤动已有条目；各投影的当前
输出集合不作为历史排序的事实源。工具时间线从已提交模型调用恢复原参数，从任务/许可
事实投影执行状态，从 delivery 读取实际交付内容；业务 payload 只作原始事实保存，不能被
UI 投影当作 exit code、审批或成功状态来源。成功且具有已保存结束 Turn 控制的 complete
工具交付，通过生产者的版本化 decoder 将原 summary 投影为最终回复，并使用交付时固定的
时间线位置；业务 payload 中的 completed 字符串不能独自生成完成状态。Composer 只有仍
持有非空 accepted input identity 时才消费对应 Turn 的提交结果；启动确认清除关联后，
后续执行失败只留在时间线，不能把空 identity 相等误判为当前输入失败。

Studio 拥有 Thread 产品观察任务，它是该 Thread **唯一的实时投影发布者**：直接消费 owner 发布的
不可变 effect，持有该 Thread 唯一的 live projection，把内容发布进共享会话并投影 Agent/Thread
目录、显式进展和模型计费，再把历史/调用事实交给各自 writer。它在没有 GUI 订阅时也照常消费已
成立事实，订阅只读取这份投影的帧，不驱动任何发布。普通提交直接从内存 effect 投影，不逐批
flush，也不回读历史：终态报告与目录摘要都由这份可靠内存投影结果生成，因此跨多条 effect 的长
Turn 会带上它此前提交的正文。owner 命令队列里的 flush 只在 owner 空闲时被处理，正在执行的 Turn
会让 owner 停在 Turn future 里，逐批 flush 会把实时产品事实拖到 Turn 结束。只有内存投影无法证明
它从该 Turn 第一条记录折起时才按 `items_for_turn` 从 history 读该 Turn 的有界切片重建终态事实，
而这只发生在两处冷边界：没有驻留投影的冷恢复，以及投影安装时一次性播种该 Thread 运行中及最新
Turn 的已提交条目（Turn 运行中才被重新激活或中途接管）。热路径没有 gap 重播种、也没有第二份
字典兜底：投影只消费可靠受理队列，一条 fact 只投影一次。冷读保留所需的 durable 屏障；该屏障
等待的是投影 owner 自己的接管票据（它已交回的最新提交），
不是绝对 commit 水位，因此本进程新激活、早先提交已经 durable 的 Thread 不会被一个无法再移动的
水位卡住；已从 Turn 起步折起的内存投影不在此列。除实时 effect 帧外，投影 owner 还以可靠通道的
水位变化为信号补齐迟到的初始化受理（准入被存储压力推迟到观察安装之后的事实不会被当成实时帧
重放，但照样要交回保存）。终态报告只带已提交的条目身份，未被提交收束的流式预览不进入父级报告与
目录摘要。实时投影为报告常驻的是有界累积器（终态文本按 identity 归并、可见 commentary 有上限、
最新工具条目与已提交身份集合），不是整轮过程正文；可变正文只在还能被改写时驻留，其余条目只保留
放置元数据。effect window 的释放来自存储后端 pressure 报告里的 typed durable 水位，owner 在提交
后的内存安全边界折入它并释放已覆盖批次，不需要显式 flush 或额外的 lease。投影自己的表（仍可改写
的驻留条目、放置元数据与报告累积器）在批次 durable 之后仍然驻留，因此投影 owner 向同一可靠通道
发布**绝对**驻留字节水位，Thread 预算与暂停判定把它和队列正文、在途预留一起计入；正常所需的报告
结果因此不被丢弃或静默截断，压力表现为暂停新模型/工具准入的安全间隙。投影无法完成的事实永远不会
被交接，所以它同时是一次保存失败：等待保存回执的屏障既观察协调器水位，也观察可靠通道自己的类型化
故障，两者都以真实原因 fail-closed，而不是等一个永远不会推进的水位。计费使用回执中的冻结
模型绑定、价格和用量，恢复从调用库按 inference identity 幂等读取；父级通知只处理观察注册后的
新增事实，使用稳定消息 identity
防止失败重试重复送达；观察失败保留可重试状态，由同步屏障返回错误；同步屏障除等待最终
commit 的投影、目录保存及计费保存完成外，还确认该 commit 的 history 已 durable——观察不再
逐批 flush，冷恢复与未驻留子会话的终态修复直接读 history，所以“观察已应用”必须蕴含“这次交回
已落库”，而该等待只按投影 owner 已交回的提交追溯，不等待尚未投影的后续提交。观察者更新产品目录时仅在目录 owner 的锁内修改
运行状态和更新时间，不重写预先读取的完整 Thread；并发标题、Mode 与归档操作保持当前
事实，已移出热目录的条目不被迟到观察结果重新插入。Agent 查询仅允许读取自身或同树
后代，不通过查询激活冷 Thread；会话及进展分页游标绑定数据库 ID 与 applied write 水位，进展保留每次显式
提交的完整 detail，超大单条以带字节位置的 UTF-8 JSON 分片继续读取，不能截断正文。
冷目录读取遇到未知 mode payload 保留已有目录元数据；只读历史展示原始载荷，真正激活
仍严格校验所需 producer codec。

Thread runtime 的缓存统计是独立的 typed 投影 `cache_usage`，包含参与统计的输入总量、
缓存读取总量、命中率及完整性标记；不复用总用量的完整性标记决定能否计算。样本要求
输入与缓存读取均已报告，读取不超过输入；若报告缓存写入，则读取与写入之和必须
不溢出且不超过输入。输出计数缺失不影响这一判定；缺失或矛盾的输入缓存计数不参与
累计，并将缓存统计标记为不完整。零输入零读取为有效样本，但零累计分母不产生比例。
未结束调用不贡献样本；已中断且无回执的调用表示统计不完整。已结束调用和压缩回执
在各自 canonical 身份下只计入一次，重投影、重同步和冷恢复得到同样结果；新事实
发布后使用当前样本重建累计投影，不在 GUI 另设增量账本。缓存有效样本使用 checked
计数累加，溢出显式报错，不发布回绕或截断比例。现有总 token、原始缓存计数和费用
保持原语义，派生缓存统计不改写持久化账单与日志。`ThreadRuntimeUsage` 的外部消费者
须迁移旧的 `cacheHitRate`、`cacheMissTokens` 到 `cacheUsage`：比例取其 `hitRate`，
未命中取同组 `inputTokens - cacheReadTokens`；旧字段不再提供，Bridge 与 GUI 须一并升级。

模型性能历史以冻结账单中的实际发送模型作为统计身份，并投影配置模型、Provider 返回模型和
`matched`、`mismatched`、`unreported`、`legacyUnknown` 四态结果。返回模型不参与费用或速度
汇总，旧账单没有观察事实时只能标记 `legacyUnknown`。

工具附件投影：Studio 从持久化工具媒体通过格式所有者的 typed 解码生成工具输出附件，
实时与历史恢复共用同一投影。附件身份沿用归档引用，MIME、字节数与可用尺寸必须对应
实际归档变体，未知尺寸不猜测；core 不解释媒体格式，Flutter 不解析不透明载荷。附件
读取入口同时支持用户附件与工具图片，但读取工具资源前必须验证 Thread 访问权以及该
资源属于该 Thread 的持久化工具媒体引用——仅有资源 ID 前缀、摘要或一个可访问 Thread
都不足以授权，客户端路径不能成为文件读取入口；未引用、损坏或丢失资源显式失败，不得
用原始 workspace 文件补回。已完成的工具交付从 Thread history 中的完整 delivery 读取；
尚未落库的交付从当前未持久化 effect 读取，不能依赖已被回收的 resident snapshot。

### 18.8.1 活动、内容窗口与存储状态（本轮已落地）

三条事实各自有唯一所有者，互不代替：

- **活动（activity）**：`ThreadActivity` 是当前“在做什么”的 typed 小摘要，与 ChatView 历史窗口
  正交。它由该 Thread **唯一的投影 owner**（观察任务里的 `ActivityProjection`）从它刚发布的
  `ThreadSnapshot` 推导（turns/attempts/tasks/permissions/interactions/input_execution/
  `model_execution`/`model_progress`/`tool_progress`），是**纯内存纯函数**：不读 `Session`、不做
  SQL、不整 Turn 回读历史、不新增第二份正文缓存。身份是 `activity:{turn_id}:{step}:{kind}`；
  身份不变时只有内容真变化才递增 `revision`，身份变化即新活动。阶段变化（含 `model_execution`
  的阶段推进）由 owner 在提交新 snapshot 后随活动帧补发，不依赖 `effect.turn` 变化；普通保存
  不会虚构 `persisting` 阶段。详情（reasoning/正文/工具参数与流式输出）按活动身份**按需**读，
  同样只从驻留 owner 事实推导，活动结束后返回 `Superseded`/`Ended` 而不回退旧正文；驻留摘要与
  详情计入同一可靠保留预算。

  运行中工具的实时输出不按“每来一个 chunk 就把累计全文再报一遍”投影：生产者持有共享不可变
  `ContentBlock` 链，只把变化的部分作为该调用自己 opaque 身份（例如 `command-output`）的
  `Append{chunk}` / `Replace{text}` 交回 owner（契约见 [07-streaming](./07-streaming.md) §7.4）。
  owner 把它存成该调用的 `ToolProgress`，实时投影直接共享最新 block、按该身份的 `version` 去重，
  不逐 chunk 复制或物化整段正文；只有 canonical/DTO 交付与详情展开才把当前有界窗口物化一次文本。
  活动行与 `retained_bytes` 因此按 `ToolProgress::bytes()` 计量而不物化正文，天然计入同一可靠
  保留预算。工具条目的 `ordinal` 取 owner 写在该任务上的真实开始序号（`TaskRecord.started_sequence`），
  不再恒为 `None`；Unix `started_at` 本会话没有该事实仍如实为 `None`。`latest_started` 取真实
  启动顺序：先比 owner 开始序号，同一提交批内并发创建的任务序号相等时再用该 Turn 的实际调用次序
  （`call_rank`）破平，最后按 `call_id` 稳定排序，不靠新到的输出或 wall-clock 猜。

  启动顺序本身是 owner 的持久化事实：`TaskRecord.started_sequence` 是 `Option<u64>`，带
  `#[serde(default)]`，旧 v2 的 `state.toml`/history payload 缺少该字段时解码为 `None`（回落到
  该 Turn 的调用次序，并发工具的真实启动次序就是调用次序，语义仍正确），不升 schema、不清库、
  不改旧目录。
- **内容窗口（ChatView）**：一个内容窗口只有一份所有者——core 的 `ChatView`/`ChatUpdates`，HTTP
  `/window`（`ChatWindowStream`）与 FRB `BridgeChatView` 都只映射它的差量。窗口统一
  pending/live/history：未落盘的流式条目与已落盘历史条目在同一个 canonical 条目集合里按
  ordinal 排序，窗口版本只在窗口事实变化时前进。订阅**只读**：状态流不再生成或转发 `Item*`/
  `Delta` 帧（过滤发生在生产端），正文更新不会再走一遍状态流，因此客户端也不必靠“末端丢弃内容
  帧”来避免重复，不会因此制造 revision 空洞。滚动/跳转统一用“锚点 + 方向”，窗口条目数恒有界。
  窗口正文按稳定 `ChatField` 字段身份输出 typed 增量（`Append`/`Replace`/`Remove`/`Unchanged`，
  一次 `UpdateItem` 只提交一次 item 内容 revision），item 内容 `revision`、窗口 `version`、保存
  水位 `saved` 与执行终态 `ChatLifecycle` 是四个互不替代的事实，宿主不再按 JSON 字符串拼正文；
  完整契约见 [07-streaming](./07-streaming.md) §7.4。
- **存储状态（storage）**：`ThreadStorageState` 是 typed 事实，不由错误文本推断。故障类别与
  generation、history 的 accepted/durable 水位来自 runtime 持久化协调器的 typed 值；执行阶段
  （`running`/`pausing`/`paused`）与压力暂停来自 core 的存储执行事实；core 的原始错误文本只作为
  诊断字段透出。首帧随 `ThreadSnapshot` 带权威存储状态，后续只用一条独立、typed 的存储 watch
  （协调器）加 core 执行阶段变化补发 `StorageChanged`，正文更新不会额外走状态流。订阅同时等待
  投影帧与这条存储 watch，并把交付过的状态按**值**比较，因此保存暂停即使没有任何内容/活动帧
  也会被及时送达；owner 侧只用一个无载荷的存储唤醒帧表示「快照里与存储相关的字段变了」，
  载荷仍由订阅者从 typed 来源读取，不是第二份存储事实。

  `resume_required` 是**显式继续**的 typed 闸门，与自动恢复的 `pressure_paused` 严格区分：
  短暂压力（队列/字节预算）在安全间隙自动等待并自动恢复，不改写执行阶段以外的事实；保存
  真正失败（`queueFull`/`writeFailed`/`writerUnavailable`/`noProgress`/`checkpointFailed`/
  `blobFailed`）会把 core 的准入闩住并把 `resume_required` 报为 `true`，新的模型与工具工作在
  下一次准入前停止，已有在途结果在预留预算内收束，预算耗尽则取消该操作并保留已产出内容。
  截断（在途输出超出可靠预留）本身就是一条 typed 故障：工具预览被拒当刻、以及模型流按预留
  拒绝增量当刻就发布，不等调用返回——慢收束的工具不会把故障和准入拦截藏到 commit 之后；
  发布者是该 Thread 的唯一 owner，不新增第二份存储事实。预留额度**按操作归属**：该次调用自己产出
  的事实（含任务转 `running`、取消请求、attempt 进入 `running` 等中间事实）在入队时接过这份预留
  （事实之后计自己这一份，调用结束时只归还未被覆盖的余量），所以同一份输出只计一次、正常调用的
  次数不会累积泄漏进程预算，进程预算被并行预留占满时结果也仍能交接。没有归属的事实（受理的输入、
  Turn/生命周期提交等）不得花别的在途操作预留的额度，必须自己装进真实余量，装不下就是类型化
  背压，绝不超预算接纳。

  恢复因此是**两个动作**，界面必须都提供，且任何一个都不是“看起来成功”的按钮：

  1. `retryThreadHistory(thread_id, fault_generation)`：按代数重试保存并等到该 Thread 的
     checkpoint/history durable fence 确认。它只把积压事实写下去，**不**恢复执行。
  2. `resumeThreadHistory(thread_id, fault_generation)`：把调用方核验过的代数交给驻留 owner，
     owner 重新读取后端 typed 报告，只有在代数匹配、故障已消失且每个已发布批次都已上交时才
     解除闩并把执行阶段置回 `running`。owner 在故障首次被观测时就从自己已发布的事实上固定这次恢复
     要到达的 durable fence（已上交的最新提交与存储尚未接管的队首批次中的较大者），并核验该 fence
     真的 durable——后端只是接受了批次、或只是不再上报错误都不算恢复。代数过期
     （`ThreadError::StaleStorageRecovery`）、后端仍在上报故障、仍有未上交批次、仍有压力或 fence
     尚未 durable（`ThreadError::StorageRecoveryPending`，typed 拒绝而非无界等待）都会被拒绝。
     显式继续走 owner 的**有界 mailbox**：暂停中的 Turn 停在存储安全间隙、只服务 mailbox，因此
     在 Turn 仍在运行时这条命令同样可达，不会因为外层命令队列要等 Turn 结束而自我死锁。

  重试成功不够，核验通过后也必须由用户显式继续：系统不会在重试成功后偷偷续执行。不驻留的
  Thread 没有活跃 owner，也就没有活跃的准入闩，`resumeThreadHistory` 返回成功但不激活任何
  Thread；重新激活的 Thread 不继承旧闩。失败后 `pressure_paused` 仍有压力时 `resume` 会被拒绝，
  界面保持暂停态直到压力消散后再次继续。

  `ThreadStorageState.canResume` 是这条恢复链的就绪事实，而不是界面推断：它要求闩仍然 owed、
  没有仍在报告的硬故障、每个已发布批次都已上交、没有字节阈值挡住，且本次故障代数固定的恢复
  fence 已 durable；后端命名了故障代数时，还要求**该后端自己核验**的恢复代次正是当前代数。
  它只有**一个**计算点：core 的存储 owner 用上述 typed 事实算出 `state.persistence.resume_ready`，
  投影直接采用它，绝不与 writer 自己的 `persistence.resume_ready` 做 OR——否则旧代次的 writer
  恢复会在 core 仍有新故障时代替 owner 点亮继续入口。writer 的 typed 报告只作为 `StoragePressure`
  输入（`durable_sequence`、`fault`/`fault_generation`、`recovered_generation`）进入 owner 的同代次
  核验，不是并列的第二事实源。任何输入变化都立即触达 owner：暂停中的 Turn 在安全间隙读同一条
  后端 watch，没有运行中 Turn 的空闲 owner 也把同一条 watch 作为独立唤醒源，重算并发布就绪性，
  因此保存重试让 fence 落地时界面无需任何其它帧就能看到继续入口变为可用；`resume` 本身仍重查全部
  条件，`canResume` 只决定是否提供入口，绝不替代守卫。

状态的发布者与内容的发布者是同一个人：该 Thread 的观察任务。没有 GUI 订阅时它照常消费与发布，
订阅只转发它广播的帧。

存储状态同样只由这一处推导：投影 owner 直接监听保存协调器的 typed status watch（故障类别、
代数、accepted/durable 水位）与该 Thread 自己的 owner 快照（执行阶段、压力闩、`resume_required`、
诊断文本），派生出唯一的 `ThreadStorageState` 并通过该 Thread feed 的独立 `watch` 发布。协调器
的 watch 是观察任务 select 里的一等唤醒源，所以"保存失败但没有任何内容帧"也会被立即发布，
而不是等某个不相关提交顺带到达。订阅者只镜像这个 `watch` 的值并在变化时交付 `StorageChanged`，
不再自己从协调器快照与 owner 快照拼接第二份存储状态——存储状态只有一个计算点。慢 GUI 不进入
任何门控：投影 owner 在无订阅时照常发布，writer 的保留释放也不等订阅者。

## 18.9 顶栏操作与 VS Code 打开

会话顶栏 actions 区（与智能体切换器、费用 chip 同级）提供「...」更多菜单。菜单项当前
只有「在 VS Code 中打开工作区」：本地打开该会话的工作区地址，远端项目经 Remote-SSH 打开
同一地址的远端形态，菜单项辅助行展示打开目标。打开目标只由会话工作区地址决定，不按
`workspace_mode` 分别取 Project 路径或工作树路径，GUI 不推导工作树布局。入口仅在探测到
VS Code 安装且当前会话存在所属项目时渲染；探测失败或无项目时菜单为空，整个「...」入口
不出现，不显示空占位。

打开通过系统 URL 协议完成，不派生 `code` CLI 子进程：本地使用
`vscode://file/<绝对路径>/`（尾斜杠表示文件夹，Windows 盘符形如 `vscode://file/c:/x/y/`，
路径按 URI 规则百分号编码），远端使用
`vscode://vscode-remote/ssh-remote+<别名>/<远端路径>`；Remote-SSH 的 URI 不携带端口与
密钥，连接参数由 `~/.ssh/config` 的 Host 别名解析（见 [22](./22-ssh-remote.md)）。URI
构建与协议白名单校验是纯函数；实际打开复用外部 URL 启动器，仅放行上述两种 vscode URI
形态，http/https 白名单不变。启动失败以界面提示回报，不自动重试。VS Code 探测在宿主
平台层完成：Windows 检查 PATH 与已知安装位置，Linux 检查 PATH 与
`x-scheme-handler/vscode` 处理器；结果缓存于进程内 provider，demo 与测试可覆写。
