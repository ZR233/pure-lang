# 19 - Studio 单库存储与诊断

## 19.1 数据库

Studio 只使用 `~/.pure/studio/studio.sqlite`，schema v1。数据库启用 WAL、foreign keys、五秒
busy timeout 和 synchronous=FULL；连接池最多四个连接，mutation 通过 SQLite 单 writer
事务串行化，snapshot、分页和设置查询共用同一连接池。

核心表：

- projects
- threads
- thread_inputs
- turns
- items
- interactions
- attachments
- app_settings
- task_runs、work_units、deliveries、review_rounds、merge_records、branch_leases

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

## 19.3 归档重建

首次启动新 schema 时，若存在旧 `studio_state.sqlite`、`studio_history.sqlite`、`studio_2.sqlite`
或 attachments：

1. 确认没有活动数据库连接。
2. 只读提取项目、Task、Pure worktree、branch、dirty/ahead 等可得资源信息，写
   `manifest.json`。
3. 把三个旧数据库及 `-wal/-shm`、attachments 与 manifest 一起移入
   `archive/thread-schema-v1-{unixSeconds}`。
4. 全部移动成功后创建新的 `studio.sqlite`；任一步失败则逆序回滚移动并停止启动。

不导入旧会话或 Task，不自动删除 manifest 中的 worktree/branch。未来 schema、损坏或锁定的
新库拒绝打开并保留现场。

## 19.4 归档与附件

归档 project/thread 是可恢复的逻辑操作：在一个数据库事务中标记 Project 关闭和 Thread
已归档，保留 Turn、Item、Interaction、attachment row 与附件文件。有活动 Task 时拒绝
归档 Project。Task worktree 只能走 preview-confirm-revalidate 产品清理，普通归档不删除
库外文件、worktree 或 branch。

## 19.5 诊断

Rust 使用 tracing，只记录稳定 ID、kind、数量、字节数、事务耗时、通道积压、lag 和 outcome，
不记录完整 prompt、模型上下文、secret 或工具结果。数据库 mutation、恢复、订阅和 Task 操作
都携带 correlation ID。日志保留与 crash 文件策略沿用 Studio 现有实现。
