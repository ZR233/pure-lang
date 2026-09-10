# 19 - Studio 存储与诊断

## 19.1 数据库

Studio 产品数据使用 `~/.pure/studio/studio.sqlite`，会话使用同目录的 `sessions.sqlite`。
拆库升级清空旧会话但保留项目与配置，不允许通过整库重建清理会话。
统一条目契约见 [25-session-entry-storage.md](./25-session-entry-storage.md)。
启动前取得 Studio home 的跨进程独占 lock；数据库使用 WAL、foreign
keys、busy timeout 与串行 write-behind transaction。

产品库保存项目、Thread 目录、设置、观测缓存与版本化 Studio object；通用 Thread journal、
输入、交互、工具交付、扩展及资源元数据保存在独立会话库，Item 由日志投影。
Workflow 是 Studio 编码的 `studio.workflow` Thread 扩展，不新增 workflow 阶段/边/转换业务表。旧 TaskRun、
WorkUnit、ReviewRound、MergeRecord、旧 worktree registration 与 Task recovery 表全部删除。新的 worktree
lease 复用 `studio_objects`，不新增 Task 表；lease 保存
`prepared | active | preserved | cleanupRequested | cleaned`、repo/path/branch/base 和 revision，仅表达
物理资源 ownership，不恢复旧任务业务模型。

## 19.2 checkpoint

活动 Thread owner 是唯一事实源。write-behind queue 接收已冻结编码的不可变 commit；worker 只执行外层完整性校验和 SQLite transaction。workflow tool-call、tool result 与 working state 同批提交，失败共同回滚。
上述回滚仅限数据库事务，不回滚已提交内存。后台写入独立重试并完整保留未保存事实；待保存量达到 Thread/store 阈值时暂停新的执行受理，既有结果继续保存。内存 revision 与 durable revision 独立，持久化状态仅用于诊断和释放判断。
完整 workflow state 最大 256 KiB；图 hash 与尾部历史在进入 存储前已由 Studio 验证。完整图与
Mode Prompt 只存在于当前内存注册快照，不写入 repository。

查询不产生 mutation。read snapshot、timeline keyset page 与 observed state 必须可重复且不触发扫描、
修复、默认 Thread 创建或工具执行。

生命周期遵循同一内存契约：worktree lease 的唯一 owner 驻留于进程，创建、激活、保留、
清理与补偿只消费 owner 状态，lease 事实进入同一 write-behind 队列。Git/文件系统失败影响
操作结果；数据库失败只影响保存诊断，不能回滚已提交内存或已完成物理清理。
启动与显式历史恢复允许读取冷数据；恢复后的运行不回退到 SQLite 判断 ownership。
冷目录分页与历史查询不能作为运行完成判据。尚未清理的 lease 与未保存事实不得淘汰。
崩溃可能留下未保存 lease 的物理资源；启动核对受管目录与 Git 注册，对不明资源保留并报告。

## 19.3 文件配置

主配置位于 `~/.pure/config.toml`，保存 provider、模型 route 和 `disabled_system_agents`。用户 Agent
Profile 位于 `~/.pure/agents/*.toml`，一个文件一个稳定 Agent ID；runtime 原子保存单文件并单独报告
解析诊断。系统 Profile 不写 TOML。

Thread Mode 由内存注册表提供，不复制到数据库或用户目录。run 只保存 `ThreadModeId` 与图 hash；
Prompt-only 更新从下一 Turn 生效，图变化则在 provider 前归档并替换 active run。

## 19.4 恢复与诊断

启动恢复处理进程 lease、Agent session snapshot、不可用项目路径和 durable worktree lease。worktree
部分缺失或身份不匹配时保留现场并发布带 revision、branch/base/head、dirty/changed-files 的 Recovery
preview；显式 cleanup 才能删除。恢复 DTO 不携带 Task run、merge 或自动整合状态。日志错误包含 operation、Thread/Turn/Interaction identity 与
redacted correlation id，不记录 provider token、Mode Prompt 正文或用户 Profile credential。

Live artifact 对 wire capture、配置和日志执行 credential redaction；wire capture 可携带不进入
provider wire 的 `sessionId`、`turnId` 与 `inferenceId` trace identity，使验收能按 canonical
session 聚合跨 inference 调用，而不依赖 Profile 提示词片段或工具调用形态猜测 actor。失败
artifact 保留 workflow snapshot、GUI/Driver 日志、截图、文件 diff、验证输出和最后进程树。

工具流式 trace 已分配的 item identity 在执行与输出期间保持不变。provider item ID 后到时只补充
provider identity，不重命名 canonical trace item；后续命令输出必须发布到已存在的 canonical item。

上下文恢复验证每个条目的内容摘要、信封与索引一致性、连续 ordinal，以及同一 checkpoint 的
transcript manifest 总数；中间缺口和尾部丢失都必须失败关闭。追加只写入新增条目，整体替换与新
manifest 在同一事务提交，不反复编码完整历史前缀。

后台 writer 通过 revision receipt 识别已保存的批次前缀；事务失败或确认结果不明时保留不可变
待保存事实并幂等重试。后台编码结果与 SQLite 查询结果不作为活动 actor 的第二份热状态。

通用协作 ownership 约束 LRU：未关闭的 child 与仍拥有未关闭 child 的 parent 保持驻留，
不依赖任务模式、角色名或返工计数。关闭后可在全部事实耐久且无其他活动引用时转为冷历史。
