# 17 - Studio 存储、迁移与诊断

本文是 Studio 文件布局、目录加载、会话激活、历史/调用数据库、迁移和恢复诊断的唯一权威源；
Thread checkpoint 与 writer 合同见 [15](./15-session-storage.md)，安全边界见
[04](./04-security.md)。

## 17.1 数据布局

Studio home（`~/.anywork/`）的布局为：

```text
~/.anywork/
├── config.toml                     # provider/model/Skill 配置
├── settings.toml                   # UI 与产品设置
├── workspaces.toml                 # Project/Workspace 定义
├── catalog.toml                    # 会话目录摘要
├── agents/<agent-id>.toml          # 用户 Agent Profile
├── sessions/<storage-key>/         # 每个 Thread 一个目录（storage-key = id 的 sha256 十六进制）
│   ├── state.toml
│   ├── state.prev.toml
│   ├── history.sqlite
│   └── blobs/                      # 附件正文与外置 checkpoint 正文
│       └── checkpoint/<xx>/<sha256>   # 外置 checkpoint 正文（> 64 KiB）
├── calls/
│   ├── calls.sqlite                # 全局模型/工具调用记录
│   └── blobs/
├── migrations/                     # 一次性迁移状态、源指纹与备份/归档
│   ├── session-migration.json
│   ├── layout-publication.json     # 仅布局切换在途时存在
│   └── session-archive/
└── studio/                         # 产品库与进程级资源
    ├── studio.sqlite               # 产品库；辅助/对象存储
    ├── runtime.lock                # 跨进程独占锁
    ├── attachment-drafts/          # 临时附件草稿
    └── skills/.system/             # 构建期预置系统 Skill
```

canonical 会话、调用与迁移目录位于应用 home 根（`~/.anywork/`）；只有产品库与进程级资源在
`studio/` 子目录。`<storage-key>` 是 Thread id 的 SHA-256 十六进制摘要，不是原始 id。
一次性迁移的 staging 目录（`sessions.staging/`、`calls.staging/`）与旧共享库
`studio/sessions.sqlite`、旧逐产品 `studio/calls.sqlite`、旧全局 `studio/attachments/`
同处应用 home 范围，仅供迁移边界读取/切换。

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

启动顺序固定为：取得锁 → 完成或恢复迁移 → 读取全局 TOML → 读取 workspace/catalog 摘要 →
创建空会话注册表 → 发布 GUI 首屏。此时加载的会话状态、打开的会话数据库和历史条目均为零。

启动不遍历全部 `state.toml`，也不在首屏之后后台激活全部历史会话。全局恢复只处理迁移状态、
数据根目录级资源和孤立物理 lease。GUI 首个可用画面出现后，若展示的是已选中 Thread，
由 GUI 对该 Thread 发起一次普通打开；无选中 Thread 时不打开。其他会话的内部恢复推迟到
显式打开、执行或需要人工处置该会话时。

打开 Thread 时：

1. 校验 catalog 身份和祖先关系；
2. 合并同 Thread 并发打开；
3. 读取并验证一次 `state.toml`；
4. 纯恢复内存 owner，不执行模型/工具；
5. 建立增量订阅；
6. 已受理 effect 通过 history writer 固定屏障后，HistoryReader 查询首个 SQL 可见窗口。

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

每个会话 history writer 直接消费 `ThreadEffectBatch` 并写入该会话 `history.sqlite`；全局 call
recorder 消费模型/工具调用事实并写 `calls.sqlite`。两者都不保存 owner snapshot，也不向执行
路径提供可变状态。

已写入但尚未关联 Turn 的消息条目，随后可以用同一身份与 ordinal 绑定到它的 Turn。若它
早于该 Turn 已持久化的首项，writer 必须在更新该条目的同一事务中向前扩展
`history_turns.first_ordinal`；除此合法绑定之外，Turn 首项不能被任意改写。

产品投影在 effect 提交时形成 canonical Turn/Item/Interaction 增量：

```text
Thread owner
├─ 当前状态 snapshot → 状态订阅
├─ Item/Turn effect → history writer
├─ Item/Turn notification → GUI
└─ model/tool call fact → call recorder
```

首次 Thread snapshot 只包含当前状态、pending interaction、runtime、workflow 和目录引用，不含
完整 Timeline。History API 使用 `Latest/Before/After/Around` 直接 SQL 分页；热 Thread 与冷
Thread 使用相同 reader，不建立驻留期全历史 `TimelineIndex`。
活跃 Thread 的 reader 与 writer 复用同一数据库身份与初始化状态；新库创建至 schema/meta
完成之间，reader 等待本句柄 writer 初始化，而冷读不存在的库仍返回空且不创建文件。损坏库
继续按错误上报，不以重试将其伪装成空历史。

打开或重连为避免间隙，先注册事件接收端，再冻结 owner 当前已提交的 revision，直接等待唯一
writer 的 history 与 checkpoint 水位覆盖该 revision，最后查询数据库窗口；这就是首窗屏障。
它不向正在执行 provider 流的 owner 排队 `Flush`，后续 effect 仍由已注册接收端交付。
未终态流式正文不进入 writer 或
`history.sqlite`，只作为实时 overlay 与 SQL 首窗合并。期间收到的事件按版本化通知封套合并；
封套携带
`epoch` + `base_revision` + `revision`，同 `epoch` 且 `base_revision` 等于客户端已知水位才连续，
否则即为缺口。Item delta 还要求命中当前未终态 Item 且 revision 严格递增。任何缺口、未知变体
或 `lagged` 一律重新订阅并从数据库窗口重建，不拼接空洞、不向 owner 索取已释放的历史 effect。
普通滚动分页只查询数据库，不读取 owner、不 flush writer、不触发恢复。

## 17.5 Snapshot scheduler 与保存诊断

每个已加载 Thread 最多一个 snapshot scheduler。它维护正在写的一份和最新待写的一份，使用
跳过错过 tick 的一秒 interval；序列化与文件 IO 在 owner 临界区外完成。scheduler 先等待
checkpoint 的 history/blob fence，再原子发布 TOML。

公共持久化状态至少暴露：

- state dirty/saving/durable revision；
- history/calls admitted/durable write sequence；
- pending operations/bytes、oldest pending age、in-flight bytes；
- 最近类型化错误和是否因压力暂停准入。

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

## 17.7 迁移实现状态与剩余边界

本节区分已落地的迁移行为与仍存在的边界；文中未列出的能力不得据本文假定已实现或已验收。

已实现（当前代码路径）：

- 数据迁移只由一次性协调器执行，运行在 `~/.anywork/studio/runtime.lock` 独占运行锁内，相位
  与源指纹写入 `~/.anywork/migrations/session-migration.json`；来源只能是共享会话库
  （`studio/sessions.sqlite`）、旧逐产品调用库（`studio/calls.sqlite`）、旧全局附件根
  （`studio/attachments/`）或旧产品 schema，正常启动不读取退役表。
- 相位机为 `Detected → BackedUp → Upgraded → Exported → Verified → Published`，逐相位落盘，
  崩溃或取消后按记录相位继续，已验证步骤按稳定身份不重复累加。
- 产品库 schema 19–21 的正常打开明确失败（要求迁移），不能隐式原地升级：产品表结构升级
  只在协调器内、独占锁与备份之后原位执行，v20 经 `20 → 21 → 22` 两步在同一事务提交，v21
  只执行 `21 → 22`；步骤幂等可重启，仅在自身列改动与载荷回填成功后才推进 `user_version`。
  仅有产品库（无会话 journal）的 home 走同一显式入口，操作者命令为
  `pl-studio-server [--studio-home <绝对路径>] migrate-legacy-storage --confirm <绝对路径>`
  （`--confirm` 必须等于解析出的 Studio home，且要求存在原始产品库）；正常启动只在存在旧
  产品 schema / 旧布局信号时转换，否则保留字节并失败关闭。
- 退役 `studio/calls.sqlite` 按 §17.8 的“退役调用库导入与发布”规则导入、重验与归档。
- 布局切换有独立的 durable 公告 `~/.anywork/migrations/layout-publication.json`：它在任何
  canonical 文档或 layout root 被触碰前写入，在报告落盘 `Published` 后删除并 fsync 其目录；
  这条删除是整次切换的提交点。公告存在期间没有读取者可以归类/读取 canonical 布局或自建
  canonical root（例如空 `calls/calls.sqlite`），一律 fail closed；崩溃留下的公告由下一次
  持锁启动续跑相位机后退役。
- 无 journal 的空壳 Thread 被拒绝：若退役产品目录仍有 Thread 目录行，而 staged 结果里没有
  可恢复的会话 journal/checkpoint，迁移明确失败并保留全部原字节，绝不发布一个打不开的 Thread
  或重建空会话。
- 已发布当前布局的 home 在正常启动时不重建 canonical 文档：`catalog.toml`、`settings.toml`、
  `workspaces.toml` 任一缺失即失败关闭，原字节保留；只有全新 home 才创建空文档。
- 配置启动迁移当前处理 schema 18 与 19 → 20：备份原文件后完成路由与 disabled agent 的
  变换，保留 provider 身份与凭据，失败保留原文件。
- 正常启动中的一次性存储迁移若遇到确定不可继续的版本、源事实或迁移状态错误，在独占运行锁内
  先将整个旧 Studio home 归档到同卷独立目录，保留原配置、会话、附件、报告和已有备份，再按
  全新 home 的合同初始化。GUI 和 HTTP server 均适用；显式迁移命令只报告失败，不触发兜底。
  归档有可续跑的持久进度，只有原文件全部转移并核对后才可启用默认配置与空目录；归档或初始
  化中断时续跑，冲突时失败关闭。文件占用、权限、空间不足及无法分类的 I/O 错误不触发兜底。
  归档永久保留且不自动导回；完成兜底的这次启动由 GUI 与 server 恢复状态提示归档位置和脱敏
  原因，GUI 可关闭提示。成功初始化后删除进度标记，不保留额外恢复记录，后续启动不再将已完成
  的归档当作待处理问题；旧版本留下的通知文件在下次成功启动时清理。系统凭据库不删除，旧配置
  中的凭据引用随归档保留，新配置不隐式复用旧项目或会话。

剩余边界（不要据此宣称完整迁移能力）：

- 会话版本迁移窗口硬编码为 `sessions.sqlite` schema 6 与 7；产品库迁移窗口为 schema 20–22。
  超出窗口的版本明确失败并保留原数据，没有通用迁移路径或“逐版本补齐”。注意产品库 schema 19
  的正常打开会报“需要迁移”，但协调器只接受 20–22，因此 19 目前**无可用转换路径**（保留字节
  并报错）；这是已知缺口，不是已支持能力。
- 配置只有 18、19 有版本化转换路径；启动期对未知/未来版本、不可解析、当前 schema 校验失败
  或含内联凭据的配置一律 fail closed，保留原字节与 provider 凭据关联并报错，不再有“备份后
  替换默认”的降级路径；仅当 `config.toml` 路径条目真正缺失时才采用内存默认配置，权限、
  元数据或符号链接异常同样 fail closed，且解析失败诊断只报路径、类别与行/列位置，不回显
  原文或凭据。通用版本化转换仍不完整（配置契约见 [20](./20-config.md)）。
- 损坏 WAL、未知 payload 等异常组合未被穷尽；已知异常组合 fail closed，未知组合未验证。
- 迁移测试覆盖“旧会话 journal + 已归档的早期 calls/附件”等磁盘态的确定性恢复，但**没有**对
  发布中的真实 SIGKILL 做故障注入；生产切换的原子性由上述公告 + 相位续跑提供，不是单次
  rename 的原子性。

## 17.8 旧存储迁移

从 `studio.sqlite` + `sessions.sqlite` 迁移到 TOML + 每会话历史库 + 全局调用库，在
`~/.anywork/studio/runtime.lock` 独占运行锁内完成。旧事实有两个调用来源：共享会话 journal
中随 Turn 产生的调用记录，以及旧逐产品 `studio/calls.sqlite`；两者都并入 canonical 全局
`~/.anywork/calls/calls.sqlite`。迁移前：

1. 识别产品、会话、配置和 payload 版本，确认完整升级路径；
2. 对 SQLite 执行完整性检查和 WAL checkpoint；
3. 创建不覆盖既有文件的一致性备份；
4. 写入持久化迁移状态和源文件指纹。

逐 Thread 转换：读取旧不可变 journal → 使用旧 decoder 纯重放 → 通过当前 projector 生成所有
历史 items/turns 与调用记录 → 写临时 history/calls 目标 → 验证数量、身份、顺序、引用、hash 和
终态 → 写临时 checkpoint。全部 Thread、目录、配置、凭据关联、附件和 lease 验证成功后进入
发布：写入布局公告，重写三份 canonical 文档，对 `sessions/` 与 `calls/` 两个 layout root
各做一次目录 rename（改用先写公告、再切换、最后删公告提交的多根协议，而非单次 rename），
再归档退役源文件，最后落盘 `Published` 并删除公告。已存在的目标 root 视为此前发布已到达该步，
重跑不重复 rename；崩溃后由公告与相位报告续跑，不会把半切换的安装当成现役安装。

迁移幂等且可恢复：目标记录以稳定身份写入，已验证步骤不会重复累加；崩溃或取消后按迁移状态
继续或恢复到切换前。旧 journal decoder、schema 和 projector 仅编译进迁移模块；切换后正常
runtime、query、activation 和 subscription 不可引用它们。

迁移必须保留：Project/Thread/parent 身份、目录字段、完整历史正文与原始未知 payload、顺序与
revision、调用 binding/usage、附件/blob、provider/credential 关联、workflow 扩展和 worktree
ownership。未知未来版本、损坏 WAL、缺失 decoder、引用不一致或任一写入失败都保留原数据并
明确失败，不能清空、回默认值或只留下备份。

### 退役 `studio/calls.sqlite` 的导入与发布

旧逐产品调用库 `~/.anywork/studio/calls.sqlite`（与旧产品库同级）是独立于会话 journal 的调用
事实源，必须并入 canonical `~/.anywork/calls/calls.sqlite`，不能在迁移中丢弃或只保留备份：

- **只读快照**：导入只读取 phase-1 备份出的字节副本，从不打开或改写现场退役库；其正文
  通过与会话调用相同的加法式、数据保全 schema upgrader 归属 blob。
- **字节身份绑定**：源库的存在性、schema 版本、database id、行数与只读聚合指纹在导入前记录。
  指纹是主库文件、`-wal` 边车与 `blobs` 根下每个内容寻址 blob 的相对路径和完整字节的流式摘要；
  `-shm` 是 SQLite 打开时自建的瞬时索引，**不进入指纹**（仅在发布阶段随其他退役成员一起归档）。
  导入与发布前都要求实际现场或归档副本仍与记录指纹一致。
- **分阶段重验**：phase-4 从保留的 phase-1 快照重新推导源与目标审计——在“实时源 + 字节归档”并
  集上复算源指纹要求其与记录一致，并从快照重新核对目标覆盖源事实；目标首次验证后记录字节身份。
  发布边界只复算并比对已记录的源/归档指纹与目标字节身份，不重新推导整份源到目标的逐行审计；
  仅当 `verified` 为真、且两处身份比对都通过时，才允许 rename 或归档。
- **失败关闭**：源库存在却无法识别、正文缺失/损坏、目标冲突、指纹或存在性变化、发布后目标
  被改动、源与归档同时出现同一成员等都 fail closed，保留全部原始字节并把失败写入迁移报告。
- **幂等发布**：恢复到 `Verified` 相位时以实际 live 目标（canonical 或 staging）为准重验，
  已 rename 的目标直接跳过；未验证的源不会被发布，也不会作为“已核实”归档。

只有导入并验证成功后，退役的 `studio/calls.sqlite`（含 `-wal`/`-shm` 边车及其 blobs）才与其
他退役源一起移入 `~/.anywork/migrations/session-archive/`；canonical `~/.anywork/calls/calls.sqlite`
原地保留。

## 17.9 数据库和发布验收

新 history/calls 数据库启用 WAL、FULL、foreign keys 与 busy timeout。打包时记录并验收实际
链接的 SQLite 版本；不能只依据 ORM crate 版本宣称具备某个 WAL 修复。迁移和发布测试至少覆盖：

- 相邻及跨版本、非空历史、未知 payload、附件与 lease；
- WAL 中仍有已提交数据、重复启动和中途失败恢复；
- checkpoint 不越过 history/blob fence；
- writer busy/失败/确认不明时不丢批次；
- 大历史启动不扫描会话，分页成本不随历史前缀线性增长；
- 冷历史查询不激活 owner、不执行模型/工具、不修改文件。

## 17.10 诊断与脱敏

错误包含 operation、Thread/Turn/Interaction/call identity 和脱敏 correlation ID，不记录 token、
Mode Prompt 正文、用户 credential 或未授权请求正文。失败 artifact 可包含 migration 状态、schema、
数据库 quick check、写入水位、队列压力、GUI/Driver 日志和文件 diff；敏感原始备份受权限保护。

Timeline 的工具 item 在开始时分配稳定身份；provider identity 后到只更新同一 item。历史缺口、
重复 ordinal、同 revision 内容冲突、checkpoint fence 越界或 blob hash 不匹配必须失败关闭，不能
用当前工作区内容或当前工具渲染器补回。
