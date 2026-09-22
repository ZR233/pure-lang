# 15 - 会话状态、历史与调用记录

本文是 Thread 当前状态导出、会话历史写入、调用记录、保存屏障与恢复的唯一权威源；Studio
数据布局与迁移协调见 [17](./17-studio-storage.md)，core 内核契约见
[16](./16-core-contracts.md)。

## 15.1 事实源分离

同一 Thread 的持久化分为三个互不替代的事实源：

| 数据 | 活动事实源 | 持久化 | 读取路径 |
| --- | --- | --- | --- |
| 当前执行状态、当前上下文、待处理输入与交互 | Thread owner | `state.toml` | Thread 激活时完整读取一次 |
| 已提交 Turn、Item 与工具展示记录 | history writer | `history.sqlite` | HistoryReader 直接 SQL 分页 |
| 模型/工具调用、用量、耗时和诊断 | 调用记录器 | 全局 `calls.sqlite` | CallReader 独立分页与统计 |

Thread owner 不保存已持久化历史，不提供历史仓库或历史分页 API。GUI snapshot 不包含完整
Timeline；历史读取不得经过 Thread owner、会话激活或 journal replay。

“运行时不存历史”不表示丢弃模型上下文。下一次请求真正需要的 `ContextSnapshot`、provider
私有 continuation、当前运行事实与未完成状态仍由 owner 持有；用于展示和审计的历史记录具有
独立生命周期。上下文可以压缩、替换或裁剪，已提交历史不因此删除。

允许存在的临时数据只有：当前运行 Item 的完整内存正文与输出组装、尚未受理或尚未持久化的
写入批次、一次 SQL 查询结果和 GUI 可见窗口。未终态正文不周期写入历史；writer 确认固定
写入水位后立即释放对应 effect payload，不把队列变成历史缓存。

## 15.2 Core 状态与输出事实

`pl-core` 的 Thread 类型分为：

- `ThreadSnapshot`：当前内存执行状态，只包含恢复当前逻辑状态及活动观察所需的事实。
- `ThreadCheckpoint`：可序列化的当前状态 DTO，包含 schema、Thread 身份、状态 revision、
  `history_fence`、当前上下文、未完成输入/交互/任务/交付、当前扩展和子 Thread 引用。
- `ThreadEffectBatch`：一次原子状态提交产生的不可变输出事实。它用于实时投影、history/calls
  writer 和产品观察；不是恢复日志，持久化后不由 owner 保留。

checkpoint 保存：

- 当前 Turn 与 attempt 的逻辑状态；
- 当前有效上下文、压缩结果、运行事实和可持久化 model 私有状态；
- 未消费输入、pending interaction/permission、未结束 task 和尚未交付结果；
- 当前扩展、模型 route、workflow 状态、资源引用和子 Thread ID；
- 当前状态 revision 及其依赖的 `history_fence`。

checkpoint 不保存：

- 全部历史 attempt、已结束 Turn、已交付工具结果和已完成交互；
- 每次请求的完整旧输入上下文、所有上下文替换前内容；
- 完整 effect/journal、GUI items、运行句柄、锁、future 或取消令牌。

大型正文与二进制通过稳定 blob 引用保存；不透明 payload 保留 `format`、`version` 与原始 UTF-8
正文，不在 checkpoint 边界解析再编码。

### effect window 与 durable 权威

owner 用一个瞬态 effect window 向活动观察者提供"最近提交但尚未耐久"的 `ThreadEffectBatch`。
窗口只保留尚未被确认 durable 的批次：任一 commit 一旦被固定写入水位确认已落到 history/calls，
其条目立即释放，不再作为第二份历史缓存。窗口不序列化进 `state.toml`，也不自带字节计量——
一次提交只是克隆一个 `Arc`，发布不阻塞在编码完整正文上。因此 `state.toml` 不含历史正文：
已结束 Turn、工具交付正文与已完成交互只存在于 history/calls，checkpoint 只引用其水位。

慢消费者与 GUI 订阅不保证读到全部窗口：consumer 落后于已释放水位时收到显式缺口
（`lagged` / gap），必须从该 Thread 的 history/calls 重新同步，而不是读取过期正文或假设事实
丢失。窗口压力（未耐久字节过多）只暂停新的模型/工具准入；已提交的结果、取消、关闭与交互
收束仍进入保存队列，绝不丢弃已受理的事实。

## 15.3 Effect 发布与背压

owner 在串行提交边界同时完成：

1. 校验候选状态；
2. 分配递增 `state_revision` 与 `write_seq`；
3. 发布新的当前 snapshot；
4. 将同一 `ThreadEffectBatch` 受理到 history/calls writer；
5. 向实时订阅发布增量事件。

状态提交不等待磁盘 IO。writer 的 `admit` 只接受不可变数据并返回固定 `WriteTicket`，不得阻塞
执行器；失败时 owner 保留未受理的最新事实并暂停新的模型/工具执行准入。已经完成的实际结果、
取消、关闭和交互收束仍可进入保存队列，不能为满足内存阈值丢弃已成立事实。

每个会话 history writer 与全局 calls writer 分别报告：排队操作数、排队字节、最老未保存年龄、
正在写入字节、已受理/已持久化水位和最近错误。压力超过阈值时暂停新的有费用执行；低于恢复
阈值后解除。保存失败保留批次与重试入口。

实时事件通道是有界观察通道，不是可靠日志。慢消费者可收到 `Lagged`；重同步从
`history.sqlite` 的稳定 cursor 读取，不向 owner 请求丢失的历史 effect。

### 幂等回执与 by-id 可读条件

owner 的串行提交边界同时产出可持久化的幂等回执，使重复命令
跨窗口、跨重启返回同一结果而不重放执行：

- **输入回执**：`submitPrompt` 的稳定 `inputId` 以最小身份写入 history 身份索引，并与该
  effect 的 items/Turn 行在同一事务提交；重复提交先从该索引返回原回执，不重新受理、不重放
  历史。
- **交互/权限回执**：终态 interaction/permission 事实的 `(item_id, revision, digest, payload)`
  与产生它的 effect 在同一 history 事务写入；重复 resolve 只在回应与变更均相同时返回同一回执，
  冲突载荷明确失败，不重复授予执行权限或重放 effect。
- **消息去重**：消息以稳定 ID 幂等受理；已消费消息身份以有界账本（固定条数）驻留，超窗的重复
  由 history 中已接受的受理事实回答，账本不随历史增长。
- **任务/调用 by-id 权威**：任务状态与工具交付的权威在全局 `calls.sqlite`。按
  `(thread_id, call_id)`（或 `task:{call_id}`）主键可读取任务身份与状态而不加载正文；终态工具
  交付按同一身份读取内容寻址正文。可读条件是事实已 durable 且属于该 Thread：非终态、跨 Thread
  或未知身份返回"无此事实"，调用方必须显式报告，不能当作已完成结果或新工作受理。

在 owner 仍驻留时，重复命令由 effect window 中最新提交直接回答；一旦对应批次被 durable 释放，
同一查询改由上述 history 身份索引 / 终端事实回执与 calls by-id 读取回答，两者语义一致，不因
释放窗口而改写终态或重放副作用。

## 15.4 `state.toml` 保存

每个已加载 Thread 维护 dirty revision。正常情况下每秒捕获一次最新 checkpoint；状态未改变时
不写。单 Thread 最多存在一份正在保存和一份最新待保存 checkpoint，中间 revision 可合并，旧
写入不能覆盖新 revision。Turn 终态、停止、关闭、应用退出、重要 ownership 变化和显式恢复
检查点请求提前保存。

捕获不可变 checkpoint 后立即退出 owner 临界区；TOML 编码、文件同步和原子替换在持久化任务中
完成。统一原子写入流程为：

```text
编码完整对象
→ 写同目录临时文件
→ sync 临时文件
→ 原子替换 state.toml
→ sync 目录
```

`history_fence` 只能指向已由唯一 history writer 固定确认的 effect 水位；未终态正文没有
history 事实，也不能成为 checkpoint fence。发布新 checkpoint 前必须先：

1. `history.flush_through(checkpoint.history_fence)`；
2. 等待 checkpoint 引用的新 blob 已持久化；
3. 原子替换 `state.toml`，并将上一份有效文件保留为 `state.prev.toml`。

因此磁盘上可见的 checkpoint 不会引用尚未保存的历史事实或 blob。owner 不持锁等待保存屏障。

### checkpoint schema 2：外置正文

`state.toml` 的当前 schema 是 `ThreadCheckpoint.schema_version = 2`。当单条正文超过
`CHECKPOINT_BODY_THRESHOLD_BYTES = 64 KiB` 时，它离开 TOML，改以一条
`CheckpointExternalBody`（`slot` + `body` + 版本化引用）记入 `externalBodies`；引用是
`reference_version + digest("sha256:<64 hex>") + byte_len`，物理文件落在
`sessions/<storage-key>/blobs/checkpoint/<前两位十六进制>/<sha256>`。可外置的 slot 覆盖当前
上下文内容与工具调用参数、未消费输入/message、pending interaction/permission、未交付
delivery、扩展与运行事实、以及未结束 attempt 的诊断正文。

发布顺序被强制为：外置 → 写 blob（内容寻址、幂等、不覆盖已存在文件）→ 逐个校验 blob
字节与引用一致并 fsync 文件与目录 → 才原子替换 `state.toml`（并保留 `state.prev.toml`）。
激活时在把 checkpoint 交给任何调用方之前，按引用回读每个 blob 并校验 digest/length；缺失、
损坏或身份不符一律 fail closed，绝不当作空正文或截断正文。schema-1（全内联）文件仍按原样
可读；未知未来 schema 显式拒绝。checkpoint 恢复不依赖 `calls.sqlite`，且不重放旧 journal。

## 15.5 会话历史数据库

每个 Thread 使用独立 `history.sqlite`，面向稳定条目和 keyset 分页，不保存完整执行 journal。
该文件位于应用 home 根下的 `~/.anywork/sessions/<storage-key>/history.sqlite`，`<storage-key>`
是 Thread id 的 SHA-256 十六进制摘要（布局见 [17](./17-studio-storage.md) §17.1）。
最小逻辑 schema：

```sql
CREATE TABLE history_meta (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version    INTEGER NOT NULL,
    database_id       TEXT NOT NULL,
    thread_id         TEXT NOT NULL,
    applied_write_seq INTEGER NOT NULL
);

CREATE TABLE history_items (
    ordinal         INTEGER PRIMARY KEY,
    item_id         TEXT NOT NULL UNIQUE,
    turn_id         TEXT NOT NULL,
    kind            TEXT NOT NULL,
    revision        INTEGER NOT NULL,
    lifecycle       TEXT NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    payload         TEXT NOT NULL CHECK (json_valid(payload)),
    last_write_seq  INTEGER NOT NULL
);

CREATE INDEX history_items_by_turn            ON history_items(turn_id, ordinal);
CREATE INDEX history_items_by_kind_lifecycle  ON history_items(kind, lifecycle, ordinal);
CREATE INDEX history_items_by_kind            ON history_items(kind, ordinal);

CREATE TABLE history_turns (
    turn_id         TEXT PRIMARY KEY,
    first_ordinal   INTEGER NOT NULL,
    last_ordinal    INTEGER NOT NULL,
    revision        INTEGER NOT NULL,
    payload         TEXT NOT NULL CHECK (json_valid(payload)),
    last_write_seq  INTEGER NOT NULL
);

CREATE INDEX history_turns_by_last_ordinal ON history_turns(last_ordinal);

CREATE TABLE history_ordinals (          -- 受理期预留、供 writer 复用的稳定 ordinal
    item_id TEXT PRIMARY KEY,
    ordinal INTEGER NOT NULL
);

CREATE TABLE history_input_identities (  -- submitPrompt 跨重启幂等身份
    item_id        TEXT PRIMARY KEY,
    ordinal        INTEGER NOT NULL,
    revision       INTEGER NOT NULL,
    digest         TEXT NOT NULL,
    request_digest TEXT,
    payload        TEXT NOT NULL CHECK (json_valid(payload)),
    last_write_seq INTEGER NOT NULL
);

CREATE TABLE history_fact_receipts (     -- 终态交互/权限回执（与 effect 同事务）
    item_id        TEXT NOT NULL,
    revision       INTEGER NOT NULL,
    kind           TEXT NOT NULL,
    digest         TEXT NOT NULL,
    payload        TEXT NOT NULL CHECK (json_valid(payload)),
    last_write_seq INTEGER NOT NULL,
    PRIMARY KEY(item_id, revision)
);

CREATE TABLE history_message_identities ( -- 已受理消息最小身份；digest 可空
    message_id     TEXT PRIMARY KEY,
    item_id        TEXT NOT NULL,
    sequence       INTEGER NOT NULL,
    digest         TEXT,
    last_write_seq INTEGER NOT NULL
);
```

`ordinal` 在条目开始时分配，后续完成顺序不改变它；`revision` 是同一条目内容版本，
`last_write_seq` 是保存协调水位。更新必须满足：

- 相同 item/revision/内容幂等；相同 revision 不同内容冲突；
- 旧 revision 不覆盖新 revision；
- item 的 Thread、Turn、ordinal 和 kind 不可改变；
- terminal item 拒绝迟到的草稿或 delta。

payload 是条目自身的 JSON 正文，不含独立 `payload_format`/`payload_version` 列：需要一个无法解释
的原始载荷时以 `kind = 'raw'` 的条目保存，格式与版本随 `ThreadRawPayload` 存在 payload JSON 内
（见 [16](./16-core-contracts.md)）。`history_meta.thread_id`、`database_id` 与
`applied_write_seq` 是游标与水位身份，`history_message_identities.digest` 可空，仅用于迁移回填
留下的“身份已知、正文不可验证”行（读取方 fail-closed）。

流式输出在块开始时分配身份并通过 `history_ordinals` 预留稳定 ordinal；delta 只更新内存组装
与 GUI，不写入 `history_items`，也不因时间间隔保存中间草稿。成功、失败或取消的终态事实随
同一 `ThreadEffectBatch` 受理，由该 Thread 的唯一 history writer 写入同 item 的最终 revision；
输入与消息身份回执仍与该 effect 在同一事务生效。结果不持久化 token 事件日志，checkpoint
必须继续等待 writer fence。

## 15.6 SQL 分页与 cursor

HistoryReader 提供 `Latest`、`Before`、`After`、`Around`（`TimelineQuery`，`item_id` 既接受
canonical item identity 也接受 opaque cursor token），全部使用 `ordinal` 索引和短读事务。
禁止全量读取后切片，也禁止用大 `OFFSET` 或最终时间排序。条目游标是版本化、自校验的
`TimelineCursor`：

```text
version + thread_id + database_id + ordinal + item_id + applied_write_sequence
```

数据库替换、跨 Thread、水位回退或身份不匹配 cursor 明确失败。`TimelinePage` 返回 items、本页
涉及的 turn 摘要、双向 cursor、首尾 item ID、`database_id`、`watermark`（=applied write
sequence）、`truncated` 与 `previews`。查询同时限制条目数、整页序列化字节（2 MiB）和单条预览
（`TIMELINE_ITEM_PREVIEW_BYTES = 256 KiB`）；因字节预算提前结束时，cursor 指向实际返回的最后
一条，不跳过内容。

超大条目以 `TimelineItemPreview` 返回：它对同一 item identity/ordinal/revision 只给预览，附
`total_bytes`/`preview_bytes`/`omitted_bytes`；完整正文另经 `TimelineItemQuery{item_id}` →
`TimelineItemRead{thread_id,database_id,watermark,ordinal,item}` 按身份回读，不激活 owner。

Turn 页同样是有界 keyset：按 `history_turns.last_ordinal` 倒序（`history_turns_by_last_ordinal`
索引、无 OFFSET/排序），受行上限与整页字节预算约束；一条超大 Turn 以连续窗口返回，其
`next_cursor` 也是同一个版本化 `TimelineCursor`（绑定 database/水位/条目），续传停在 Turn 内部
的真实位置。

唯一刻意的例外是按身份的整条回读 `TimelineItemRead`：它只取被点名的那一条正文，为取回超预算
条目而**不设字节上限**，且不缓存进 `HistoryStore`；遍历历史必须走 `page`/`turn_page` 的有界窗口，
不能靠反复 `read_item` 物化整段历史。

`history_turns` 只服务展示，不参与运行恢复。当前 Turn 的权威是 owner；恢复依据是
`state.toml`；历史 Turn 的展示依据是 `history.sqlite`。

## 15.7 调用记录

全局调用库位于应用 home 根下的 `~/.anywork/calls/calls.sqlite`（正文 blob 在
`~/.anywork/calls/blobs/`，见 [17](./17-studio-storage.md) §17.1），按调用身份保存模型/工具
调用的冻结 binding、开始/结束时间、结果类别、用量、价格、请求/响应诊断和关联 Thread/Turn/Item
ID。大正文使用内容寻址 blob 引用。另有一张 `call_watermarks(thread_id, admitted_write_seq,
durable_write_seq)` 记录每个 Thread 的调用队列水位，供 `flush_through` 固定目标与续跑对齐。
调用记录不参与 Thread 恢复或 Timeline 排序；Thread 关闭无需等待无关 Thread 的调用队列，只等待
自己的固定 ticket。

同一调用身份重试写入幂等，冲突明确失败；未结束调用可以更新为终态，但终态不可被较旧观察
覆盖。计费和性能统计从调用库或其明确产品投影读取，不扫描会话历史。
effect 窗口缺口恢复计费时先等待已受理写入的固定 ticket，再只查询缺少 `billing_ref` 的
调用事实；已有计费正文不可从摘要列重构后再次投递，否则同身份的有损正文会触发冲突。

## 15.8 SQLite 与关闭

SQLite 默认启用：

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
```

单库单逻辑 writer 批量执行短事务；分页不跨用户滚动持有事务。`flush_through(ticket)` 只等待
调用时固定目标，不等待整个系统空闲。

Thread 关闭先封闭准入并收束当前工作，再保存实际终态 effect；history/calls 达到固定水位后
保存最终 checkpoint。全部成功才释放 owner。保存失败保持 `Closing`、owner、未保存事实和重试
入口。会话数据库连接与 writer 按需创建，空闲且无订阅、无执行、无待保存数据时释放。

## 15.9 恢复与格式演进

激活 Thread 时只读取并验证当前 `state.toml`；无效时可验证 `state.prev.toml` 并发布显式恢复
诊断。恢复将遗留 Running 状态收束为 Interrupted，不重建模型、工具、外部进程或取消令牌，
不执行历史副作用。需要继续执行时由 Studio 装配当前服务实例。

旧 `ThreadCommit` journal decoder、完整 replay 和共享 `sessions.sqlite` 只存在于版本迁移模块。
迁移一次性：验证旧日志 → 纯重放最终状态 → 投影全部历史与调用记录 → 写入新数据库 → 等待
固定水位 → 在布局公告（`layout-publication.json`）划定的边界内发布 checkpoint 与各 layout
root（不是单次 rename 的原子切换，见 [17](./17-studio-storage.md) §17.8）。正常启动、查询、
激活和订阅不能调用旧 replay。

未知未来版本、损坏数据、缺失迁移路径和未知必需 producer 格式均失败并保留原始字节；未知但
仅影响历史展示的载荷以 raw 历史条目保存。迁移不能用清空、默认状态或只有备份没有转换来替代。
