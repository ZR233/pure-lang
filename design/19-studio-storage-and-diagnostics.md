# 19 - Studio 单库存储与诊断

## 19.1 数据库

Studio 默认只使用 `~/.pure/studio/studio.sqlite`，schema v2；测试和隔离验收可通过绝对路径
`PURE_STUDIO_HOME` 改写整个 Studio 数据根。数据库启用 WAL、foreign keys、五秒
busy timeout 和 synchronous=FULL；应用数据库连接池固定一个连接，mutation 通过 SQLite
单 writer 事务串行化，snapshot、分页和设置查询共用该连接。

核心表：

- projects
- threads
- thread_inputs
- turns
- items
- interactions
- attachments
- app_settings
- task_runs、work_units、work_completions、review_rounds、merge_records、branch_leases

不存在 history 数据库、storage generation pair、history_gc_jobs、session snapshot JSON、agent
runtime snapshot、agent outcome 或 durable event journal。

## 19.2 提交

ThreadActor 向 ThreadRepository 提交窄 `ThreadMutation`。每次 mutation 在一个 SQLite 事务中
校验 thread revision、更新涉及的 canonical 行并递增 revision；事务失败不更新 actor、不广播。

Item start 分配不可变 ordinal；delta 不写库；terminal 更新同一 Item 完整 payload。Turn terminal、
shutdown 和 Interaction resolution 都等待事务完成。历史查询按 `(thread_id, turn_sequence)`
keyset 分页，不使用 OFFSET。

模型上下文直接从有序 Item 重建；contextPatch 保存一次采样前模型可见的运行上下文差量，
contextCompaction 重置基线。两者都属于内部 Item，Bridge 查询永不返回。contextPatch 只用于
模型输入重建和审计，不能代替 ThreadRuntimeSnapshot 中的当前事实。

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

## 19.3 不兼容库重建

启动先只读检查 canonical `studio.sqlite` 的 `user_version`、`quick_check` 与必需表/列
fingerprint。版本、结构或完整性不兼容时不迁移、不归档、不导入：

1. 关闭本次检查创建的全部数据库连接。
2. 再次证明目标是配置解析得到的精确 canonical Studio 数据库文件。
3. 精确删除 `studio.sqlite`、`studio.sqlite-wal` 与 `studio.sqlite-shm`；不使用 glob，不删除目录。
4. 创建空 schema v2 并完成 fingerprint 校验后才向 Runtime 提供 store。

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
billing 和 contextPatch 不进入 Flutter timeline。
