# 17 - Studio 存储与诊断

本文是 Studio 配置文件、目录库与逐 Thread 会话库、write-behind checkpoint、恢复与诊断的唯一权威源；通用会话条目与
core 存储语义见 [15](./15-session-storage.md)，数据切换的安全边界见 [04](./04-security.md)。

## 17.1 数据库

Studio 产品目录使用 `~/.anywork/studio/studio.sqlite`，会话使用
`~/.anywork/studio/sessions/<thread-id>.sqlite`，root 与 child 各自一库。用户配置与模型路由
使用 `config.toml`，工作空间声明使用 `workspaces/<project-id>.toml`（见 [20](./20-config.md)）。
产品库保存 Thread 树目录、项目动态状态、观测摘要与版本化 Studio object；工作空间声明的
数据库列仅为可重建检索索引，不作为第二个可写配置源。通用 Thread journal、输入、交互、
工具交付、扩展及资源元数据保存在对应会话库，Item 由日志投影。
Studio 拥有分库路由与连接生命周期；core 仍只拥有通用单库读写，不解释产品索引。
数据库路径只由校验后的 Thread ID 派生，冷读缺失库必须报错，不能创建空库。
拆库或结构升级遵循 17.4 的迁移契约，不能通过清空会话
完成升级。启动前取得 Studio home 的跨进程独占 lock；数据库使用 WAL、foreign keys、busy
timeout 与串行 write-behind transaction。运行锁文件中的 PID/宿主/启动时间仅用于诊断，
独占权由操作系统文件锁决定；诊断元数据不强制刷盘，数据库的持久化策略不受影响。

Workflow 是 Studio 编码的 `studio.workflow` Thread 扩展，不新增 workflow 阶段/边/转换业务
表。历史结构仅在迁移边界转换为当前事实，不恢复旧任务运行入口。worktree lease 复用
`studio_objects`，不新增任务表；lease 保存
`prepared | active | preserved | cleanupRequested | cleaned`、repo/path/branch/base 和
revision，仅表达物理资源 ownership（生命周期合同见 [12](./12-collaboration.md)）。

lease 载荷还记录归属类型（`session` 会话自身工作区 | `child` 子智能体工作区）与 owner
Thread id，两类归属共用同一状态机、存储与显式清理入口。Thread 目录事实另外保存会话级
`workspace_mode`（`local | worktree`）：它是产品事实，lease 是物理资源 ownership，两者职责
不同；没有 lease 不代表会话回到 `local`，也不允许由 GUI 或目录查询推导。

Thread 目录事实还保存会话级工作区地址 `workspace_path`：`local` 行是 canonical Project
目录，`worktree` 行是该会话工作树路径；它在创建会话时写定、之后只读，子线程继承所属根会话
的地址。地址是投影给 GUI 的会话事实，不是第二份 ownership：`worktree` 行的地址由同一归属
派生（见 [12](./12-collaboration.md) §12.5），物理创建、身份校验与清理仍只消费 durable
lease。

## 17.2 checkpoint 与分页

活动 Thread owner 是唯一事实源。write-behind queue 接收已冻结编码的不可变 commit；worker
只执行外层完整性校验和 SQLite transaction。workflow tool-call、tool result 与 working state
同批提交，失败共同回滚；该回滚仅限数据库事务，不回滚已提交内存。后台写入独立重试并
完整保留未保存事实；待保存量达到 Thread/store 阈值时暂停新的执行受理，既有结果继续保存
（阈值与背压见 [15](./15-session-storage.md)）。内存 revision 与 durable revision 独立，
持久化状态仅用于诊断和释放判断。完整 workflow state 最大 256 KiB；图 hash 与尾部历史在
进入存储前已由 Studio 验证；完整图与 Mode Prompt 只存在于当前内存注册快照，不写入
repository。

查询不产生 mutation：read snapshot、timeline keyset page 与 observed state 必须可重复，
且不触发扫描、修复、默认 Thread 创建或工具执行。

Studio 的 Timeline 条目索引从同一 canonical journal 与提交水位派生，可以重建；不改变
core 日志格式，不建立第二份历史数据库。产品派生索引位于对应会话库，包含稳定 item
身份、ordinal、版本化展示内容、关联 Turn 与有界面板摘要。索引由 commit 增量推进，不能
在每次查询或追加时重放历史前缀；完整投影仅用于迁移或显式重建。
索引数据和索引水位在自己的事务内原子提交，水位不能领先 durable journal；这不是跨
连接或跨库原子事务。索引落后由专属 worker 补齐，查询返回一致页或明确的准备中/失败，
不隐式扫描或重建。目录状态与交互定位使用带水位的摘要/索引，不逐库读取全部 journal。

索引写入前必须确认对应 commit 已在 core journal 耐久，并只消费该 Thread 的有界
journal 分页；core 只读 reader 不 hydrate 资源、不启动 writer，也不作为写路径。
因此打开会话库不会把整条历史载入内存；journal 行不是注册资源，不参与资源查询与
幂等冲突判定，重复受理同一不可变 journal 内容保持既有行与元数据，内容变化才冲突。

冷读取使用 SQLite keyset 查询 latest/before/after/around，默认及上限均为 100 items，
同时受正文预算约束；不使用深 OFFSET，不以整 Turn 限制页大小。游标绑定 Thread、索引
代次、读取水位与排序边界，返回覆盖边界、双向游标及相关 Turn 元数据。无效或过期游标
明确报错并允许重新定位；跨页不重复或跳过条目。热追加使用相同身份和水位，旧页不得
覆盖更新的热条目，订阅不持有自己的完整 journal 副本。
超大条目返回有界预览与版本化内容引用，完整展示内容按 UTF-8 字节边界分段读取并校验
身份、版本及摘要；不得为一个片段解码整个大 commit，不截断丢失原文，也不改变模型
可见上下文。派生展示内容的唯一重建来源仍是 canonical journal。
连接、页缓存与未落盘热窗口受数量和字节预算约束；保留逐 Thread 和全局背压，不能因
拆库无限增加 writer 或连接。闲置连接在排空 writer 后关闭，未耐久事实不能被缓存淘汰。

生命周期遵循同一内存契约：worktree lease 的唯一 owner 驻留于进程，创建、激活、保留、
清理与补偿只消费 owner 状态，lease 事实进入同一 write-behind 队列。Git/文件系统失败影响
操作结果；数据库失败只影响保存诊断，不能回滚已提交内存或已完成物理清理。启动与显式
历史恢复允许读取冷数据；恢复后的运行不回退到 SQLite 判断 ownership。

Thread 冷激活先校验目标及待激活祖先的身份、归属与未归档状态，再将已读取的目录条目
补入内存热集合，最后发布 owner。缓存预热不改变目录 revision、不广播目录变更，且不能
覆盖已有的更新条目。工具刷新与运行状态观察只能看到目录就绪的活动 owner；普通分页
查询不隐式预热，延迟观察也不能重新插入已归档条目。冷目录分页与历史查询不能作为运行
完成判据；尚未清理的 lease 与未保存事实不得淘汰。崩溃可能留下未保存 lease 的物理资源：
启动核对受管目录与 Git 注册，对不明资源保留并报告。

## 17.3 文件配置

主配置位于 `~/.anywork/config.toml`，保存 provider、模型 route 和 `disabled_system_agents`。
用户 Agent Profile 位于 `~/.anywork/agents/*.toml`，一个文件一个稳定 Agent ID；runtime 原子
保存单文件并单独报告解析诊断；系统 Profile 不写 TOML。Thread Mode 由内存注册表提供，
不复制到数据库或用户目录；run 只保存 Mode ID 与图 hash（见 [11](./11-thread-mode.md)）。
配置 schema 的完整契约见 [20](./20-config.md)。

## 17.4 恢复与版本迁移

启动恢复处理进程 lease、Agent session snapshot、不可用项目路径和 durable worktree
lease。worktree 部分缺失或身份不匹配时保留现场并发布带 revision、branch/base/head、
dirty/changed-files 的 Recovery preview；显式 cleanup 才能删除。preview 与显式 cleanup 只
面向需要人工处置的 lease：`preserved`、没有已注册 Thread 的孤儿 lease、归档清理失败而保留
的现场，或身份与物理资源缺失、不匹配的现场；由活动（未归档）Thread 持有的 `active`
lease 不是清理候选，cleanup 命令在服务端复合同一前置条件后才执行删除。运行期收束为
`preserved` 的会话 worktree 与启动审计使用同一发布与清理路径，不要求等到下次启动才可见。
运行期仍处于创建中的 lease 由进程内标记豁免发布与清理；崩溃遗留（标记随进程消失）仍按需要
人工处置处理。任何把会话 worktree 收束为 `preserved` 的运行期路径都要在返回错误前完成发布。
启动恢复检查只读取目录恢复候选摘要和 durable lease，不遍历全部会话 journal；完整历史
审计是显式操作。目标 Thread 执行所需恢复在 activation 前完成，普通历史选择、分页及
订阅不激活 owner。损坏历史按 Thread 报告，不能默认空状态或影响无关会话。

恢复检查在首屏可用后执行，不作为全应用启动屏障。当前 schema 的重复启动不重新执行建表和版本写入；完整性与
版本检查、WAL 和同步持久化保证保持不变。预置技能以构建期内容指纹及文件清单（长度、
修改时间）复用本地资源；派生清单位于资源目录外，原子替换，不作为安全授权依据。
缓存缺失或文件元数据改变时通过 staging 准备后替换，失败保留可重试状态。

以下为 anywork 版本演进必须满足的迁移契约；当前实现尚未全部满足，缺口见 17.6。

- Studio 在持有数据根目录独占运行锁、正常运行尚未发布时协调迁移。先识别 schema 与载荷
  版本、校验完整性并确定到当前版本的完整迁移路径；支持跨版本升级，不要求用户逐版安装。
  调用方取消等待不提前释放迁移锁，操作收束或恢复状态持久化后才允许交接。
- 迁移前保留可恢复的一致性备份，不覆盖已有备份，覆盖相关数据库及其已提交 WAL 数据。库内修改使用事务；
  跨库、文件或凭据关联的切换使用可恢复步骤，未完成前不发布混合版本状态。
- 迁移保留项目、Thread 身份、日志事实与顺序、配置关联、附件内容、资源引用及 worktree
  ownership。可重建投影从迁移后的事实派生，不能以空历史、空数据库或备份归档替代转换。
- 各格式所有者负责历史结构到当前结构的转换；Studio 协调产品关系，core 仅转换通用信封，
  provider/工具 payload 按所属格式处理。旧解码器仅服务迁移，不能成为运行时兼容入口。
- 步骤必须可安全重试；崩溃或取消后依据持久化进度继续或恢复至切换前的一致状态，不重复
  转换已完成数据。提交前校验记录、引用和目标格式，全部成功后才交给当前运行时。
- 未知未来版本、损坏数据、不明 WAL、缺失迁移路径或转换失败均明确失败并保留原数据与
  恢复材料；不能默认为初始状态，也不自动删除用户数据。独立打开存储不隐式执行产品迁移。

v20→v21 是在位产品迁移，在持有数据根独占运行锁的启动协调器内、备份产品库之后完成两件事：
一是为 `threads` 增加 `workspace_mode` 列并把 worktree lease 载荷由版本 1 的 `childId` 转换
为版本 2 的 `ownerKind` + `ownerThreadId`，既有行解释为 `local`，不删除 Thread 行或会话关联
事实；二是把 `ssh_servers` 行迁出产品库成为 `~/.ssh/config` 管理块，`projects.ssh_server_id`
重写为 `ssh_alias` 并删除旧表（契约明细见 [22](./22-ssh-remote.md)）。两步都幂等可重试，
文件写入与别名分配在提交前执行外键与指纹校验；旧 lease 解码器只服务这次迁移，不成为运行时
兼容入口，会话工作区模式不参与会话库升级，也不因 schema 变化被清空。

v21→v22 是同一在位迁移路径：为 `threads` 增加 `workspace_path` 列并回填既有行——`local` 行
取所属 project 的 `path`，`worktree` 行取该 root thread durable lease 的 `path`，lease 缺失
时回退所属 project 的 `path`；不以默认值或清空代替转换，迁移幂等可重试，不删除 Thread 行、
会话关联事实或 lease。

迁移验证覆盖相邻及跨版本升级、拆库、引用与附件保全、重复启动、中途失败和重启恢复，
以及未来版本、损坏输入、缺失转换、备份或提交失败时原数据保持可恢复。配置和凭据关联
的专项契约见 [20](./20-config.md)。

## 17.5 诊断

日志错误包含 operation、Thread/Turn/Interaction identity 与脱敏 correlation id，不记录
provider token、Mode Prompt 正文或用户 Profile credential。Live artifact 对 wire capture、
配置和日志执行凭据脱敏；wire capture 可携带不进入 provider wire 的 sessionId、turnId 与
inferenceId trace identity，使验收能按 canonical session 聚合跨 inference 调用。失败
artifact 保留 workflow snapshot、GUI/Driver 日志、截图、文件 diff、验证输出和最后进程树。

工具流式 trace 已分配的 item identity 在执行与输出期间保持不变：provider item ID 后到时
只补充 provider identity，不重命名 canonical trace item；后续命令输出必须发布到已存在的
canonical item。

上下文恢复验证每个条目的内容摘要、信封与索引一致性、连续 ordinal，以及同一 checkpoint
的 transcript manifest 总数；中间缺口和尾部丢失都必须失败关闭。追加只写入新增条目，整体
替换与新 manifest 在同一事务提交，不反复编码完整历史前缀。后台 writer 通过 revision
receipt 识别已保存的批次前缀；事务失败或确认结果不明时保留不可变待保存事实并幂等重试；
后台编码结果与 SQLite 查询结果不作为活动 actor 的第二份热状态。

通用协作 ownership 约束 LRU：未关闭的 child 与仍拥有未关闭 child 的 parent 保持驻留，
不依赖任务模式、角色名或返工计数；关闭后可在全部事实耐久且无其他活动引用时转为冷
历史。订阅 pin：有活跃订阅的线程不参与 LRU 淘汰。

## 17.6 已实现迁移与剩余边界

逐 Thread 布局升级由启动协调器在独占锁下执行：先完成已支持的旧 schema 转换，再逐
Thread 向 staging 分库复制全部 current/history/head 事实，同时导出工作空间声明。
派生索引不在本次复制中声明就绪：布局标记只声明基础布局并把索引记为待建，索引由后续
worker 按 commit 增量构建，查询在建成前返回准备中。

复制目标库必须整体为空，复制前校验源、复制后校验目标，校验覆盖记录顺序、head 与
history 摘要、current 与重放一致性及索引字段，不接受摘要正确但结构错误、缺尾或
current 不符的库。staging 以持久清单记录目标文件集合、每文件摘要与阶段；恢复时枚举
并拒绝未登记条目、链接或孤儿 sidecar，源内容变化时保留原 staging 与备份并报告冲突，
不清理不明材料。布局标记、清单与备份路径都拒绝链接或 reparse，并同步目标文件与目录
后才发布标记，崩溃在标记之前不暴露半发布状态。

迁移进度及布局版本持久化；校验 Thread 身份、顺序、摘要、附件引用、父子关系
与 lease 后才发布新布局。跨文件步骤必须可恢复，不同时发布新旧布局；旧聚合库只作为
恢复材料保留，不成为正常运行的回退入口。迁移限制内存并发量，不同时装入全部会话。
未知版本、缺失转换、损坏或不明 WAL 明确失败，不覆盖原数据。会话目录读取直接消费
持久目录事实与摘要，不再为每一行重放该会话的 journal。

- 当前会话 schema 6→7 已实现原位事务迁移，取代协调清空流程；保留 Thread 目录、历史正文、
  工具身份、顺序和模型/凭据关联。使用当前用户数据副本及旧版本真实 API 生成的非空历史验证，
  可读取旧交付并续跑同一子代理。具体变换见下节。
- core 独立打开不兼容格式仍拒绝并保留原库；Studio 对尚无转换路径的会话版本明确失败，
  不以备份后清空替代迁移。未穷尽所有损坏、断电和更早版本组合。
- 配置格式不随分库改变；旧版本缺失明确转换路径、未知版本、解析或校验失败必须保留
  原文件并报错，禁止备份后恢复默认值。不存在配置文件才使用内存默认配置；凭据关联不变。

### Turn 协作格式迁移

会话 schema 6→7 在启动独占锁和一致备份后，以单一事务转换 current entries、完整 history
和 head hash，并核对重放一致性；journal v2→v3 合并正常结束原因，旧完成正文与证据保留，
旧 progress/notification 转为不可执行历史载荷。原工具标识、参数和模型绑定是历史事实，不
重命名历史调用。缺失迁移路径或遗留 destructive-reset 标记时明确失败保留材料，不再清空
Thread 目录或重建空会话。产品 schema 22 无表结构变化。
