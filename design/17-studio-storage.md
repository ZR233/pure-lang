# 17 - Studio 存储、迁移与诊断

本文是 Studio 文件布局、目录加载、会话激活、历史/调用数据库、迁移和恢复诊断的唯一权威源；
Thread checkpoint 与 writer 合同见 [15](./15-session-storage.md)，安全边界见
[04](./04-security.md)。

本次是大版本的会话存储断代：旧版本的会话目录、目录摘要、产品库、调用统计与迁移状态
保留在旧路径，**不导入、不激活、不自动归档或清空**。新运行时在独立的 `v2/` 数据根
创建新的会话与目录。`config.toml` 与用户 Agent Profile 等非会话配置沿用原位置，
provider 配置升级继续由配置模块负责并保护凭据引用；旧会话转换不属于本版本的启动
路径或验收目标。

## 17.1 数据布局

Studio home（`~/.anywork/`）的布局为：

```text
~/.anywork/
├── config.toml                     # provider/model/Skill 配置
├── agents/<agent-id>.toml          # 用户 Agent Profile
├── v2/
│   ├── settings.toml               # 本版本 UI 与产品设置
│   ├── workspaces.toml             # 本版本 Project/Workspace 定义
│   ├── catalog.toml                # 本版本新会话目录摘要
│   ├── sessions/<storage-key>/     # 每个新 Thread 一个目录（storage-key = id 的 sha256）
│   │   ├── state.toml
│   │   ├── state.prev.toml
│   │   ├── history.sqlite
│   │   └── blobs/                  # 附件正文与外置 checkpoint 正文
│   │       └── checkpoint/<xx>/<sha256>
│   ├── calls/
│   │   ├── calls.sqlite            # 本版本尽力而为调用统计
│   │   └── blobs/
│   └── migrations/                 # 仅本版本迁移状态
├── migrations/                     # 旧版本迁移状态，保留但不读取
│   ├── session-migration.json
│   ├── layout-publication.json     # 仅布局切换在途时存在
│   └── session-archive/
└── studio/                         # 产品库与进程级资源
    ├── v2/studio.sqlite            # 本版本产品库
    ├── studio.sqlite               # 旧版本产品库，保留但不读取
    ├── runtime.lock                # 跨进程独占锁
    ├── attachment-drafts/          # 临时附件草稿
    └── skills/.system/             # 构建期预置系统 Skill
```

canonical 会话、调用与迁移目录位于 `~/.anywork/v2/`；产品库位于
`~/.anywork/studio/v2/`，进程级锁与 Skill 仍位于 `studio/`。
`<storage-key>` 是 Thread id 的 SHA-256 十六进制摘要，不是原始 id。
旧共享库 `studio/sessions.sqlite`、旧逐产品 `studio/calls.sqlite`、旧全局
`studio/attachments/` 及旧迁移 staging 目录原样留在旧数据根，本版本不读取或切换它们。

会话数据库始终位于应用 home，不写入本地或 SSH 项目工作区。进程启动前取得
`~/.anywork/studio/runtime.lock` 的跨进程独占锁；锁文件 PID/宿主/启动时间只用于诊断，所有权
由 OS 文件锁决定。

`catalog.toml` 只保存轻量目录摘要：Thread/parent/workspace 身份、title、mode、role、工作区
模式和地址、创建/更新时间、归档状态与 `status`。它不保存当前上下文、完整 Turn、历史正文、
请求/工具结果或执行状态，也没有独立的活动摘要字段：侧栏活动指示由内存热集合覆盖，持久
`status` 只反映最后一次已提交的目录状态。目录摘要可用于侧栏，不可用于判断工具或 Turn 当前
是否仍在运行。

`workspaces.toml` 保存 Project/Workspace 定义和稳定引用；物理 worktree ownership 使用版本化
lease 记录，保留 `prepared | active | preserved | cleanupRequested | cleaned`、owner Thread、
repo/path/branch/base/revision。目录展示与物理 ownership 是不同职责，不能互相推导。

## 17.2 启动与按需激活

启动顺序固定为：取得锁 → 打开本版本独立存储 → 读取全局配置 → 读取本版本 workspace/catalog 摘要 →
创建空会话注册表 → 发布 GUI 首屏。此时加载的会话状态、打开的会话数据库和历史条目均为零。

启动不遍历全部 `state.toml`，也不在首屏之后后台激活全部历史会话。全局恢复只处理本版本的
数据根目录级资源和孤立物理 lease。GUI 首个可用画面出现后，若展示的是已选中 Thread，
由 GUI 对该 Thread 发起一次普通打开；无选中 Thread 时不打开。其他会话的内部恢复推迟到
显式打开、执行或需要人工处置该会话时。

打开 Thread 时：

1. 校验 catalog 身份和祖先关系；
2. 合并同 Thread 并发打开；
3. 读取并验证一次 `state.toml`；
4. 纯恢复内存 owner，不执行模型/工具；
5. 建立增量订阅；
6. ChatView 在同一订阅边界取得当前内存首帧（最近最多 32 条）；Session 的尾部缓存按需
   从历史 keyset 查询补齐最多 100 条，活跃/待保存条目直接按同一身份合并，无需等待 writer。

读取历史页面不激活 owner。离开 GUI 页面只释放订阅和远离窗口的 GUI 数据，不自动停止仍在执行
的 Thread。空闲、无订阅、无待保存数据且没有必须存活资源的 owner 在最终 checkpoint 后可释放。

## 17.3 配置和目录文件

`config.toml` 保存 provider、模型 route、指令、Skill/MCP 等配置；`settings.toml` 保存 UI 和
产品设置；`workspaces.toml` 保存 workspace/project；`catalog.toml` 保存会话目录。所有文件使用
版本化 schema、CAS revision 与统一原子写入，不在正常路径双读旧格式。

用户 Agent Profile 继续一个稳定 ID 一个 TOML 文件。系统 Profile 不写用户文件。Provider secret
保存在 OS 凭据库；配置迁移改变 provider ID 时必须先写入并验证目标凭据，再切换引用。

目录变更与会话 checkpoint 不共享伪造的跨文件事务。创建/归档等跨资源命令使用显式可恢复
步骤和迁移/操作记录：目标文件全部验证并持久化后才发布新 catalog revision；失败保留可重试
状态和原资源。

## 17.4 历史、调用与实时流

每个会话由会话管理器持有可靠的未提交 effect 队列，history writer 借用队头并写入
`history.sqlite`，事务确认后才释放；同一事务记录任务身份、终态工具交付和交互回执。
全局 call recorder 独立、尽力而为地写 `calls.sqlite`；其积压和故障不阻塞执行或历史确认。
旧调用库不进入本版本的历史库，旧用户数据原样留在旧数据根。writer 退出后原队列由会话
管理器保留，待旧写任务结束后按历史水位重试。

新 Session 首次分配 ordinal 前，只读一次本会话 `history_items` 中已持久化条目的最大 ordinal
作为 seed（空库为 0）。Session 在内存中按身份分配唯一、正数、处于 SQLite 整数范围内的
ordinal；重试同一身份保持原值，未落盘的分配在进程重启后不恢复。实时预览和 writer 共用
同一个 Session 分配结果；writer 只持久化已赋值的 ordinal，缺失、改变或重复均失败关闭，
不使用数据库自增键、预留表或写入时回退分配。旧库中的预留表不参与 seed，也不被迁移。

已写入但尚未关联 Turn 的消息条目，随后可以用同一身份与 ordinal 绑定到它的 Turn。若它
早于该 Turn 已持久化的首项，writer 必须在更新该条目的同一事务中向前扩展
`history_turns.first_ordinal`；除此合法绑定之外，Turn 首项不能被任意改写。

产品投影在 effect 提交时形成 canonical Turn/Item/Interaction 条目变更；同一身份
同时进入 core 的 TimelineState 与历史 writer：

```text
Thread owner
├─ 当前状态 snapshot → 状态订阅
├─ Item/Turn/task effect → reliable history channel → history writer
├─ TimelineState → ChatView 快照/批量差量 → GUI
└─ model/tool statistics → lossy call recorder
```

首次 Thread snapshot 只包含当前状态、pending interaction、runtime、workflow 和目录引用，不含
完整 Timeline。core ChatView 是热/冷 Thread 的同一历史阅读入口：`Latest` 首帧 32 条、
按身份 `Around`、Older/Newer 每次 32 条、每窗口上限 96 条；Session 共享最近最多 100 条，
另外按字节约束。core 负责尾部缓存、活动条目、可靠未保存事实与 SQLite 页的合并、去重和
窗口版本；Studio 的 HistoryStore 只实现按顺序键查询/按身份正文读取及写入，不能另设
100 条 SQL 页缓存。GUI 不再维护“实时转历史”的两个对象，也不持有随会话增长的消息列表。
活跃 Thread 的 reader 与 writer 复用同一数据库身份与初始化状态；新库创建至 schema/meta
完成之间，reader 等待本句柄 writer 初始化，而冷读不存在的库仍返回空且不创建文件。损坏库
继续按错误上报，不以重试将其伪装成空历史。

打开或重连在 core 同一窗口版本边界取得首帧与下一次通知；首帧不能等待当前 effect 的
history/checkpoint 水位，也不能让 GUI 自行合并 SQL 与实时 overlay。历史查询先持有可靠
未保存引用，再读短 SQL 事务；并发 commit 造成的重叠按 item_id/revision 合并，不能漏行。
`Latest` 跟随尾部，`Around` 保持旧阅读窗口并提示新消息，跳转/返回最新不扩大窗口。
慢消费者基线过期由 core 给 Reset；普通滚动分页不经过执行 owner、不 flush writer、不
激活冷会话。存储故障以独立 watch 发布，不排在可能积压的 GUI/历史增量之后。

## 17.5 Snapshot scheduler 与保存诊断

每个已加载 Thread 最多一个 snapshot scheduler。它维护正在写的一份和最新待写的一份，使用
跳过错过 tick 的一秒 interval；序列化与文件 IO 在 owner 临界区外完成。scheduler 先等待
checkpoint 的 history/blob fence，再原子发布 TOML。

公共持久化状态至少暴露：

- state dirty/saving/durable revision；
- history/calls admitted/durable write sequence；
- pending operations/bytes、oldest pending age、in-flight bytes；
- 历史故障类型、代次、执行暂停相位，以及是否因压力暂停准入；
- 统计投影的丢失情况，缺失值不得解释为零。

正常 Turn 不等待持久化。历史队列满、历史或 checkpoint/blob 保存失败时保留未确认事实，
在下次发布、模型或工具启动安全点暂停；状态改变独立唤醒空闲的会话。人工按会话重试原队列，
补入未受理批次并等待该次暂停的固定保存水位，再以故障代次核对后恢复原执行位置。
无后续输出、关闭 GUI 页面、停止 Turn 均不得释放待保存历史。正常退出不能把未保存事实当作
关机成功；强制退出必须明确告知未提交内存内容的风险。

状态错误不回滚已提交内存事实。关闭、归档和 shutdown 只等待相关 Thread 的固定 ticket；其他
Thread 持续写入不能阻塞当前操作。

## 17.6 worktree 与资源恢复

worktree lease 是物理资源唯一 ownership。会话的 `workspace_mode` 与 `workspace_path` 是目录
展示事实，不因 lease 缺失自动改为 local。激活时 lease 或物理身份缺失/不匹配明确失败并发布
Recovery preview；不静默切换到项目根目录。

Recovery preview 包含 revision、branch/base/head、dirty/changed files 和缺失/冲突原因。只有
`preserved`、孤立 lease、清理失败或身份不匹配资源进入人工清理列表；活动 Thread 的 active
lease 不是清理候选。显式 cleanup 在服务端重新验证前置条件后执行。

物理资源创建、变更和清理必须在对应 checkpoint/catalog fence 前耐久化其 ownership 记录。
文件系统或 Git 操作成功而目录保存失败时保留资源和补偿记录，不删除现场冒充回滚。

## 17.7 大版本隔离与配置迁移

本版本不运行旧会话 journal、旧产品库或旧调用库的转换、导出与归档，也不提供
`migrate-legacy-storage` 命令。`~/.anywork/studio/sessions.sqlite`、旧产品数据库、调用库、
附件与旧迁移报告均保留在原路径；新会话只读写 `v2/` 和 `studio/v2/` 下的独立数据。
旧数据不可作为新会话缺失条目的补全来源。只有全局 provider 等非会话配置按配置契约升级；
配置和 OS 凭据关联必须保留，失败不得用默认配置覆盖旧文件（见 [20](./20-config.md)）。

## 17.8 数据库和发布验收

新 history/calls 数据库启用 WAL、FULL、foreign keys 与 busy timeout。打包时记录并验收实际
链接的 SQLite 版本；不能只依据 ORM crate 版本宣称具备某个 WAL 修复。本次迁移和发布验收至少覆盖：

- 旧会话、调用库和产品库在原路径保持字节不变，既不导入也不自动归档；
- 仅供应商配置、凭据关联等非会话配置安全迁移，失败不得覆盖旧配置或丢失凭据引用；
- 新建 v2 会话的数据在 WAL 中已提交、重复启动和写入中断恢复；
- checkpoint 不越过 history/blob fence；
- writer busy/失败/确认不明时不丢批次；
- 大历史启动不扫描会话，分页成本不随历史前缀线性增长；
- 冷历史查询不激活 owner、不执行模型/工具、不修改文件。

## 17.9 诊断与脱敏

错误包含 operation、Thread/Turn/Interaction/call identity 和脱敏 correlation ID，不记录 token、
Mode Prompt 正文、用户 credential 或未授权请求正文。失败 artifact 可包含 migration 状态、schema、
数据库 quick check、写入水位、队列压力、GUI/Driver 日志和文件 diff；敏感原始备份受权限保护。

Timeline 的工具 item 在开始时分配稳定身份；provider identity 后到只更新同一 item。历史缺口、
重复 ordinal、同 revision 内容冲突、checkpoint fence 越界或 blob hash 不匹配必须失败关闭，不能
用当前工作区内容或当前工具渲染器补回。
