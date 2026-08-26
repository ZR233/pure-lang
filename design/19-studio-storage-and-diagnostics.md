# 19 - Studio 单库存储与诊断

## 19.1 数据库

Studio 默认只使用 `~/.pure/studio/studio.sqlite`，schema v13；测试和隔离验收可通过绝对路径
`PURE_STUDIO_HOME` 改写整个 Studio 数据根。数据库启用 WAL、foreign keys、五秒
busy timeout 和 synchronous=FULL；应用数据库连接池固定一个连接，mutation 统一经后台
write-behind writer 的批量事务串行化，snapshot、分页和设置查询共用该连接。

Skill Provider 与用户手势不增加数据库表，数据库 schema 仍为 v13。Thread wire schema v7 的 Skill
Item 保存 typed resource base、provider identity 与 `Tool | UserGesture` 激活来源；旧
`path + toolCallId`、缺失 provider 或未知字段严格拒绝。恢复审计发现旧 Skill Item 时阻断其 root
tree 激活并发布 cleanup recovery issue，不迁移、不默认填充、不自动删除现有数据库。

打开、检查、删除或重建 SQLite 之前，`StudioRuntime` 必须取得 Studio home 下
`runtime.lock` 的跨进程独占 OS 文件锁。锁文件只记录 PID、宿主类型和启动时间供诊断，占用
状态只以 OS 锁为准；文件内容不是 lease。所有 runtime clone 共享同一 lock owner，完整
shutdown/drop 后才释放。取得失败返回 typed `InstanceBusy`，不进入任何 SQLite/config IO。
该文件不属于数据 schema，不触发数据库或配置迁移。

所有 `read*` 查询严格无 mutation；测试注入 SQLite mutation counter 并验证重复 read 为零。
Provider Usage 与 Updater 的 last-known cache 复用现有 `app_settings`，键分别为
`observed:providerUsage:v1` 和 `observed:studioUpdate:v1`，不新增 migration。内存 owner snapshot
不是第二套 durable projection；进程重启后仅从 canonical tables/config 和这两个明确的 observed
cache 重建。这里的“不是第二套 durable projection”只说明内存不承担跨进程耐久化；进程运行期间，
Thread、Task、Project 及其目录 owner 的内存聚合是活动状态唯一可信事实，SQLite 不参与活动查询或
状态转换。

核心表：

- projects
- threads
- thread_inputs
- turns
- items
- interactions
- attachments
- app_settings
- task_runs、task_stop_events、task_issues、work_units、work_completions、review_rounds、merge_records
- thread_context_segments、thread_session_state

Turn、thread input、Item、Interaction、TaskRun、WorkUnit、ReviewRound、WorkCompletion、TaskIssue 与
Merge cleanup 等具有生命周期的记录只保存一份完整 `state_json`。需要筛选的表使用从
`state_json.kind` 生成的 stored `state_kind`；应用不得分别写 status、phase、reason、resolvedAt、
failure 或 terminal flags。身份、外键、ordinal、revision、计费与查询所需的稳定公共列继续关系化。

不存在 history 数据库、storage generation pair、history_gc_jobs、session snapshot JSON、agent
runtime snapshot、agent outcome 或 durable event journal。

## 19.2 提交

ThreadActor、TaskRuntime 与产品目录 owner 的内存 snapshot 是各自活动聚合的唯一权威实例。每次
mutation 先在串行 owner 中完成全部校验和纯投影，以一个复合状态原子替换 snapshot、递增 revision
并立即广播可观察事实，再追加待落库批次。SQLite writer 在后台按 owner/revision 顺序异步落库；
确认只推进 `durable_revision`，不得改变业务状态。

Thread 与 Project 的目录事实（创建、child 注册、mode 变更、归档、项目打开/关闭）使用同一条
提交纪律：目录命令在串行临界区内基于内存或冷加载的聚合完成校验，先以统一的 `DirectoryDelta`
（upsert 记录 + removal 标识）更新内存目录并广播 `DirectoryChanged` 事件，再把同一 delta 追加进
write-behind 队列。不存在命令路径上的同步直写 SQLite；SQLite 写入失败只影响持久化健康状态
（Degraded/Blocked 与新工作门禁），不回滚已发布的内存事实、不使命令失败。writer 队列 FIFO 保证
Thread 注册 delta 先于该 Thread 的首个 state commit 落库；owner 淘汰与关机排水的 `awaitDurable`
屏障同时要求该 owner 没有未落库的目录 delta。

不再存在“等待 SQLite 后才能发布”的 Immediate 提交。流式增量、Turn 终态、input claim/consume、
Interaction、conversation recovery、Task 终态和计划者唤醒都以内存 commit 为可见边界。显式
`awaitDurable(owner, revision)` 只用于正常关机、owner 淘汰、工作目录或分支删除以及其他不可逆
外部动作。进程异常退出时尚未确认的内存事实可以丢失；重启以 SQLite 最后成功写入的 revision 为
恢复基线。数据库保持 `synchronous=FULL`，批量事务继续摊薄 fsync 成本。

writer 不得在重试耗尽后退出或删除批次。三次快速重试失败后进入 Degraded，继续以最多三十秒间隔
退避重试；首次成功后进入 Recovering，积压清零后自动回到 Ready。结构冲突、数据库损坏或无法安全
处理的容量错误进入 Blocked。公开的 `PersistenceState` 携带 revision、待写数量、最旧未保存修订号、
首次失败时间和无敏感内容的错误摘要。Degraded、Recovering、Blocked 暂停新 Turn、新 Task 和新资源
创建，但停止、查询、当前活动轮次收束和手动重试保持可用。

队列上限仍为 1024 个批次，其中 768 个用于普通提交，256 个为已启动生命周期的终态收束预留。启动
新生命周期前必须取得终态许可。Timeline delta 只存在于内存 owner 与 live stream，不进入持久化
队列；writer 只能合并已经发布后的冷存储事实，不能合并或延迟 live delta。普通区满时受控取消继续
产生持久事实的轮次，并使用预留位置提交终态，禁止静默丢弃。正常关机必须等待 pending=0；强制退出
必须明确告知最后耐久修订号之后的数据可能丢失。

Faulted 可以保留来源 Turn 作为诊断身份，但该身份不再属于活动 Turn；同一个内存 commit 必须把
对应 Turn 写为 Failed 并立即发布。诊断 Turn 缺少失败结果、身份不一致或结果不是失败终态时，在
owner 替换前拒绝转换。合法的 Faulted commit、SQLite 错误或发布失败均不得再次把 Agent Faulted。
类型化可恢复运行时或协议故障可以通过 `RecoverFaulted` 在验证快照、修订号和 transcript 后回到 Idle；
聚合损坏与未知旧故障保持 Faulted。旧字符串故障只有命中已知 reasoning 分块错误且会话审计通过时
才自动升级为可恢复协议故障。恢复不复活旧 Turn 或旧 TaskRun，下一条输入创建新的执行轮次。

Item ordinal 是内存权威事实：由 ThreadEventBus（每线程唯一投影者）在通知首次应用时按
到达序分配 `max(ordinal)+1`，此后不可变；分配后的规范化通知同时供给内存快照、订阅广播
与 SQLite 落库，三处同源。落库原样保留 item ordinal，不再从数据库派生顺序事实；恢复时
`replace_snapshot` 以已落库 ordinal 种子化总线，续号从 max+1 继续。delta 不写库；terminal
更新同一 Item 完整 payload。历史查询按 `(thread_id, turn_sequence)` keyset 分页，不使用
OFFSET。Turn、thread input 与 submission 的 ordinal 在 writer 单写者事务内从 durable max
派生：FIFO 单写者保证该派生结果与内存到达序一致，且不存在第二个写入方可以观察到中间态；
这是 item 机制之外刻意保留的持久层内部顺序派生边界，不向上游协议暴露派生细节。

ThreadEventBus 还按开始顺序保留当前驻留期观察到的 Turn 热窗口；Turn 完成并清除 `active_turn` 后
仍留在该窗口。历史分页先按 cursor 选择热 Turn，再从 SQLite 读取冷页；相同 Turn 或 Item 标识以内存
状态覆盖数据库内容，Item 按 owner 分配的 ordinal 重排。cursor 位于尚未落库的热 Turn 时，从热窗口
继续后再衔接冷历史最新端，并排除 cursor 及其后的热标识，保证跨冷热边界不重复、不倒序。

模型 transcript 与 Studio Timeline 分开持久化。`thread_context_segments` 按 Thread revision 保存
`append | replace`：普通 checkpoint 只追加新增 `ModelContextItem` suffix；compaction、回滚或截断
在同一事务删除旧 segment 并写入新的 replacement baseline。恢复时按 revision fold segment，
拒绝断层、非法首段或损坏 payload。

runtime 从每次 Thread transition 的提交前后 session 自动派生 transcript mutation；TurnFinished、
rollover 和 child 注册不得以 `context=None` 丢弃已变化 transcript。Replace、session snapshot、
Turn terminal 与 mailbox 状态属于同一个持久化批次，并在 SQLite 中以单一事务应用；事务失败只
回滚该次数据库尝试，不回滚已经发布的内存事实。writer 保留原批次并进入 Degraded 或 Blocked，
重试成功后只推进 `durable_revision`；重同步只能从内存 owner 修复消费者投影，不得用旧数据库
基线覆盖仍驻留的活动聚合。

`thread_session_state` 每个 Thread 只有一行，replacement 保存 pinned working context、session note
与 prompt generation 状态。Evidence Ledger 更新只覆盖这行的有界 working state，不复制完整
transcript，也不直接进入 provider request。`items` 不再包含 `contextPatch`，也不再保存 `provider_private_payload`；
`contextCompaction` 可作为无正文内部审计 Item 保留，Bridge 查询永不返回。transcript replacement
不删除 Studio Timeline，working state 也不能代替 `ThreadRuntimeSnapshot` 的当前产品事实。

Conversation recovery 不新增 SQLite 表、不提升 schema 版本。`AgentWorkingState` 通过 serde default
保存版本化 `ConversationRecoveryState`：单调 recovery revision、累计 rolled-back Turn 范围、最近
恢复记录、恢复前后 transcript hash、移除 input/item 数与固定
`externalStatePolicy=preserved`。旧数据库缺少该字段时读取为空状态。Timeline 查询根据累计 Turn
范围派生 `ThreadContextDisposition::RolledBack`，不得更新或删除历史 Turn/Item 行。

conversation transcript、working state、recovery marker、mailbox 状态与 Thread revision 在同一
事务写入新的 replacement baseline。重复 recoveryId 返回已提交结果；expected runtime/session
revision 冲突不产生部分更新。Thread 局部重建保留 pinned handoff、Evidence Ledger、session note、
Task/WorkUnit owner 与 usage。TaskRun 不保存项目 Git snapshot 或 fingerprint；WorkUnit 只保存
worktree 创建后的资源事实和 caller 声明的审计数据。

`task_issues` 以 `(task_run_id, source_turn_id)` 唯一保存来源 Thread/Turn/agent、WorkUnit 或
ReviewRound，以及包含完整 `TurnFailure`、处置语义和解决事实的 canonical state。Recoverable issue
只在同一来源 Thread 成功开启后续 Turn 时解决；首个 fatal issue 在 TaskRuntime 的一个 owner
transition 中把 Task 与 children 一并结算为失败终态，迟到 child 事件不能覆盖该事实。热提交后
立即关闭进程内 agent，SQLite 异步跟随。同一项目的多条活动 Task 不创建项目租约，所有权、独立
工作目录和版本比较更新负责隔离。

每次模型 inference 的 usage、provider/model、价格快照和费用明细保存在对应 Turn 的
`model_json`；`usage_json` 保存同一批次重算的 Turn 聚合。Turn 终态聚合 usage 与 runtime usage
终态通知在同一内存提交后立即可见，落库由批量事务异步跟随。相同 inference ID 的相同记录幂等，
内容冲突使持久化进入 Blocked；历史费用始终使用
当时保存的价格和币种，不能按当前 catalog 重新计算。

usage 区分 prompt、缓存读取、缓存写入、completion 与 reasoning token。归一化时缓存读取与
写入之和不得超过 prompt token，cache miss 是 prompt 减缓存读取；reasoning 已包含在 provider
报告的 completion 时不得重复计费。OpenAI 缓存写入、普通输入和缓存读取分别按价格快照计算；
模型声明缓存写入 token、有效策略为 OpenAI cache key 且目录缺少显式写入价时，写入价按普通
输入价的 `1.25 ×` 冻结，
DeepSeek 继续按未命中输入、命中输入和输出计算。每个 inference 同时保存按币种的估算费用与
相对全未缓存输入的缓存节省；不同币种永不相加。`TurnBillingRecord` 的 JSON 版本可独立演进，
旧字段使用 serde default 读取，不要求数据库 schema 升级。

每个 inference billing 记录还可附带版本化 `orchestration` 指标：工具 schema/result 估算 token、
tool call 与 Tool Search/Programmatic 计数、并行候选/实际并行、工具批次 wall-clock/关键路径、
只读缓存命中，以及 Responses continuation/retry/fallback 分类。Turn 聚合只做可加计数、token 与
时长汇总；比率和节省量由聚合后的原始量计算，避免平均值再平均。旧记录缺少该对象时按零值读取。

## 19.3 不兼容库重建

启动先只读检查 canonical `studio.sqlite` 的 `user_version`、`quick_check` 与必需表/列
fingerprint。Studio schema 只保留当前版本 v13 一个事实：不存在跨版本迁移链，任何非 v13
库一律按不兼容处理，不迁移、不归档、不导入：

1. 关闭本次检查创建的全部数据库连接。
2. 再次证明目标是配置解析得到的精确 canonical Studio 数据库文件。
3. 精确删除 `studio.sqlite`、`studio.sqlite-wal` 与 `studio.sqlite-shm`；不使用 glob，不删除目录。
4. 创建空 schema v13 并完成 fingerprint 校验后才向 Runtime 提供 store。

删除或重建失败属于应用级致命错误，由错误页重试；不得在半初始化数据库上继续。重建只处理
Studio 数据库文件，不扫描、删除或修改 Project、worktree、branch、attachments、凭据或构建
缓存等库外数据。旧会话、Task 与 ownership 元数据直接丢弃；因此失去 owner 的磁盘 worktree
也不能被自动 GC。

## 19.4 归档与附件

归档 project/thread 是可恢复的逻辑操作：命令先在内存更新目录（移除 Thread 条目、关闭
Project），再把同一 `DirectoryDelta` 交给 write-behind writer，由后台批量事务标记 Project
关闭和 Thread 已归档，保留 Turn、Item、Interaction、attachment row 与附件文件。有活动 Task
时拒绝归档 Project。Task worktree 只能走 preview-confirm-revalidate 产品清理，普通归档
不删除库外文件、worktree 或 branch。

`reset_agent_sessions_for_root`、项目清理时的 `cancel_thread_for_project_cleanup`、重启恢复
扫描的 `reconcile_task_agents_after_restart` 与 `mark_restart_user_input_recovered` 是仅有的
"权威冷原语"：它们在 actor 已退役/线程可能不驻留、或 owner 尚未在内存建立的前提下直接结算
SQLite，属于用户确认后的破坏性重置或单线程启动期收束边界（结算完成后 owner 才从冷基线装
载），不得模仿其模式新增运行期直写路径。

## 19.5 诊断

Rust 使用 tracing，只记录稳定 ID、kind、数量、字节数、事务耗时、通道积压、lag 和 outcome，
不记录完整 prompt、模型上下文、secret 或工具结果。数据库 mutation、恢复、订阅和 Task 操作
都携带 correlation ID。日志保留与 crash 文件策略沿用 Studio 现有实现。

任何返回 `StudioError` 的错误映射必须先生成 correlation ID，再以同一个 ID 写入安全结构化日志。
未分类错误日志至少包含 correlation ID、稳定操作名、错误类别和诊断字节数；上下文可追加 Thread、
Turn、Task、Interaction、revision 与 generation 等稳定标识，但不得记录原始错误链、prompt、工具
参数或结果、配置正文、header、绝对私有路径和凭据。FRB 错误、HTTP 响应体与响应头继续透传同一 ID。

缓存诊断只记录 provider/wire/model、prompt generation、有效缓存策略、前缀变化原因、各固定层
hash、token 分类、费用与缓存节省。cache key、prompt、配置正文、header、凭据、工具参数和结果
均不得进入日志。Bridge 只暴露 generation、策略、变化原因与聚合 usage；内部 hash、逐 inference
billing 和 working-context 内容不进入 Flutter timeline。

诊断额外汇总 Driver reconnect、provider retry/fallback、conversation recovery mode/目标 Turn、
transcript 前后 hash、恢复次数与失败原因，但不记录 prompt 或被移除正文。验收 manifest 固定 prompt
hash、runId、Task generation、WorkUnit/worktree resource identity、初始时间、全局 deadline、恢复
次数和每次 attempt 日志目录，用于证明原始 prompt 只提交一次以及 recovery 前后 durable owner
保持不变；不记录或比较 Task 项目 Git fingerprint。

工具编排诊断仅暴露与 inference/Turn 关联的聚合数值和稳定 enum；schema、工具参数、工具结果、
program 正文、caller 原始 JSON 与 cache key 均不得进入日志或 Flutter timeline。compaction 指标只保存
替换前后 token 估算，不能保存被移除正文。

## 19.6 内存驻留与冷热目录

Thread 目录不是全量内存索引，而是"活动热集合 + SQLite 冷分页"：

- 热集合只包含仍有内存事实的 Thread：钉住集合恢复的 Thread、活动 Task 的 root、驻留 actor
  以及目录 delta 尚未耐久化的新 Thread。LRU 淘汰（已等待耐久化）或归档时条目移出热集合。
- `list_threads_page` 以 `(updated_at, id)` keyset 从 SQLite 冷分页（`archived=0` 过滤），再以
  热集合 overlay：同 ID 内存条目覆盖冷行，cursor 边界排除重复，保证跨冷热边界不重复、不倒序。
  该合并算法与 Turn 历史的冷热合并是同一个泛型组件。`readStudioState` 快照只携带首页目录条目
  与窗口游标。
- 启动不为目录做全量扫描；只有 Project 小集合目录在启动时整体载入内存。

完整会话（transcript、working state、mailbox、Interaction）按需恢复：订阅、提交输入或 Task
恢复引用时从 canonical 表加载并创建 ThreadActor；启动只为钉住集合（queued input、pending
Interaction、活动 Task 引用）主动恢复。驻留 actor 由 manager 的 LRU 双端队列管理，订阅、提交
或修复时移到队尾；空闲判定为无活动 Turn、无活跃订阅且无 pending input，超容量时从队首淘汰。
淘汰前必须等待该 Thread 的目标 revision 与未落库目录 delta 耐久化，被淘汰 Thread 保留全部
durable 状态，再次订阅时按需恢复。

TaskRuntime 使用同一驻留原则，且启动只恢复活动 Task：以 `list_active_task_runs` 为源分页装载
非终态聚合并 seed 耐久基线，终态 Task 一律作为冷数据，不参与启动装载。活动 Task 及未追平耐久
修订的终态 Task 不得淘汰；只有终态且 `durable_revision >= hot_revision` 时才可移除完整聚合，
Task 目录条目继续保留。再次访问该条目时是显式冷激活，从 SQLite 恢复聚合基线，不允许活动事件
用数据库快照覆盖驻留聚合。Task 目录同样只有"活动 + 已显式激活"的热条目；线程被选中或订阅时
显式激活其最新 Task，供该 Thread 的任务视图使用。

启动恢复 transient pending Interaction 时，先恢复对应 Thread owner，再读取该 root Thread 最新
Task。最新 Task 已 `Completed` 时，通过恢复后的 canonical owner 取消该 Thread 及 Task-owned child
Thread 的 pending Interaction；非终态 Task 的 PlanConfirmation 与 UserInput 继续按恢复原则保留。
该对账只扫描已经因 pending Interaction 被钉住的 Thread，不把终态 Task 全量装入内存，也不新增
运行期 SQLite 直写边界。

会话内容查询遵循同一窗口语义：驻留 actor 的热窗口与未确认事实从内存读取，更早 Timeline 按
`(thread_id, turn_sequence)` keyset 分页从 SQLite 读取；跨越冷热边界时按 item identity 和 ordinal
以内存覆盖数据库记录。未驻留 actor 必须已经耐久化，查询可直接回源 SQLite。
