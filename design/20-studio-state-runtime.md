# 20 - Studio 状态查询与领域生命周期

## 20.1 目标与约束

Studio 使用 Command Query Separation（CQS）。查询只读取 owner 已发布的 canonical snapshot；
初始化、激活、发现、检查、探测、同步、修复、重连、重置和关闭只能由明确的 typed command
触发。Widget 重建、页面刷新、窗口恢复以及 stream lag resync 都不是生命周期命令。

任何 `read*` 查询都不得：

- 写 SQLite、`config.toml` 或其他配置；
- 创建默认 Thread、注册或修复 ThreadActor、投递 Planner wake；
- 扫描 Skills 目录或访问网络；
- 启动、探测、重连、关闭 MCP/LSP 或其他子进程；
- 调用 reconcile、reset、repair 或 ensure。

系统不提供全局 `resetAll`、万能 StateManager 或第二套 durable projection。进程运行期间，
Project、Thread、Task、Agent、Recovery 及其目录的内存 owner snapshot 是活动状态 canonical facts；
SQLite 只提供启动恢复基线、未驻留聚合冷加载、历史分页和异步持久化。owner 激活后，查询和转换不得
回读 SQLite 来覆盖内存事实。

## 20.2 公共 observed state

跨 crate 的可观察资源统一使用 `ObservedResource<T>` 聚合，revision 与唯一状态一起发布：

```text
ObservedResource<T>
├─ revision: u64
└─ state:
   ├─ Uninitialized
   ├─ Loading(operation, operationId)
   ├─ Ready(value, lastCheckedAt?)
   ├─ Refreshing(value, operation, operationId)
   ├─ Stale(value)
   ├─ Degraded(value, operation, error)
   ├─ Failed(operation, error)
   └─ Stopped
```

每个 variant 使用独立 state struct。Refreshing/Degraded 明确拥有 last-known value；Loading/Failed
明确没有可用 value，因此不再使用 `phase + stale + payload` 推断是否可展示旧数据。失败携带 typed
`StateError { code, message, retryable }`。公开操作集合为 initialize、activate、reload、reconcile、
discover、check、probe、repair、reset 和 shutdown。

每次对外可见变化都递增 revision。异步操作捕获 operation id、
desired revision 与无 secret fingerprint；迟到结果只有仍匹配三者时才能提交。失败保留最后一次
成功 payload 时进入 Degraded，首次失败进入 Failed。只有实际执行外部观察的 discover/check/probe
更新对应状态中的 `lastCheckedAt`。

Studio runtime 自身使用 Uninitialized、Initializing、Ready、ShuttingDown、Stopped、Failed；禁止
任意 target transition。MCP、LSP、Provider Usage 和 Updater 在 observed resource 之上继续使用各自
typed state：server availability、LSP activity、更新下载/校验/启动阶段不降级为字符串或可选字段袋。
FRB 为每个资源输出具体 tagged union，Flutter 只消费 sealed canonical state。

Updater 的 canonical lifecycle 为 Disabled、Idle、Checking、UpToDate、Available、Downloading、
Verifying、InstallerLaunched、CheckFailed、InstallFailed。Available 及安装阶段必须携带已验证的完整
update；下载进度只存在于 Downloading/Verifying，检查错误与安装错误分别只存在于对应失败态。
Rust install event 先通过命令转换为该状态并 durable commit，FRB 事件与 Dart controller 直接消费完整
状态，不再各自维护 `phase + update? + error? + progress?`。

## 20.3 聚合查询与事件

`StudioRuntime::read_state()` 返回 `StudioStateSnapshot`，字段按领域保存完整快照：runtime、
projectDirectory、threadDirectory、taskDirectory、agentDirectory、settings、recovery、mcp、lsp、
skillsByProject、providerUsage、updater 和 persistence。它不接收 selected project/thread，不解析 workspace，
不创建会话，不加载磁盘配置，也不执行外部检查。

Thread 高频 workspace 继续独立：

```text
readThreadSnapshot(threadId)
subscribeThread(threadId)
repairThreadRuntime(threadId)
```

查询优先从已注册 ThreadActor 的 canonical snapshot 读取；actor 不存在时只读 SQLite 冷基线并返回
`runtimeAvailability=inactive`，不得让数据库行覆盖活动 owner。订阅是显式激活命令，可从冷基线创建
actor；纯查询和 transport 重同步不激活、不修复、不投递 wake。

`listThreadTurns(threadId, cursor, limit)` 是冷热历史查询：未驻留时直接读取 SQLite；驻留时先读取
ThreadActor 的驻留期 Turn 热窗口与完整 Item timeline，再用 SQLite 补齐更早页面。相同 Turn/Item
标识一律以内存覆盖，热 cursor 可以直接衔接冷历史，不得为了历史页把数据库快照回写到 actor。

`listThreadsPage(cursor, limit)` 与 Turn 历史使用同一个泛型冷热合并组件：SQLite keyset 冷页
（`(updated_at, id)` 倒序、`archived=0`）叠加活动热集合 overlay，同 ID 内存覆盖冷行、cursor 边界
排除重复。运行期会话列表、Task 视图与 Interaction 状态的读取一律内存优先；需要未驻留聚合时先
显式冷激活，不允许查询路径静默回读数据库覆盖热事实。

Product event 携带完整领域 snapshot：ProjectDirectoryChanged、TaskDirectoryChanged、
AgentDirectoryChanged、SettingsStateChanged、RecoveryStateChanged、McpStateChanged、LspStateChanged、
SkillsStateChanged、ProviderUsageStateChanged、UpdaterStateChanged、PersistenceStateChanged；唯一例外是
`ThreadDirectoryChanged`，它携带增量 payload（upserted entries、removed ids 与 thread directory
revision），由内存目录 owner 的 `DirectoryDelta` 提交派生（见 19.6），Flutter 按增量合并进分页窗口。
信封 sequence 只用于 transport lag；payload 自带领域 revision。

`subscribeShutdownProgress()` 是独立的短生命周期 typed 流，只在 shutdown 期间可用，不复用
product stream（它在关机早期被取消）。事件本身是 sealed 阶段状态，
固定顺序为 StoppingSubscriptions、CancellingTurns、FlushingPersistence、SuspendingTasks、
StoppingMcp、StoppingLsp、Stopped；只有 `FlushingPersistence` 承载 pending commit 数。只有 writer
确认 pending=0 才能进入后续正常关机阶段；落库失败时保持真实 pending 和 PersistenceState，不得
伪报完成。并发
shutdown 调用共享同一次阶段序列，`shutdownRuntimeForUpdate` 的 idle 关机复用同一协议。

Flutter 对每个领域分别保存 canonical snapshot。新 revision 才整体替换，相同 revision 幂等
忽略，旧 revision 丢弃；空 list、空 map 和 null 都是 authoritative value，不能解释为缺省。
Product lag 只调用 `readStudioState`；Thread lag 只重订阅并调用 `readThreadSnapshot`。

FRB 的 `readStudioState` 与 HTTP 的 `GET /api/v1/state` 都只机械调用该 query。共享操作由
`StudioOperation` 穷尽声明，FRB export 与 HTTP route/spec 测试必须分别覆盖全部共享项；初始化、
宿主 shutdown/进度、Driver fixture 和桌面 updater 安装另列为 host-only，不伪装成共享命令。

## 20.4 启动、Project 与 Thread

每个宿主只允许一次启动，顺序固定为：解析绝对 Studio home；取得 `runtime.lock` 独占锁；打开并
校验 SQLite；加载 `ConfigRuntime` 与 Usage/Updater last-known cache；从冷基线建立 Project 小集合
目录；执行持久任务恢复扫描并把非终态 Task 聚合分页装载到 TaskRuntime（终态 Task 是冷数据，
不参与启动装载）；启动 Thread framework；只为钉住集合（queued input、pending
Interaction、活动 Task 引用）恢复 ThreadActor、恢复交互并 materialize pending wake，其余 Thread
在订阅或提交输入时按需恢复；Thread 目录不做启动全量装载，活动热集合由钉住恢复、活动 Task
root 与运行期 `DirectoryDelta` 构成；初始化 MCP
owner 并发布 reconcile running；提交后台 MCP reconcile；同步内置 system Skills；发布 runtime
ready。启动只等待 MCP desired state 被 owner 接受，不等待 transport 连接、initialize、`tools/list`
或 startup timeout；后台结果通过 `McpStateChanged` 发布 ready/failed，MCP 失败不把 Studio runtime
降级为启动失败。

HTTP server 还必须成功绑定 loopback listener 才算 ready。`openapi` 子命令只生成规范，不构造
runtime、不取得实例锁。Ctrl-C/SIGTERM 首次触发停止接受请求、终止 SSE、完整 runtime shutdown
并等待 persistence/MCP/LSP 收束；第二次信号才允许强制退出。FRB 的桌面生命周期使用同一个
runtime 状态机，不保留第二套 `BridgeLifecycle`。

注册 durable child Thread 时，其运行时身份就是该 child 的 `ThreadId`；只校验它不等于所属
`rootThreadId`，不能把 child 自己的合法 `ThreadId` 误判成 root 身份。历史 closed child 也必须能
随目录恢复注册，旧版本持久化的合法 child Thread 不得让整个 Studio 启动失败。

启动在 SQLite 基线上创建 Project/Thread/Task/Agent/Recovery 目录 owner，并把活动 Task 聚合恢复到
TaskRuntime；此后产品目录只消费内存 owner 的类型化 commit，不再为活动事件重读数据库。

启动后 Flutter 读取 Studio 状态，在本地选择健康 Project/root Thread，再显式调用一次
`activateProject(projectId)`。activate 验证 workspace、切换 LSP membership、执行初始 LSP
probe 和 Skills discovery；不创建 Thread。相同 project/fingerprint 重复调用是 no-op。刷新、
重建和 lag resync 不调用 activate。

Project 可以合法地没有 root Thread。`openProject`、归档、普通查询、刷新与 resync 都不创建
默认 Thread。产品 UI 唯一的新 root 创建入口是首次提交使用的 `startNewThread` command；它在
同一生命周期临界区校验 Project 与输入、按请求 mode 以 `DirectoryDelta` 创建 root Thread 并
立即发布目录增量（turn 构建需要热集合中的目录事实）、提交首个 Turn；若首个 Turn 提交失败，
command 以归档 delta 移除尚未使用的空 Thread。SQLite 写入失败则进入持久化降级，保留已经
提交的热事实并暂停后续新工作。测试/Driver fixture 可以使用隔离的内部 seed 入口显式创建
Thread。

## 20.5 Settings 与 desired/live

`ConfigRuntime` 是 Settings 唯一 owner。启动时读取并校验 `config.toml`；此后
`readSettingsState` 只读内存。所有保存 command 携带 `expectedSettingsRevision`，先 CAS，
再使用 fail-closed credential preserve/replace/clear 和原子文件替换，最后发布完整
`SettingsStateSnapshot`。外部文件变化只有 `reloadSettingsFromDisk` 才能应用。

model/effort、instructions、permission、web search 和 general 只更新 Settings revision；只影响
未来 Turn。MCP 设置保存后提交 incremental reconcile；Skills 设置使受影响 project catalog
stale；provider endpoint/credential 变化使 Usage stale；只有 effective MCP fingerprint 改变才
reconcile 内置 MCP。desired 配置保存成功后不因下游应用失败而回滚，owner 通过
desired/applied fingerprint 不一致和 stale 表达失败。

root Thread 的编辑器显示 Settings desired route；child Thread 和历史/current Turn 的
`ThreadRuntimeSnapshot.model` 始终显示真实执行模型，不能被 Settings 保存改写。

## 20.6 MCP owner

MCP owner 提供 `reconcileMcp(desiredConfig)`、`resetMcp(scope)`、`readMcpState()` 和
`shutdownMcp()`。scope 是单 server 或 All。

启动 reconcile 使用与显式 reconcile 相同的串行 command 边界，但只在前台完成 desired
fingerprint 与 running snapshot 发布，候选 generation 的连接和 discovery 由 owner task 后台完成。
shutdown 必须先取消并等待该启动 task，再关闭 generation，禁止迟到结果把 stopped snapshot
覆盖回 ready/failed。

reconcile 对无 secret effective config 计算 fingerprint；与 applied 相同必须完全 no-op。未变化且
健康的连接复用，新增/删除/禁用/变化只影响对应 server。候选 generation 完成后原子发布；新
server 失败仍作为 unavailable 应用 desired generation。

reset 不复用目标 server，范围外 server 继续复用。单 server 只构造该 server 的候选；All 构造
全候选。reset 成功后原子切换；失败保留当前 live generation 并发布 failed/stale。旧 generation
在最后一个引用（活动 TurnToolLease 或进行中调用）释放后关闭。shutdown 是不可恢复终止态，
取消候选、拒绝新 lease，并关闭全部连接；之后只允许读取 stopped snapshot。

UI 的“刷新”只读状态，“重新连接”调用单 server reset，“全部重置”经确认调用 All；Settings
保存只用 reconcile。Flutter 对 command response 继续执行领域 revision replacement，迟到响应
不得覆盖更高 revision 的 `McpStateChanged` 事件。

## 20.7 LSP owner

LSP owner 提供 membership、probe、repair、reset、read 和 shutdown 六种边界。membership 只维护
workspace/server 定义、检查静态 workspace 特征并移除 stale client；不得执行 `--version`、
rustup 或网络请求。probe 才执行 `rust-analyzer --version`；缺少 rustup component 只记录 typed
状态。repair 只接受该 typed 状态，执行 `rustup component add rust-analyzer` 后重新 probe。

LSP query 可以启动已确认 available 的 client，但不能顺便 probe。启动失败回写 registry
availability/error 并发布状态。reset 对目标 client 执行 LSP shutdown/exit，清理 diagnostics、
activity 和 handlers；此前已启动的立即重启，未启动的回到 available/unstarted。shutdown 是
registry 终止态，不能用于 reset。workspace/server 删除时在生命周期锁外等待进程退出。
Windows 的 probe、repair 和 server process 全部使用 `CREATE_NO_WINDOW`。
Flutter LSP 页的页面进入和“刷新”只调用 read；probe、typed repair 与 reset 都使用稳定控件显式
触发，command response 仍按 LSP revision replacement。

## 20.8 Skills、Usage、Updater 与 Recovery

`SkillCatalogRuntime` 按 Project 持有 `SkillsStateSnapshot`。read 不访问文件系统，discover 才扫描
project/user/system/external；system Skills 只在 runtime 启动时安装。TurnFactory 冻结 catalog
revision，`skills_list`/`skill_view` 使用该 catalog；`skill_manage` 用冻结 catalog 校验，写入成功后
owner 为未来 Turn 重建，当前 Turn 保留旧 revision。

Provider Usage 和 Updater 都使用 last-known owner。read 只读缓存，check 才访问网络；失败保留旧
payload并标 stale。provider config 变化标 stale，删除 provider 时 authoritative 删除。
Provider Usage 的单 provider 状态为 Unsupported、MissingCredential、Ready、Failed；Ready 内再以
typed data union 区分 DeepSeek balance 与 Zhipu coding plan。命令携带 provider revision 与 operation
id，重复 operation 只有 payload 完全一致才 no-op。持久化复用 `app_settings` 的
`observed:providerUsage:v2` 与 `observed:studioUpdate:v1`，不新增
数据库 migration。update check 使用编译时当前版本；install 只接受缓存中的
`expectedRevision + version`，Flutter 不回传 URL、hash 或 manifest。

Recovery read 只读 registry；启动扫描属于 `startStudioRuntime`，重试、重扫、preview 和 cleanup
都是明确 command。Project/Thread 局部错误保持隔离，不升级为全应用失败。

启动恢复扫描还负责幂等收束历史悬挂：根 Agent 已 Faulted 而 TaskRun 仍非终态时，旧任务一次性写为
`Completed(Fatal)` 并收束子事实。类型化可恢复 Agent 故障可经 `RecoverFaulted` 验证后恢复同一会话
为 Idle；恢复只解除忙碌投影，不复活旧 Turn 或旧 TaskRun。聚合损坏和未知旧故障保持封闭，并向用户
提供诊断与复制到新会话路径。

## 20.9 并发与验收

持久化 owner 另行发布 Ready、Flushing、Degraded、Recovering、Blocked 五态和单调 revision。
Degraded、Recovering、Blocked 是全局新工作准入门禁，不是 Task 或 Agent 业务状态；停止、查询、
当前轮次收束和手动重试继续可用。Flutter 必须在控件和 controller 两层执行同一门禁，后端 command
入口再次校验，避免迟到界面状态绕过限制。

每个 owner 使用串行 command mailbox。同 fingerprint/scope 的 pending command 合并；新的 desired
revision 使旧结果失效；reset、shutdown 与 reconcile 在生命周期锁上串行，但状态锁内不得等待
网络、进程退出或长 IO。先替换 owner snapshot，再发布事件。Flutter 不用迟到 command response
覆盖更高 event revision。

副作用探针覆盖 SQLite mutation、actor registration、durable wake、process spawn、Skills scan、
Usage network、Updater fetch、MCP connector 与 LSP probe。所有 read 连续调用必须保持计数为零；
SQLite mutation 探针同时验证 mutation 只来自后台 write-behind writer 的批量事务，持久化降级、
自动恢复、显式耐久化屏障与关机 drain 有隔离测试，惰性恢复与 LRU 淘汰有回归测试。真实验收使用隔离
`PURE_STUDIO_HOME`、`cargo xtask run-gui --driver`、Driver health `ok`、
`set_frame_sync(false)`、稳定 `ValueKey`、SQLite 对比、Windows 进程审计和绝对路径截图；默认不使用
Computer Use。
