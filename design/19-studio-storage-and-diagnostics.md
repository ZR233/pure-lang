# 19 - Studio 双库存储、会话历史与诊断

本文定义 Pure Studio 当前唯一的本地持久化架构。实现使用 SeaORM 2.0 的
Entity-first schema，不保留手写建库 SQL、旧 SQLite 运行期兼容层或跨库原子事务假象。

## 19.1 数据库边界

Studio 在 `~/.pure/studio` 使用两个 SQLite 文件：

- `studio_state.sqlite`，schema v11：项目、会话、Task、work unit、review、merge、branch
  lease、interaction、attachment、agent latest state、input queue、turn 与 UI projection。
- `studio_history.sqlite`，schema v1：完整的 append-only 会话历史、turn 索引和模型上下文
  checkpoint。

两库分别拥有 SeaORM `DatabaseConnection`，均启用 WAL、foreign keys 和五秒 busy timeout。
状态库使用 `synchronous=NORMAL`，历史库使用 `synchronous=FULL`。连接池固定为
`min_connections=1`、`max_connections=4`；历史库只允许一个顺序 writer，其他连接只服务
恢复和分页读取。SQLite 选项必须在创建连接时映射到每个 SQLx SQLite connection，不能只在
池中任意一个连接上执行一次 PRAGMA。

每个库都保存 `database_kind`、`schema_version` 与相同的 `storage_generation_id`。启动只接受
类型、版本和 generation 全部匹配的数据库对；单库缺失、版本过高、generation 不一致或损坏时
保留现场并把 typed storage failure 返回 Studio，禁止静默拼接或单独重建。

Entity 是 schema 的唯一事实源。空库使用 SeaORM `SchemaBuilder::apply` 创建表、关系和索引；
生产启动不执行 schema sync、migration chain 或手写 DDL。业务 CRUD、upsert、CAS 和分页使用
typed Entity query；raw SQL 只允许出现在 PRAGMA、`user_version`、完整性检查和 SQLite 无 typed
等价物的边界。

## 19.2 历史分表与查询

历史库按访问模式固定为三张表，不按日期、项目或 session hash 动态创建物理分片：

- `session_history_turns`：主键 `(session_id, turn_sequence)`，唯一键
  `(session_id, turn_id)`；保存 turn 状态、模型、开始/结束时间和终态错误摘要。
- `session_history_items`：主键 `(session_id, sequence)`，唯一键
  `(session_id, item_id)`；保存终态 message、reasoning、tool call/result、状态和完整 typed
  payload；索引 `(session_id, turn_id, sequence)`。
- `session_history_checkpoints`：主键 `(session_id, revision)`；保存
  `through_sequence` 与完整模型上下文；索引 `(session_id, through_sequence DESC)`。

历史永久保留到用户显式归档/删除 owning session 或 project。历史分页使用 keyset：turn 按
`turn_sequence DESC`，turn 内 item 按 `sequence ASC`；默认最近 50 个 turn，服务端把 limit
限制在 `1..=200`。任何分页实现不得使用 OFFSET。

`session_history_items` 是会话语义历史的事实源。状态库中的 turn、timeline snapshot、当前
context 和 runtime sequence 只是可重建 projection/cache。完整回放保存所有终态消息、reasoning、
工具调用与结果、状态及模型上下文；streaming delta 只作为 live overlay，不持久化也不复刻原始
到达节奏。

## 19.3 提交顺序与异步 writer

Agent runtime 向宿主提交 typed `SessionHistoryCommit`。commit 携带 session/turn、单调
sequence/revision、history items、turn transition 与
`SessionContextMutation::{Append,Replace}`。

宿主使用容量 1024 的有界队列。checkpoint 只校验并 `try_send`，不等待 SQLite；writer 最多把
128 个已排队 commit 合并到一个历史库事务。队列满、writer 失败或 revision 冲突必须 fail closed，
取消并 fault owning agent，禁止丢弃或降级为仅内存成功。

跨库一致性固定为以下顺序：

1. history writer 提交 append-only facts 并推进 durable watermark；
2. 对应 projection mutation 才可交给 state writer；
3. state writer 提交可重建 snapshot/checkpoint 并推进 projection watermark；
4. durable event 在 history ack 后广播；completed/failed/cancelled terminal 只有在两个
   watermark 都越过 barrier 后广播。

状态 projection 不得有意领先历史。若进程在步骤 1 与步骤 2 之间退出，启动从历史 suffix
重建状态；若历史写失败，状态不得提交。完成、失败、取消和 runtime shutdown 都必须等待
barrier；transient delta 仍可即时广播。进程内 `SessionEventHub` 最多保留 4096 个 durable item
供短线重连，不能用该上限裁剪磁盘历史。

模型恢复读取最新 Replace checkpoint，再追加 `through_sequence` 后的 context items。Replace
只改变下一次模型请求的上下文基线，不删除旧 UI 历史。

## 19.4 跨库删除

SQLite WAL 下不承诺跨 attached database 崩溃原子性，因此产品事务不使用 `ATTACH` 模拟跨库
提交。状态库删除 session/project 时在同一状态事务写入 `history_gc_jobs`；后台 worker 按
session 幂等删除三个历史表，再删除对应 job。启动继续未完成 job。GC 期间的孤儿历史没有可见
state owner，Bridge 不返回它，但在 GC 成功前保留以便重试。

## 19.5 破坏性升级

legacy `studio_2.sqlite` 只用于识别 v10。首次升级必须在没有活动连接时把主文件、`-wal` 与
`-shm` 一起移入 `archive/storage-v10-{unix_seconds}`，随后创建新的双库；不迁移、不导入旧
数据。归档失败、旧库锁定或新数据库对只创建了一半时停止初始化并保留全部文件。高于支持版本
的库拒绝打开。

## 19.6 Bridge 与 UI 恢复

Bridge 提供 typed `loadSessionHistoryPage`。请求包含 `sessionId`、可选
`beforeTurnSequence` 与 `limit`；响应包含倒序 turn、每个 turn 的顺序 items、
`nextBeforeTurnSequence` 与 `hasMore`。wire 字段使用 camelCase，ID 为 String，时间戳为 Unix
秒 `i64`。

session subscription 仍负责 authoritative current snapshot、live delta 和 durable cursor。
Flutter 在首次选择或重启时先加载最近历史页，再建立无 cursor subscription；历史页只补充旧
rows，不能覆盖更高 sequence 的 current projection。继续向上滚动时按 keyset 加载更旧 turn。

## 19.7 tracing 与保留策略

Rust 统一使用 `tracing`。日志不得复制完整 prompt、模型上下文或工具结果，只记录稳定 ID、类型、
字节数、耗时和 outcome。默认 filter 为 `warn`，优先级为命令行 `--log-level`、`RUST_LOG`、
默认 `warn`；命令行只接受 `error|warn|info|debug|trace`。

WebSocket 尝试、checkpoint、工具指标、MCP stderr 摘要、数据库批次和恢复细节使用 `trace`；
`info` 只保留启动、正常关闭和 schema/归档等必要生命周期；可恢复降级使用 `warn`，终态故障
使用 `error`。

普通日志异步写入 `logs/studio-YYYY-MM-DD.log`。Rust error 同步追加
`logs/error-YYYY-MM-DD.log`，Dart isolate 错误同步追加 `logs/dart-error-YYYY-MM-DD.log`。panic hook 在调用原 hook 前同步写独立 crash 文件、`sync_all`，
再发出 `tracing::error!`。正常关闭显式 flush writer guard。启动及每小时只清理 Pure Studio
自有的 studio/error/dart-error/crash 文件，删除条件为最后修改时间严格早于当前时间 48 小时；会话历史
不受日志保留策略影响。日志目录不可写不能导致 GUI 崩溃，必须回退到 stderr/Windows 调试输出。
