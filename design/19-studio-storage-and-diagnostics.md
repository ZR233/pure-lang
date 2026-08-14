# 19 - Studio 单库存储与诊断

## 19.1 数据库

Studio 默认只使用 `~/.pure/studio/studio.sqlite`，schema v4；测试和隔离验收可通过绝对路径
`PURE_STUDIO_HOME` 改写整个 Studio 数据根。数据库启用 WAL、foreign keys、五秒
busy timeout 和 synchronous=FULL；应用数据库连接池固定一个连接，mutation 通过 SQLite
单 writer 事务串行化，snapshot、分页和设置查询共用该连接。

所有 `read*` 查询严格无 mutation；测试注入 SQLite mutation counter 并验证重复 read 为零。
Provider Usage 与 Updater 的 last-known cache 复用现有 `app_settings`，键分别为
`observed:providerUsage:v1` 和 `observed:studioUpdate:v1`，不新增 migration。内存 owner snapshot
不是第二套 durable projection；进程重启后仅从 canonical tables/config 和这两个明确的 observed
cache 重建。

核心表：

- projects
- threads
- thread_inputs
- turns
- items
- interactions
- attachments
- app_settings
- task_runs、task_failures、work_units、work_completions、review_rounds、merge_records、branch_leases
- thread_context_segments、thread_session_state

不存在 history 数据库、storage generation pair、history_gc_jobs、session snapshot JSON、agent
runtime snapshot、agent outcome 或 durable event journal。

## 19.2 提交

ThreadActor 向 ThreadRepository 提交窄 `ThreadMutation`。每次 mutation 在一个 SQLite 事务中
校验 thread revision、更新涉及的 canonical 行并递增 revision；事务失败不更新 actor、不广播。

Item start 分配不可变 ordinal；delta 不写库；terminal 更新同一 Item 完整 payload。Turn terminal、
shutdown 和 Interaction resolution 都等待事务完成。历史查询按 `(thread_id, turn_sequence)`
keyset 分页，不使用 OFFSET。

模型 transcript 与 Studio Timeline 分开持久化。`thread_context_segments` 按 Thread revision 保存
`append | replace`：普通 checkpoint 只追加新增 `ModelContextItem` suffix；compaction、回滚或截断
在同一事务删除旧 segment 并写入新的 replacement baseline。恢复时按 revision fold segment，
拒绝断层、非法首段或损坏 payload。

runtime 从每次 Thread transition 的提交前后 session 自动派生 transcript mutation；TurnFinished、
rollover 和 child 注册不得以 `context=None` 丢弃已变化 transcript。Replace、session snapshot、
Turn terminal 与 mailbox 状态在同一 SQLite 事务中提交，失败时整体回滚且 actor 不更新内存状态。

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
Task/WorkUnit owner、usage 和全部 Git/工作区状态。

`task_failures` 以 `(task_run_id, source_turn_id)` 唯一保存来源 Thread/Turn/agent、WorkUnit 或
ReviewRound、完整 `TurnFailure`、Task disposition 与 resolved 状态；`task_runs.terminal_failure_id`
固定首个 fatal failure。Recoverable failure 只在同一来源 Thread 成功开启后续 Turn 时解决，fatal
failure 永不被迟到 child 事件覆盖。fatal terminalization 使用 SQLite immediate 事务串行化首胜、
Task/children/lease 更新，数据库提交后才关闭进程内 agent。

每次模型 inference 的 usage、provider/model、价格快照和费用明细保存在对应 Turn 的
`model_json`；`usage_json` 保存同一事务重算的 Turn 聚合。完整 usage 必须先持久化成功，再发布
runtime usage 通知。相同 inference ID 的相同记录幂等，内容冲突拒绝事务；历史费用始终使用
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
fingerprint。版本、结构或完整性不兼容时不迁移、不归档、不导入：

1. 关闭本次检查创建的全部数据库连接。
2. 再次证明目标是配置解析得到的精确 canonical Studio 数据库文件。
3. 精确删除 `studio.sqlite`、`studio.sqlite-wal` 与 `studio.sqlite-shm`；不使用 glob，不删除目录。
4. 创建空 schema v4 并完成 fingerprint 校验后才向 Runtime 提供 store。

删除或重建失败属于应用级致命错误，由错误页重试；不得在半初始化数据库上继续。重建只处理
Studio 数据库文件，不扫描、删除或修改 Project、worktree、branch、attachments 或其他 legacy
数据库。旧会话、Task 与 ownership 元数据直接丢弃；因此失去 owner 的磁盘 worktree 也不能被
自动 GC。

## 19.4 归档与附件

归档 project/thread 是可恢复的逻辑操作：在一个数据库事务中标记 Project 关闭和 Thread
已归档，保留 Turn、Item、Interaction、attachment row 与附件文件。有活动 Task 时拒绝
归档 Project。Task worktree 只能走 preview-confirm-revalidate 产品清理，普通归档不删除
库外文件、worktree 或 branch。

## 19.5 诊断

Rust 使用 tracing，只记录稳定 ID、kind、数量、字节数、事务耗时、通道积压、lag 和 outcome，
不记录完整 prompt、模型上下文、secret 或工具结果。数据库 mutation、恢复、订阅和 Task 操作
都携带 correlation ID。日志保留与 crash 文件策略沿用 Studio 现有实现。

缓存诊断只记录 provider/wire/model、prompt generation、有效缓存策略、前缀变化原因、各固定层
hash、token 分类、费用与缓存节省。cache key、prompt、配置正文、header、凭据、工具参数和结果
均不得进入日志。Bridge 只暴露 generation、策略、变化原因与聚合 usage；内部 hash、逐 inference
billing 和 working-context 内容不进入 Flutter timeline。

诊断额外汇总 Driver reconnect、provider retry/fallback、conversation recovery mode/目标 Turn、
transcript 前后 hash、恢复次数与失败原因，但不记录 prompt 或被移除正文。验收 manifest 固定 prompt
hash、workspace/Git identity、runId、Task generation、初始时间、全局 deadline、恢复次数和每次
attempt 日志目录，用于证明原始 prompt 只提交一次以及 recovery 前后 Task/WorkUnit/worktree identity
与 Git fingerprint 保持不变。

工具编排诊断仅暴露与 inference/Turn 关联的聚合数值和稳定 enum；schema、工具参数、工具结果、
program 正文、caller 原始 JSON 与 cache key 均不进入日志或 Flutter timeline。compaction 指标只保存
替换前后 token 估算，不能保存被移除正文。
