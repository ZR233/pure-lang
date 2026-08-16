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

系统不提供全局 `resetAll`、万能 StateManager 或第二套 durable projection。SQLite 仍是
Project、Thread、Task 和 Recovery 的 canonical facts；内存 owner 只拥有其领域 live runtime
和 last-known observation。

## 20.2 公共 observed state

跨 crate 协议统一使用 `ObservedStateMeta`：

```text
ObservedStateMeta
├─ revision: u64
├─ phase: Uninitialized | Ready | Running | Failed | Stopped
├─ updatedAt: i64
├─ lastCheckedAt: i64?
└─ stale: bool
```

`Running` 和 `Failed` 携带 `StateOperation`；运行中另携带 `operationId`，失败携带 typed
`StateError { code, message, retryable }`。公开操作集合为 initialize、activate、reload、
reconcile、discover、check、probe、repair、reset 和 shutdown。

每次对外可见变化（进入 running、成功或失败）都递增 revision。异步操作捕获 operation id、
desired revision 与无 secret fingerprint；迟到结果只有仍匹配三者时才能提交。失败保留最后一次
成功 payload 并标记 stale；首次失败使用领域定义的 authoritative empty。只有实际执行外部观察的
discover/check/probe 更新 `lastCheckedAt`。

## 20.3 聚合查询与事件

`readStudioState()` 返回 `BridgeStudioStateSnapshot`，字段按领域保存完整快照：runtime、
projectDirectory、threadDirectory、taskDirectory、agentDirectory、settings、recovery、mcp、lsp、
skillsByProject、providerUsage 和 updater。它不接收 selected project/thread，不解析 workspace，
不创建会话，不加载磁盘配置，也不执行外部检查。

Thread 高频 workspace 继续独立：

```text
readThreadSnapshot(threadId)
subscribeThread(threadId)
repairThreadRuntime(threadId)
```

查询从 repository canonical state 读取，并通过 `tryGetThreadHandle` 合并已注册 actor 的 live
overlay。actor 不存在时返回 `runtimeAvailability=inactive`；订阅返回
`runtimeNotActivated`，只有 repair command 可以注册、恢复并重新投递 durable wake。

Product event 携带完整领域 snapshot：ProjectDirectoryChanged、TaskDirectoryChanged、
AgentDirectoryChanged、SettingsStateChanged、RecoveryStateChanged、McpStateChanged、LspStateChanged、
SkillsStateChanged、ProviderUsageStateChanged、UpdaterStateChanged；唯一例外是
`ThreadDirectoryChanged`，它携带增量 payload（upserted entries、removed ids 与 thread directory
revision），由常驻内存目录索引派生（见 19.6），Flutter 按增量合并进分页窗口。信封 sequence 只用于
transport lag；payload 自带领域 revision。

`subscribeShutdownProgress()` 是独立的短生命周期 typed 流，只在 shutdown 期间可用，不复用
product stream（它在关机早期被取消）。事件携带阶段 enum、阶段序号与 pending commit 计数，
固定顺序为 StoppingSubscriptions、CancellingTurns、FlushingPersistence、SuspendingTasks、
StoppingMcp、StoppingLsp、Stopped；`FlushingPersistence` 的完成事件必须携带 pending=0。并发
shutdown 调用共享同一次阶段序列，`shutdownRuntimeForUpdate` 的 idle 关机复用同一协议。

Flutter 对每个领域分别保存 canonical snapshot。新 revision 才整体替换，相同 revision 幂等
忽略，旧 revision 丢弃；空 list、空 map 和 null 都是 authoritative value，不能解释为缺省。
Product lag 只调用 `readStudioState`；Thread lag 只重订阅并调用 `readThreadSnapshot`。

## 20.4 启动、Project 与 Thread

`startStudioRuntime` 是唯一启动 command，顺序固定为：打开并校验 SQLite；加载
`ConfigRuntime`；加载 Usage/Updater last-known cache；执行启动恢复；修复 root Thread role；
启动 Thread framework；建立全部未归档 durable Thread 的内存目录索引；只为钉住集合（queued
input、pending Interaction、活动 Task 引用）恢复 ThreadActor 并 materialize pending wake，其余
Thread 在订阅或提交输入时按需恢复；初始化 MCP
owner 并发布 reconcile running；提交后台 MCP reconcile；同步内置 system Skills；发布 runtime
ready。启动只等待 MCP desired state 被 owner 接受，不等待 transport 连接、initialize、`tools/list`
或 startup timeout；后台结果通过 `McpStateChanged` 发布 ready/failed，MCP 失败不把 Studio runtime
降级为启动失败。

注册 durable child Thread 时，其运行时身份就是该 child 的 `ThreadId`；只校验它不等于所属
`rootThreadId`，不能把 child 自己的合法 `ThreadId` 误判成 root 身份。历史 closed child 也必须能
随目录恢复注册，旧版本持久化的合法 child Thread 不得让整个 Studio 启动失败。

启动后 Flutter 读取 Studio 状态，在本地选择健康 Project/root Thread，再显式调用一次
`activateProject(projectId)`。activate 验证 workspace、切换 LSP membership、执行初始 LSP
probe 和 Skills discovery；不创建 Thread。相同 project/fingerprint 重复调用是 no-op。刷新、
重建和 lag resync 不调用 activate。

默认 root Thread 只由以下 command 创建：`openProject` 的创建事务、显式 `createThread`、归档
最后一个 root Thread 的同一事务。普通查询永远不创建 Thread。

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
持久化复用 `app_settings` 的 `observed:providerUsage:v1` 与 `observed:studioUpdate:v1`，不新增
数据库 migration。update check 使用编译时当前版本；install 只接受缓存中的
`expectedRevision + version`，Flutter 不回传 URL、hash 或 manifest。

Recovery read 只读 registry；启动扫描属于 `startStudioRuntime`，重试、重扫、preview 和 cleanup
都是明确 command。Project/Thread 局部错误保持隔离，不升级为全应用失败。

## 20.9 并发与验收

每个 owner 使用串行 command mailbox。同 fingerprint/scope 的 pending command 合并；新的 desired
revision 使旧结果失效；reset、shutdown 与 reconcile 在生命周期锁上串行，但状态锁内不得等待
网络、进程退出或长 IO。先替换 owner snapshot，再发布事件。Flutter 不用迟到 command response
覆盖更高 event revision。

副作用探针覆盖 SQLite mutation、actor registration、durable wake、process spawn、Skills scan、
Usage network、Updater fetch、MCP connector 与 LSP probe。所有 read 连续调用必须保持计数为零；
SQLite mutation 探针同时验证 mutation 只来自后台 write-behind writer 的批量事务，Immediate
flush 边界与关机 drain 有隔离测试，惰性恢复与 LRU 淘汰有回归测试。真实验收使用隔离
`PURE_STUDIO_HOME`、`cargo xtask run-gui --driver`、Driver health `ok`、
`set_frame_sync(false)`、稳定 `ValueKey`、SQLite 对比、Windows 进程审计和绝对路径截图；默认不使用
Computer Use。
