# 17 - Studio 存储与诊断

本文是 Studio 双库划分、write-behind checkpoint、恢复与诊断的唯一权威源；通用会话条目与
core 存储语义见 [15](./15-session-storage.md)，破坏性重建的安全面见 [04](./04-security.md)。

## 17.1 数据库

Studio 产品数据使用 `~/.anywork/studio/studio.sqlite`，会话使用同目录的 `sessions.sqlite`。
两库不以业务表或双写协议复制 Thread 状态：产品库保存项目、Thread 目录、设置、观测缓存
与版本化 Studio object；通用 Thread journal、输入、交互、工具交付、扩展及资源元数据保存
在独立会话库，Item 由日志投影。拆库升级清空旧会话但保留项目与配置，不允许通过整库重建
清理会话。启动前取得 Studio home 的跨进程独占 lock；数据库使用 WAL、foreign keys、busy
timeout 与串行 write-behind transaction。

Workflow 是 Studio 编码的 `studio.workflow` Thread 扩展，不新增 workflow 阶段/边/转换业务
表；旧任务体系业务表（任务运行、工作单元、评审轮次、合并记录、旧 worktree 注册与任务
恢复）已全部删除，不恢复。worktree lease 复用 `studio_objects`，不新增任务表；lease 保存
`prepared | active | preserved | cleanupRequested | cleaned`、repo/path/branch/base 和
revision，仅表达物理资源 ownership（生命周期合同见 [12](./12-collaboration.md)）。

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
core 日志格式，不建立第二份历史数据库。索引随 Thread 驻留释放，稳定 item 身份用于双向
游标和锚点查询；分页返回覆盖边界、双向游标、水位与相关 Turn 元数据，不以整 Turn 限制
页大小。连续历史阅读分页契约：冷分页与热追加消费同一水位语义，跨页阅读不重复或跳过
条目。

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

## 17.4 恢复与协调重置

启动恢复处理进程 lease、Agent session snapshot、不可用项目路径和 durable worktree
lease。worktree 部分缺失或身份不匹配时保留现场并发布带 revision、branch/base/head、
dirty/changed-files 的 Recovery preview；显式 cleanup 才能删除。启动逐 Thread 恢复审计先
读取纯目录关联，再独立解码各自 journal；单条日志损坏产生该 Thread 的清理提示，不提前
阻断同项目其他日志的恢复收束。

旧格式协调重置仅从持有 Studio 独占运行锁的启动入口进入：先校验两个数据库版本与完整性，
再将数据库完整快照备份到私有目录；可恢复标记依次记录备份完成、旧会话库归档、产品
关联就绪和新会话库就绪。标记存在时禁止产品发布，重启继续同一操作；调用方取消等待不会
释放锁或中断 marker/备份文件操作；任务完成后转交锁给运行时，失败保留恢复标记并释放锁
供下一次启动继续。只清理已知会话表与会话投影，保留项目、配置、SSH 凭据、附件文件、
worktree 与未知产品对象；未来版本、损坏数据库和不明 WAL 均明确失败保留现场。独立打开
产品库或 core 存储不隐式执行此重置。

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
