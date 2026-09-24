//! Per-Thread durable timeline storage with keyset pagination.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use pl_core::chat::{ChatError, ChatHistory, ChatItem, ChatQuery, HistoryPage};
use pl_protocol::thread::TimelineCursor;
use pl_protocol::thread::{TIMELINE_ITEM_PREVIEW_BYTES, preview_timeline_item};
use pl_protocol::{
    ThreadItem, ThreadItemKind, ThreadItemState, TimelinePage, TimelineQuery, TimelineTurn,
};
use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection,
    SqliteTransactionMode, Statement, TransactionOptions, TransactionTrait, Value,
};

const HISTORY_SCHEMA_VERSION: i64 = 3;
/// 单页返回的总 payload 字节预算；达到预算即停止装入更多条目。
const PAGE_BYTE_BUDGET: usize = 2 * 1024 * 1024;
/// 单条预览预算必须小于整页预算，否则一条预览都无法落入一页。
const _: () = assert!(TIMELINE_ITEM_PREVIEW_BYTES < PAGE_BYTE_BUDGET);
/// 一页 Turn 最多返回的 Turn 行数（调用方 `limit` 的上限）。
const TURN_PAGE_ROW_LIMIT: usize = 200;
/// 一条 Turn 在一个 Turn 页里最多返回的条目行数。
///
/// 整页字节预算已经限制返回字节，但 SQL 读取本身也必须先有行上限，否则一条巨型 Turn 会在
/// 裁剪之前被整条读进内存。达到行上限的剩余条目由 `next_cursor` 在同一 Turn 内继续取回。
const TURN_PAGE_ITEM_LIMIT: usize = 500;
/// 一页 agent 会话窗口最多返回的条目行数（与 `session_page` 的 1..=50 契约一致）。
const AGENT_PAGE_ROW_LIMIT: usize = 50;

/// One independently owned Thread history database handle.
///
/// Construction performs no filesystem or database IO. Cold reads open the *existing* database
/// read-only and never create the directory/database nor run schema mutations, while writes lazily
/// create and upgrade through the single writer connection. A deleted or damaged history store
/// therefore stays untouched by a page/turn/item query instead of being silently rebuilt, and the
/// per-Thread history writer stays the only owner that initializes or upgrades the schema.
#[derive(Clone)]
pub(crate) struct HistoryStore {
    state: std::sync::Arc<HistoryStoreState>,
}

/// One weak, copyable view of a history handle's shared connection state.
///
/// It lets the per-Thread persistence coordinator remember which handle *is* the Thread's single
/// ordered writer without keeping it alive: a handle exists exactly as long as a real holder (the
/// effect sink or a live subscription) keeps it, so closing or evicting the Thread releases the
/// database connections and no never-expiring strong history cache can appear.
#[derive(Clone)]
pub(crate) struct HistoryStoreShare(std::sync::Weak<HistoryStoreState>);

impl std::fmt::Debug for HistoryStoreShare {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HistoryStoreShare")
            .field("alive", &self.0.strong_count())
            .finish()
    }
}

struct HistoryStoreState {
    path: PathBuf,
    thread_id: String,
    /// The single writer connection, opened on the first write.
    ///
    /// Effect batches use one connection for short single-writer transactions. The Session owns
    /// ordinal allocation; this connection only persists the ordinals it supplies.
    writer: tokio::sync::OnceCell<HistoryConnection>,
    /// A read-only connection to an *existing* database, opened on the first cold read.
    reader: tokio::sync::OnceCell<HistoryConnection>,
    /// Serializes the first writer initialization against every cold read-only open.
    ///
    /// The writer's `connect(mode=rwc)` creates the file before `initialize` commits the schema and
    /// the identity row, so a read that observed the file inside that window would validate a
    /// half-initialized database and fail closed on a store that is merely still being created. The
    /// writer therefore holds the write side for its whole connect+initialize, while every cold open
    /// takes the read side first: a reader runs either before the file exists (an empty history) or
    /// after the database is fully initialized. Once the writer connection is published both paths
    /// return through the `writer.get()` fast path above it, so the lock is only ever contended
    /// during that first initialization.
    init: tokio::sync::RwLock<()>,
}

/// One live connection plus the database identity it validated.
struct HistoryConnection {
    db: DatabaseConnection,
    database_id: String,
    schema_version: i64,
}

/// 一条终态输入的最小身份记录，外加可重建的 host 提交摘要。
///
/// `request_digest` 只覆盖 host 提交身份（原始 request 与 presentation），因此重复提交可以在
/// 附件草稿已被消费之后重新计算它并校验正文；载荷无法证明正文身份时为 `None`。
pub(crate) struct InputIdentityWrite {
    pub entry: crate::studio::storage::state::InputIdentityEntry,
    pub request_digest: Option<String>,
    pub presentation: pl_protocol::MessagePresentation,
}

/// 持久化幂等命中：保存的最小身份加上保存的 host 提交摘要。
pub(crate) struct InputIdentityReceipt {
    pub entry: crate::studio::storage::state::InputIdentityEntry,
    pub request_digest: Option<String>,
}

/// One durable receipt of a terminal interaction/permission fact.
///
/// `payload` is the canonical record the effect committed, so a repeated command can be answered
/// with the same receipt instead of minting a new revision or a new execution grant.
pub(crate) struct FactReceipt {
    pub kind: String,
    pub payload: String,
}

/// One receipt write that must land in the same transaction as the effect that produced it.
pub(crate) struct FactReceiptWrite {
    pub item_id: String,
    pub revision: u64,
    pub kind: &'static str,
    pub payload: String,
}

/// 一条已受理消息的最小持久身份。
///
/// `digest` 覆盖 core 定义的消息身份（原始 id、source、正文格式与内容，以及冻结的 context），
/// 与 `sequence` 一起在产生该消息的 effect 事务里落库。因此重复投递可以证明“同身份同正文”，
/// 命中时返回原始受理序号；`digest` 为 `None` 只可能来自迁移回填：索引证明该身份已受理，但
/// 无法证明正文，读取方必须 fail-closed，绝不静默重投。
pub(crate) struct MessageIdentityWrite {
    pub message_id: String,
    /// 该消息在时间线上的条目身份，便于与 `history_items` 对齐排查。
    pub item_id: String,
    pub sequence: u64,
    pub digest: Option<String>,
}

/// 持久化消息身份命中：原始受理序号与保存的正文摘要。
pub(crate) struct MessageIdentityReceipt {
    pub sequence: u64,
    pub digest: Option<String>,
}

/// One effect's durable history write, committed as a single transaction.
pub(crate) struct EffectCommit<'a> {
    pub items: &'a [ThreadItem],
    pub rolled_back_turns: &'a std::collections::BTreeSet<String>,
    /// Minimal identities of the terminal inputs this effect's state still carries.
    pub identities: &'a [InputIdentityWrite],
    /// Minimal identities of the messages this effect admitted, committed with the same transaction.
    pub messages: &'a [MessageIdentityWrite],
    /// Terminal fact receipts produced by this effect.
    pub receipts: &'a [FactReceiptWrite],
    /// Task lifecycle and full delivery are authority, not lossy call statistics.
    pub tasks: &'a [pl_core::thread::task::TaskRecord],
    pub deliveries: &'a [pl_core::thread::ToolDelivery],
    pub attempt: Option<&'a pl_core::thread::journal::AttemptUpdate>,
}

impl std::fmt::Debug for HistoryStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HistoryStore")
            .field("thread_id", &self.state.thread_id)
            .finish_non_exhaustive()
    }
}

impl HistoryStore {
    /// Binds one Thread history database path without touching the filesystem or database.
    ///
    /// The first write creates the directory/database and initializes/upgrades the schema; reads
    /// only ever observe an existing database (or an explicitly empty history) and never mutate a
    /// byte, so an absent or damaged store cannot be resurrected by pagination or `read_item`.
    pub(crate) async fn open(path: &Path, thread_id: &str) -> Result<Self> {
        ensure!(
            !thread_id.is_empty(),
            "history Thread identity must not be empty"
        );
        Ok(Self {
            state: std::sync::Arc::new(HistoryStoreState {
                path: path.to_owned(),
                thread_id: thread_id.to_owned(),
                writer: tokio::sync::OnceCell::new(),
                reader: tokio::sync::OnceCell::new(),
                init: tokio::sync::RwLock::new(()),
            }),
        })
    }

    /// A weak view of this handle's shared connection state, for the per-Thread writer registry.
    pub(crate) fn shared(&self) -> HistoryStoreShare {
        HistoryStoreShare(std::sync::Arc::downgrade(&self.state))
    }

    /// The handle behind one share, or `None` once its last real holder released it.
    ///
    /// The upgraded handle is the same logical connection state — the same writer and the same
    /// validated database identity — not a new database client.
    pub(crate) fn from_share(share: &HistoryStoreShare) -> Option<Self> {
        share.0.upgrade().map(|state| Self { state })
    }

    /// The connection cold reads use, or `None` when no history database exists yet.
    ///
    /// The writer connection, once opened, is authoritative for reads too, so a hot writer and an
    /// in-memory test store never observe two divergent databases. Otherwise the existing file is
    /// opened read-only and validated; a missing file is reported as an empty history and is never
    /// created here, so a cold read leaves an absent store absent. A read that races the first
    /// writer initialization waits behind it instead of validating the half-initialized database
    /// that `connect(mode=rwc)` just created.
    async fn reader(&self) -> Result<Option<&HistoryConnection>> {
        if let Some(writer) = self.state.writer.get() {
            return Ok(Some(writer));
        }
        // The first writer may have created the file with `connect(mode=rwc)` while its schema and
        // identity are still uncommitted. Take the read side of the initialization lock before
        // touching the file, so we either run before the file exists or after it is fully
        // initialized; never against a half-initialized database.
        let _initializing = self.state.init.read().await;
        // Re-check under the guard: the writer may have published its connection while we waited.
        if let Some(writer) = self.state.writer.get() {
            return Ok(Some(writer));
        }
        if !tokio::fs::try_exists(&self.state.path).await? {
            return Ok(None);
        }
        let reader = self
            .state
            .reader
            .get_or_try_init(|| async {
                let db = connect_read_only(&self.state.path).await?;
                let (database_id, schema_version) =
                    validate_existing(&db, &self.state.thread_id).await?;
                Ok::<HistoryConnection, anyhow::Error>(HistoryConnection {
                    db,
                    database_id,
                    schema_version,
                })
            })
            .await?;
        Ok(Some(reader))
    }

    /// The single writer connection, created and upgraded on first use.
    ///
    /// The whole first connect+initialize runs under the write side of the initialization lock, so a
    /// cold read that races it cannot observe the created-but-uninitialized database. The guard is
    /// released as soon as the connection is published, and every later call returns through the
    /// fast path above it.
    async fn writer(&self) -> Result<&HistoryConnection> {
        if let Some(writer) = self.state.writer.get() {
            return Ok(writer);
        }
        let _initializing = self.state.init.write().await;
        self.state
            .writer
            .get_or_try_init(|| async {
                let parent = self
                    .state
                    .path
                    .parent()
                    .context("history database path has no parent directory")?;
                tokio::fs::create_dir_all(parent).await?;
                let db = connect(crate::studio::paths::sqlite_url(&self.state.path)).await?;
                let database_id = initialize(&db, &self.state.thread_id).await?;
                Ok::<HistoryConnection, anyhow::Error>(HistoryConnection {
                    db,
                    database_id,
                    schema_version: HISTORY_SCHEMA_VERSION,
                })
            })
            .await
    }

    /// Atomically applies one immutable writer batch and advances its fixed waterline.
    #[cfg(test)]
    pub(crate) async fn commit(
        &self,
        write_seq: u64,
        items: &[ThreadItem],
        turns: &[TimelineTurn],
    ) -> Result<()> {
        ensure!(write_seq > 0, "history write sequence must be positive");
        let write_seq = integer(write_seq)?;
        let tx = begin_write(&self.writer().await?.db).await?;
        let current = applied_write_seq(&tx).await?;
        if write_seq <= current {
            tx.rollback().await?;
            return Ok(());
        }
        for item in items {
            self.upsert_item(&tx, write_seq, item.clone()).await?;
        }
        for turn in turns {
            self.upsert_turn(&tx, write_seq, turn).await?;
        }
        if write_seq > current {
            tx.execute_raw(statement(
                "UPDATE history_meta SET applied_write_seq=? WHERE id=1",
                vec![write_seq.into()],
            ))
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Commits one effect's history facts, including every durable identity index.
    ///
    /// The minimal input/message identities and terminal receipts one effect produced are written in
    /// the same transaction as its items and Turn rows, so an index can never be durable without the
    /// effect it describes (or the other way round) after a crash between two commits.
    pub(crate) async fn commit_effect(
        &self,
        write_seq: u64,
        commit: EffectCommit<'_>,
    ) -> Result<()> {
        let EffectCommit {
            items,
            rolled_back_turns,
            identities,
            messages,
            receipts,
            tasks,
            deliveries,
            attempt,
        } = commit;
        ensure!(write_seq > 0, "history write sequence must be positive");
        let write_seq = integer(write_seq)?;
        let tx = begin_write(&self.writer().await?.db).await?;
        let current = applied_write_seq(&tx).await?;
        if write_seq <= current {
            tx.rollback().await?;
            return Ok(());
        }
        let mut turn_ids = std::collections::BTreeSet::new();
        for item in items {
            if !item.turn_id.is_empty() {
                turn_ids.insert(item.turn_id.clone());
            }
            self.upsert_item(&tx, write_seq, item.clone()).await?;
        }
        for turn_id in turn_ids {
            let Some(row) = tx
                .query_one_raw(statement(
                    "SELECT payload FROM history_items WHERE item_id=?",
                    vec![crate::studio::thread_projection::order::turn_id(&turn_id).into()],
                ))
                .await?
            else {
                continue;
            };
            let payload: String = row.try_get("", "payload")?;
            let item: ThreadItem = serde_json::from_str(&payload)?;
            let ThreadItemState::Turn(turn) = item.state() else {
                continue;
            };
            let last = tx
                .query_one_raw(statement(
                    "SELECT item_id FROM history_items
                     WHERE turn_id=? AND kind != 'contextCompaction'
                     ORDER BY ordinal DESC LIMIT 1",
                    vec![turn_id.clone().into()],
                ))
                .await?
                .context("history Turn has no last item")?
                .try_get::<String>("", "item_id")?;
            self.upsert_turn(
                &tx,
                write_seq,
                &TimelineTurn {
                    turn: pl_protocol::Turn {
                        input_id: turn.input_id().map(str::to_owned),
                        id: turn_id.clone(),
                        thread_id: self.state.thread_id.clone(),
                        revision: item.revision,
                        state: turn.state().clone(),
                        updated_at: item.updated_at,
                    },
                    last_item_id: last,
                    context_disposition: if rolled_back_turns.contains(&turn_id) {
                        pl_protocol::ThreadContextDisposition::RolledBack
                    } else {
                        pl_protocol::ThreadContextDisposition::Active
                    },
                },
            )
            .await?;
        }
        for identity in identities {
            write_input_identity(&tx, write_seq, identity).await?;
        }
        for message in messages {
            write_message_identity(&tx, write_seq, message).await?;
        }
        for receipt in receipts {
            write_fact_receipt(&tx, write_seq, receipt).await?;
        }
        if let Some(attempt) = attempt
            && let pl_core::thread::AttemptOutcome::Committed(output) = &attempt.outcome
        {
            for call in output.tool_calls.iter() {
                write_tool_task(
                    &tx,
                    write_seq,
                    &pl_core::thread::task::TaskRecord {
                        id: format!("task:{}", call.call_id),
                        call_id: call.call_id.clone(),
                        tool_id: call.tool_id.clone(),
                        turn_id: attempt.turn_id.clone(),
                        revision: 0,
                        status: pl_core::thread::task::TaskStatus::Running,
                        cancel_requested: false,
                        acknowledgement: None,
                    },
                )
                .await?;
            }
        }
        for task in tasks {
            write_tool_task(&tx, write_seq, task).await?;
        }
        for delivery in deliveries {
            write_tool_delivery(&tx, write_seq, delivery).await?;
        }
        if write_seq > current {
            tx.execute_raw(statement(
                "UPDATE history_meta SET applied_write_seq=? WHERE id=1",
                vec![write_seq.into()],
            ))
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Reads the committed task identity and exact delivery, never a call-statistics projection.
    pub(crate) async fn tool_task(
        &self,
        task_id: &str,
    ) -> Result<Option<pl_core::thread::cold::DurableToolTask>> {
        let Some(connection) = self.reader().await? else {
            return Ok(None);
        };
        let call_id = task_id.strip_prefix("task:").unwrap_or(task_id);
        let Some(row) = connection
            .db
            .query_one_raw(statement(
                "SELECT task_payload,delivery_payload FROM history_tool_tasks WHERE call_id=?",
                vec![call_id.to_owned().into()],
            ))
            .await?
        else {
            return Ok(None);
        };
        let task: String = row.try_get("", "task_payload")?;
        let delivery: Option<String> = row.try_get("", "delivery_payload")?;
        Ok(Some(pl_core::thread::cold::DurableToolTask {
            task: serde_json::from_str(&task)?,
            delivery: delivery
                .map(|body| serde_json::from_str(&body))
                .transpose()?,
        }))
    }

    /// Finds committed deliveries that may reference one archived tool attachment.
    ///
    /// The delivery is the durable authority for the full resource reference. The SQL substring
    /// only narrows candidates; the caller decodes each delivery and verifies its typed media
    /// projection before granting access to any bytes.
    pub(crate) async fn tool_media_deliveries(
        &self,
        attachment_id: &str,
    ) -> Result<Vec<pl_core::thread::ToolDelivery>> {
        let Some(connection) = self.reader().await? else {
            return Ok(Vec::new());
        };
        let rows = connection
            .db
            .query_all_raw(statement(
                "SELECT delivery_payload FROM history_tool_tasks
                 WHERE delivery_payload IS NOT NULL AND instr(delivery_payload, ?) > 0",
                vec![attachment_id.to_owned().into()],
            ))
            .await?;
        rows.into_iter()
            .map(|row| {
                let payload: String = row.try_get("", "delivery_payload")?;
                Ok(serde_json::from_str(&payload)?)
            })
            .collect()
    }

    /// Reads the minimal identity of a previously admitted input, if the durable index knows it.
    ///
    /// This is the cross-restart idempotency lookup for `submitPrompt`: the returned record lets
    /// the runtime rebuild the original receipt without re-admitting the input or replaying history.
    /// Reads the newest durable receipt of one interaction/permission identity.
    ///
    /// This is the cross-window/cross-restart authority for a repeated terminal interaction
    /// command: `payload` is the exact committed record, so the host can answer with the original
    /// receipt or reject a conflicting payload instead of minting a new revision, granting a new
    /// execution permission or replaying the effect. A missing store or receipt returns `None`.
    pub(crate) async fn latest_fact_receipt(&self, item_id: &str) -> Result<Option<FactReceipt>> {
        if item_id.is_empty() {
            return Ok(None);
        }
        let Some(connection) = self.reader().await? else {
            return Ok(None);
        };
        let row = connection
            .db
            .query_one_raw(statement(
                "SELECT revision,kind,digest,payload FROM history_fact_receipts
                 WHERE item_id=? ORDER BY revision DESC LIMIT 1",
                vec![item_id.to_owned().into()],
            ))
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(FactReceipt {
            kind: row.try_get("", "kind")?,
            payload: row.try_get("", "payload")?,
        }))
    }

    pub(crate) async fn input_identity(
        &self,
        input_id: &str,
    ) -> Result<Option<InputIdentityReceipt>> {
        let Some(connection) = self.reader().await? else {
            return Ok(None);
        };
        let row = connection
            .db
            .query_one_raw(statement(
                "SELECT payload,request_digest FROM history_input_identities WHERE item_id=?",
                vec![input_id.into()],
            ))
            .await?;
        match row {
            Some(row) => Ok(Some(InputIdentityReceipt {
                entry: serde_json::from_str(&row.try_get::<String>("", "payload")?)?,
                request_digest: row.try_get::<Option<String>>("", "request_digest")?,
            })),
            None => Ok(None),
        }
    }

    /// Only a committed hidden disposition can discharge a pruned input without a timeline row.
    pub(crate) async fn hidden_input_identities(
        &self,
        ids: impl IntoIterator<Item = String>,
    ) -> Result<std::collections::BTreeSet<String>> {
        let ids = ids.into_iter().collect::<std::collections::BTreeSet<_>>();
        if ids.is_empty() {
            return Ok(Default::default());
        }
        let Some(connection) = self.reader().await? else {
            return Ok(Default::default());
        };
        // A cold v2 reader may page history without touching the database. An active projection
        // needs the new proof column, so its owning writer upgrades that existing database first.
        let connection = if connection.schema_version < HISTORY_SCHEMA_VERSION {
            self.writer().await?
        } else {
            connection
        };
        let mut hidden = std::collections::BTreeSet::new();
        let ids = ids.into_iter().collect::<Vec<_>>();
        for batch in ids.chunks(400) {
            let placeholders = vec!["?"; batch.len()].join(",");
            let rows = connection
                .db
                .query_all_raw(statement(
                    &format!(
                        "SELECT item_id,presentation FROM history_input_identities \
                         WHERE item_id IN ({placeholders})"
                    ),
                    batch.iter().cloned().map(Into::into).collect(),
                ))
                .await?;
            for row in rows {
                let id: String = row.try_get("", "item_id")?;
                match row
                    .try_get::<Option<String>>("", "presentation")?
                    .as_deref()
                {
                    Some("hidden") => {
                        hidden.insert(id);
                    }
                    Some("visible") | None => {}
                    Some(other) => anyhow::bail!("invalid input presentation {other}"),
                }
            }
        }
        Ok(hidden)
    }

    /// Reads the minimal durable identity of one accepted message, if the index knows it.
    ///
    /// This is the cross-window/cross-restart idempotency lookup for message delivery: the returned
    /// sequence is the original admission receipt and the digest is the proof of the accepted body.
    /// A row without a digest can only come from a migration backfill, and the caller must treat it
    /// as unverifiable instead of re-delivering. A missing store or unknown identity returns `None`,
    /// while a damaged store fails so the caller can fail closed rather than admit a duplicate.
    pub(crate) async fn message_identity(
        &self,
        message_id: &str,
    ) -> Result<Option<MessageIdentityReceipt>> {
        if message_id.is_empty() {
            return Ok(None);
        }
        let Some(connection) = self.reader().await? else {
            return Ok(None);
        };
        // 缺少索引表只意味着这条身份“未知”，不是存储故障：升级前写入的库还没有这张表，写者会在
        // 下一次提交时补建它，冷读既不能失败也不能替它建表。是否真有这条历史由上层的旧历史探测
        // 决定，从而让索引之前的消息 fail-closed 而不是被当成新消息重投。缺表之外的一切错误
        // （IO、损坏库、schema 不可读）都必须原样上抛，绝不伪装成“没有这条身份”。
        if !message_identity_index_exists(connection).await? {
            return Ok(None);
        }
        let row = connection
            .db
            .query_one_raw(statement(
                "SELECT sequence,digest FROM history_message_identities WHERE message_id=?",
                vec![message_id.into()],
            ))
            .await?;
        match row {
            Some(row) => {
                let sequence = u64::try_from(row.try_get::<i64>("", "sequence")?)
                    .context("durable message identity has an invalid admission sequence")?;
                Ok(Some(MessageIdentityReceipt {
                    sequence,
                    digest: row.try_get::<Option<String>>("", "digest")?,
                }))
            }
            None => Ok(None),
        }
    }

    pub(crate) async fn existing_items(
        &self,
        ids: impl IntoIterator<Item = String>,
    ) -> Result<std::collections::BTreeMap<String, ThreadItem>> {
        let Some(connection) = self.reader().await? else {
            return Ok(std::collections::BTreeMap::new());
        };
        let mut items = std::collections::BTreeMap::new();
        let ids = ids
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        for batch in ids.chunks(400) {
            let placeholders = vec!["?"; batch.len()].join(",");
            let sql = format!(
                "SELECT item_id,payload FROM history_items WHERE item_id IN ({placeholders})"
            );
            let rows = connection
                .db
                .query_all_raw(statement(
                    &sql,
                    batch.iter().cloned().map(Into::into).collect(),
                ))
                .await?;
            for row in rows {
                let id: String = row.try_get("", "item_id")?;
                let payload: String = row.try_get("", "payload")?;
                items.insert(id, serde_json::from_str(&payload)?);
            }
        }
        Ok(items)
    }

    /// Resolves the exact rows an effect committed before releasing their in-memory owner.
    pub(crate) async fn committed_chat_items(
        &self,
        ids: impl IntoIterator<Item = String>,
    ) -> Result<Vec<ChatItem>> {
        self.existing_items(ids)
            .await?
            .into_values()
            .map(|item| chat_item(item, true))
            .collect()
    }

    pub(crate) async fn items_for_turn(&self, turn_id: &str) -> Result<Vec<ThreadItem>> {
        let Some(connection) = self.reader().await? else {
            return Ok(Vec::new());
        };
        query_items(
            &connection.db,
            "SELECT payload FROM history_items WHERE turn_id=? ORDER BY ordinal ASC",
            vec![turn_id.into()],
        )
        .await
    }

    /// Newest durable terminal Turn item, or `None` when no Turn has finished yet.
    ///
    /// Root product observation uses this single bounded row (ordinal primary key, descending) to
    /// repair a directory and terminal-derived state its effect window already dropped; it never
    /// scans the terminal history to find the newest fact.
    pub(crate) async fn latest_terminal_turn(&self) -> Result<Option<ThreadItem>> {
        let Some(connection) = self.reader().await? else {
            return Ok(None);
        };
        let row = connection
            .db
            .query_one_raw(statement(
                "SELECT payload FROM history_items
                 WHERE kind='turn' AND lifecycle='terminal'
                 ORDER BY ordinal DESC LIMIT 1",
                vec![],
            ))
            .await?;
        match row {
            Some(row) => Ok(Some(serde_json::from_str(
                &row.try_get::<String>("", "payload")?,
            )?)),
            None => Ok(None),
        }
    }

    /// Reads at most `limit` durable terminal Turn items after `after_ordinal`, in ordinal order.
    ///
    /// Live product observation uses this only to reconstruct terminal facts after its bounded
    /// effect window lost the effects that produced them, so a lagged observer still reports every
    /// terminal Turn instead of silently advancing past it. Both the ordinal cursor and the row
    /// limit bound one read, so recovery pages forward instead of scanning the whole timeline.
    pub(crate) async fn terminal_turns_after(
        &self,
        after_ordinal: u64,
        limit: usize,
    ) -> Result<Vec<ThreadItem>> {
        let Some(connection) = self.reader().await? else {
            return Ok(Vec::new());
        };
        query_items(
            &connection.db,
            "SELECT payload FROM history_items
             WHERE kind='turn' AND lifecycle='terminal' AND ordinal > ?
             ORDER BY ordinal ASC LIMIT ?",
            vec![integer(after_ordinal)?.into(), integer(limit)?.into()],
        )
        .await
    }

    pub(crate) async fn watermark(&self) -> Result<u64> {
        match self.reader().await? {
            Some(connection) => connection_watermark(connection).await,
            None => Ok(0),
        }
    }

    async fn upsert_item(
        &self,
        db: &impl ConnectionTrait,
        write_seq: i64,
        mut item: ThreadItem,
    ) -> Result<()> {
        ensure!(
            item.thread_id == self.state.thread_id && !item.id.is_empty(),
            "history item identity does not belong to this Thread"
        );
        ensure!(
            item.ordinal > 0,
            "history item ordinal must be assigned by the Session"
        );
        ensure!(item.revision > 0, "history item revision must be positive");
        let revision = integer(item.revision)?;
        let existing = db
            .query_one_raw(statement(
                "SELECT ordinal,turn_id,kind,revision,lifecycle,created_at,payload
                 FROM history_items WHERE item_id=?",
                vec![item.id.clone().into()],
            ))
            .await?;
        let kind = kind_label(item.kind());
        let lifecycle = if item.is_terminal() {
            "terminal"
        } else {
            "open"
        };
        if let Some(row) = existing {
            let old_ordinal: i64 = row.try_get("", "ordinal")?;
            let old_turn: String = row.try_get("", "turn_id")?;
            let old_kind: String = row.try_get("", "kind")?;
            let old_revision: i64 = row.try_get("", "revision")?;
            let old_lifecycle: String = row.try_get("", "lifecycle")?;
            let old_created_at: i64 = row.try_get("", "created_at")?;
            let old_payload: String = row.try_get("", "payload")?;
            ensure!(
                (old_turn == item.turn_id || old_turn.is_empty()) && old_kind == kind,
                "history item identity fields changed"
            );
            ensure!(
                item.ordinal == u64::try_from(old_ordinal)?,
                "history item ordinal changed for {}",
                item.id
            );
            item.created_at = old_created_at;
            let payload = serde_json::to_string(&item)?;
            if old_revision > revision {
                return Ok(());
            }
            if old_revision == revision {
                ensure!(old_payload == payload, "history item revision conflict");
                return Ok(());
            }
            // A later canonical revision supersedes the stored item, which is how the projection
            // completes facts it could not know earlier (binding a queued input to its Turn, or
            // settling a pending input into consumed/discarded). Only a late non-final draft may
            // not rewrite an item that already reached a terminal state.
            ensure!(
                old_lifecycle != "terminal" || item.is_terminal(),
                "terminal history item was revised"
            );
            db.execute_raw(statement(
                "UPDATE history_items SET
                    revision=?,lifecycle=?,turn_id=?,updated_at=?,payload=?,last_write_seq=?
                 WHERE item_id=?",
                vec![
                    revision.into(),
                    lifecycle.into(),
                    item.turn_id.clone().into(),
                    item.updated_at.into(),
                    payload.into(),
                    write_seq.into(),
                    item.id.clone().into(),
                ],
            ))
            .await?;
            if old_turn.is_empty() && !item.turn_id.is_empty() {
                // A queued message may predate its Turn. Binding that *same* item identity to
                // the Turn can extend the indexed start backwards, but no unrelated write may
                // change the first ordinal checked by `upsert_turn` below.
                db.execute_raw(statement(
                    "UPDATE history_turns SET first_ordinal=MIN(first_ordinal, ?) WHERE turn_id=?",
                    vec![old_ordinal.into(), item.turn_id.clone().into()],
                ))
                .await?;
            }
            return Ok(());
        }
        let ordinal = integer(item.ordinal)?;
        let payload = serde_json::to_string(&item)?;
        db.execute_raw(statement(
            "INSERT INTO history_items(
                ordinal,item_id,turn_id,kind,revision,lifecycle,
                created_at,updated_at,payload,last_write_seq
             ) VALUES(?,?,?,?,?,?,?,?,?,?)",
            vec![
                ordinal.into(),
                item.id.clone().into(),
                item.turn_id.clone().into(),
                kind.into(),
                revision.into(),
                lifecycle.into(),
                item.created_at.into(),
                item.updated_at.into(),
                payload.into(),
                write_seq.into(),
            ],
        ))
        .await?;
        Ok(())
    }

    async fn upsert_turn(
        &self,
        db: &impl ConnectionTrait,
        write_seq: i64,
        turn: &TimelineTurn,
    ) -> Result<()> {
        ensure!(
            turn.turn.thread_id == self.state.thread_id && !turn.turn.id.is_empty(),
            "history Turn identity does not belong to this Thread"
        );
        let first_ordinal = db
            .query_one_raw(statement(
                "SELECT MIN(ordinal) AS ordinal FROM history_items WHERE turn_id=?",
                vec![turn.turn.id.clone().into()],
            ))
            .await?
            .and_then(|row| row.try_get::<Option<i64>>("", "ordinal").ok().flatten())
            .context("history Turn has no item")?;
        let last_ordinal = db
            .query_one_raw(statement(
                "SELECT ordinal FROM history_items WHERE item_id=? AND turn_id=?",
                vec![
                    turn.last_item_id.clone().into(),
                    turn.turn.id.clone().into(),
                ],
            ))
            .await?
            .context("history Turn last item is missing")?
            .try_get::<i64>("", "ordinal")?;
        let revision = integer(turn.turn.revision)?;
        let payload = serde_json::to_string(turn)?;
        let existing = db
            .query_one_raw(statement(
                "SELECT first_ordinal,last_ordinal,revision,payload,last_write_seq
                 FROM history_turns WHERE turn_id=?",
                vec![turn.turn.id.clone().into()],
            ))
            .await?;
        if let Some(row) = existing {
            let old_first: i64 = row.try_get("", "first_ordinal")?;
            let old_last: i64 = row.try_get("", "last_ordinal")?;
            let old_revision: i64 = row.try_get("", "revision")?;
            let old_payload: String = row.try_get("", "payload")?;
            let old_write_seq: i64 = row.try_get("", "last_write_seq")?;
            ensure!(
                old_first == first_ordinal,
                "history Turn first item changed"
            );
            if old_revision > revision {
                return Ok(());
            }
            if old_revision == revision {
                if old_payload == payload {
                    return Ok(());
                }
                // 同一 revision 的 Turn 行只能被同一条事实的“投影前进”重写，绝不能被另一份事实
                // 替换：Turn 本体（state/input_id/updated_at，与它的 revision 一起来自那条已落库的
                // Turn 条目）必须逐字相同，结尾只能向后延伸，上下文处置只允许由 Active 落定为
                // RolledBack。其余差异都说明有人在同一 revision 下改写了这条 Turn，必须显式失败，
                // 而不是靠更大的 write_seq 静默覆盖旧 payload。
                let stored: TimelineTurn = serde_json::from_str(&old_payload)?;
                ensure!(
                    stored.turn == turn.turn,
                    "history Turn revision conflict: the Turn fact changed at revision {revision}"
                );
                ensure!(
                    last_ordinal >= old_last,
                    "history Turn end moved backwards at revision {revision}"
                );
                ensure!(
                    stored.context_disposition == turn.context_disposition
                        || stored.context_disposition
                            == pl_protocol::ThreadContextDisposition::Active,
                    "history Turn context disposition regressed at revision {revision}"
                );
                ensure!(
                    write_seq >= old_write_seq,
                    "history Turn write sequence moved backwards"
                );
            }
            db.execute_raw(statement(
                "UPDATE history_turns SET
                    last_ordinal=?,revision=?,payload=?,last_write_seq=?
                 WHERE turn_id=?",
                vec![
                    last_ordinal.into(),
                    revision.into(),
                    payload.into(),
                    write_seq.into(),
                    turn.turn.id.clone().into(),
                ],
            ))
            .await?;
            return Ok(());
        }
        db.execute_raw(statement(
            "INSERT INTO history_turns(
                turn_id,first_ordinal,last_ordinal,revision,payload,last_write_seq
             ) VALUES(?,?,?,?,?,?)",
            vec![
                turn.turn.id.clone().into(),
                first_ordinal.into(),
                last_ordinal.into(),
                revision.into(),
                payload.into(),
                write_seq.into(),
            ],
        ))
        .await?;
        Ok(())
    }

    /// Reads one bounded SQL page without activating or consulting a Thread owner.
    pub(crate) async fn page(&self, query: &TimelineQuery, limit: usize) -> Result<TimelinePage> {
        let limit = limit.clamp(1, 100);
        // 冷读不得创建目录/数据库：还没有历史库时，空窗口才是真实答案，且这里绝不重建它。
        let Some(connection) = self.reader().await? else {
            return Ok(TimelinePage {
                thread_id: self.state.thread_id.clone(),
                database_id: String::new(),
                watermark: 0,
                older_cursor: None,
                newer_cursor: None,
                first_item_id: None,
                last_item_id: None,
                truncated: false,
                previews: Vec::new(),
                items: Vec::new(),
                turns: Vec::new(),
            });
        };
        let (mut rows, edge) = match query {
            TimelineQuery::Latest => {
                let mut rows = query_items(
                    &connection.db,
                    "SELECT payload FROM history_items ORDER BY ordinal DESC LIMIT ?",
                    vec![integer(limit + 1)?.into()],
                )
                .await?;
                rows.reverse();
                if rows.len() > limit {
                    rows.remove(0);
                }
                // 最新的一窗从最新端保留；更旧的一端与本页相邻，可由 cursor 继续取回。
                (rows, BudgetEdge::Newest)
            }
            TimelineQuery::Before { item_id } => {
                let ordinal = self.anchor_ordinal(connection, item_id).await?;
                let mut rows = query_items(
                    &connection.db,
                    "SELECT payload FROM history_items
                         WHERE ordinal < ? ORDER BY ordinal DESC LIMIT ?",
                    vec![ordinal.into(), integer(limit)?.into()],
                )
                .await?;
                rows.reverse();
                // 向更旧滚动：返回窗口必须紧贴锚点，预算不足时丢弃更远（更旧）的一端。
                (rows, BudgetEdge::Newest)
            }
            TimelineQuery::After { item_id } => {
                let ordinal = self.anchor_ordinal(connection, item_id).await?;
                // 向更新滚动：起点紧贴锚点，预算不足时丢弃更远（更新）的一端。
                (
                    query_items(
                        &connection.db,
                        "SELECT payload FROM history_items
                         WHERE ordinal > ? ORDER BY ordinal ASC LIMIT ?",
                        vec![ordinal.into(), integer(limit)?.into()],
                    )
                    .await?,
                    BudgetEdge::Oldest,
                )
            }
            TimelineQuery::Around { item_id } => {
                let ordinal = self.anchor_ordinal(connection, item_id).await?;
                let before = limit / 2;
                let mut rows = query_items(
                    &connection.db,
                    "SELECT payload FROM history_items
                         WHERE ordinal < ? ORDER BY ordinal DESC LIMIT ?",
                    vec![ordinal.into(), integer(before)?.into()],
                )
                .await?;
                rows.reverse();
                let remaining = limit.saturating_sub(rows.len());
                rows.extend(
                    query_items(
                        &connection.db,
                        "SELECT payload FROM history_items
                         WHERE ordinal >= ? ORDER BY ordinal ASC LIMIT ?",
                        vec![ordinal.into(), integer(remaining)?.into()],
                    )
                    .await?,
                );
                // 显式跳转必须能读到锚点本身：预算不足时从更旧的一侧先收缩，绝不丢掉锚点行。
                // 锚点 ordinal 来自 SQLite rowid（非负），预算收缩按 u64 比较。
                (rows, BudgetEdge::Around(u64::try_from(ordinal)?))
            }
        };
        // 单条预览预算先于总字节预算：超大条目压缩为同身份预览，其余页预算才可能满足。
        let mut previews = Vec::new();
        for item in rows.iter_mut() {
            let (bounded, reference) = preview_timeline_item(item, TIMELINE_ITEM_PREVIEW_BYTES);
            if let Some(reference) = reference {
                *item = bounded;
                previews.push(reference);
            }
        }
        let truncated = apply_byte_budget(&mut rows, edge)?;
        // 只保留真正留在本页的条目引用；被总预算淘汰的条目不再出现在引用中。
        let kept = rows
            .iter()
            .map(|item| item.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        previews.retain(|reference| kept.contains(reference.item_id.as_str()));
        let watermark = connection_watermark(connection).await?;
        let first = rows.first();
        let last = rows.last();
        let older_cursor = match first {
            Some(item) if exists_before(connection, item.ordinal).await? => {
                Some(cursor(connection, &self.state.thread_id, item, watermark))
            }
            _ => None,
        };
        let newer_cursor = match last {
            Some(item) if exists_after(connection, item.ordinal).await? => {
                Some(cursor(connection, &self.state.thread_id, item, watermark))
            }
            _ => None,
        };
        let turns = turns_for(connection, &rows).await?;
        Ok(TimelinePage {
            thread_id: self.state.thread_id.clone(),
            database_id: connection.database_id.clone(),
            watermark,
            older_cursor,
            newer_cursor,
            first_item_id: first.map(|item| item.id.clone()),
            last_item_id: last.map(|item| item.id.clone()),
            truncated,
            previews,
            items: rows,
            turns,
        })
    }

    /// Reads one complete item straight from the database, bypassing the preview budget.
    ///
    /// Ordinary pagination stays pure SQL and still returns an oversized item as
    /// [`pl_protocol::TimelineItemPreview`]; this entry point resolves such a preview by identity
    /// and returns the untouched payload with its canonical ordinal, revision and read watermark.
    ///
    /// 这是按身份的**单条 on-demand 读取**，不是第二份历史：它只返回被点名的那一条正文，不带
    /// 分页、不带邻居、也不把结果缓存进 `HistoryStore`（句柄用完即释放，正文只在调用方手里）。
    /// 因此这条路径刻意没有字节上限——有上限就无法取回超预算条目的完整正文。需要遍历历史时必须
    /// 走 `page`/`turn_page` 的有界窗口，不能靠反复 `read_item` 物化整段历史。
    ///
    /// # Errors
    /// Fails on unknown item identity or a storage read failure.
    pub(crate) async fn read_item(&self, item_id: &str) -> Result<pl_protocol::TimelineItemRead> {
        let connection = self
            .reader()
            .await?
            .context("timeline item is not in this Thread history")?;
        let row = connection
            .db
            .query_one_raw(statement(
                "SELECT ordinal,payload FROM history_items WHERE item_id=?",
                vec![item_id.into()],
            ))
            .await?
            .with_context(|| format!("timeline item {item_id} is not in this Thread history"))?;
        let ordinal = u64::try_from(row.try_get::<i64>("", "ordinal")?)?;
        let payload: String = row.try_get("", "payload")?;
        let item: ThreadItem = serde_json::from_str(&payload)?;
        Ok(pl_protocol::TimelineItemRead {
            thread_id: self.state.thread_id.clone(),
            database_id: connection.database_id.clone(),
            watermark: connection_watermark(connection).await?,
            ordinal,
            item,
        })
    }

    async fn read_chat_item(&self, item_id: &str) -> Result<Option<ChatItem>> {
        let Some(connection) = self.reader().await? else {
            return Ok(None);
        };
        // `read_item` treats a missing identity as an error; only that case is optional here.
        let exists = connection
            .db
            .query_one_raw(statement(
                "SELECT 1 AS present FROM history_items WHERE item_id=? LIMIT 1",
                vec![item_id.into()],
            ))
            .await?
            .is_some();
        if !exists {
            return Ok(None);
        }
        let read = self.read_item(item_id).await?;
        let mut item = read.item;
        item.ordinal = read.ordinal;
        Ok(Some(chat_item(item, true)?))
    }

    async fn latest_allocated_order(&self) -> Result<u64> {
        let Some(connection) = self.reader().await? else {
            return Ok(0);
        };
        let row = connection
            .db
            .query_one_raw(statement(
                "SELECT MAX(ordinal) AS ordinal FROM history_items",
                vec![],
            ))
            .await?
            .context("history ordinal query returned no row")?;
        let maximum: Option<i64> = row.try_get("", "ordinal")?;
        Ok(u64::try_from(maximum.unwrap_or(0))?)
    }

    async fn chat_page(&self, query: ChatQuery, limit: usize) -> Result<HistoryPage> {
        let Some(connection) = self.reader().await? else {
            return Ok(HistoryPage {
                items: Vec::new(),
                has_older: false,
                has_newer: false,
            });
        };
        let limit = limit.clamp(1, 100);
        let rows = match query {
            ChatQuery::Latest => {
                let mut rows = query_chat_ids(
                    &connection.db,
                    "SELECT ordinal,item_id FROM history_items ORDER BY ordinal DESC LIMIT ?",
                    vec![integer(limit)?.into()],
                )
                .await?;
                rows.reverse();
                rows
            }
            ChatQuery::Before(anchor) => {
                let mut rows = query_chat_ids(
                    &connection.db,
                    "SELECT ordinal,item_id FROM history_items WHERE ordinal < ? ORDER BY ordinal DESC LIMIT ?",
                    vec![integer(anchor)?.into(), integer(limit)?.into()],
                )
                .await?;
                rows.reverse();
                rows
            }
            ChatQuery::After(anchor) => query_chat_ids(
                &connection.db,
                "SELECT ordinal,item_id FROM history_items WHERE ordinal > ? ORDER BY ordinal ASC LIMIT ?",
                vec![integer(anchor)?.into(), integer(limit)?.into()],
            )
            .await?,
            ChatQuery::Around(anchor) => {
                let anchor = integer(anchor)?;
                let mut rows = query_chat_ids(
                    &connection.db,
                    "SELECT ordinal,item_id FROM history_items WHERE ordinal < ? ORDER BY ordinal DESC LIMIT ?",
                    vec![anchor.into(), integer(limit / 2)?.into()],
                )
                .await?;
                rows.reverse();
                let remaining = limit - rows.len();
                rows.extend(
                    query_chat_ids(
                        &connection.db,
                        "SELECT ordinal,item_id FROM history_items WHERE ordinal >= ? ORDER BY ordinal ASC LIMIT ?",
                        vec![anchor.into(), integer(remaining)?.into()],
                    )
                    .await?,
                );
                rows
            }
        };
        let (has_older, has_newer) = match (rows.first(), rows.last()) {
            (Some(first), Some(last)) => (
                exists_before(connection, first.0).await?,
                exists_after(connection, last.0).await?,
            ),
            _ => match query {
                ChatQuery::Latest => (false, false),
                ChatQuery::Before(anchor) | ChatQuery::Around(anchor) => (
                    exists_before(connection, anchor).await?,
                    exists(connection, "ordinal >= ?", anchor).await?,
                ),
                ChatQuery::After(anchor) => (
                    exists(connection, "ordinal <= ?", anchor).await?,
                    exists_after(connection, anchor).await?,
                ),
            },
        };
        let mut items = Vec::with_capacity(rows.len());
        for (ordinal, item_id) in rows {
            let row = connection
                .db
                .query_one_raw(statement(
                    "SELECT payload FROM history_items WHERE ordinal=? AND item_id=?",
                    vec![integer(ordinal)?.into(), item_id.clone().into()],
                ))
                .await?
                .with_context(|| format!("chat page item {item_id} disappeared during read"))?;
            let payload: String = row.try_get("", "payload")?;
            let item: ThreadItem = serde_json::from_str(&payload)?;
            ensure!(
                item.id == item_id && item.ordinal == ordinal,
                "chat page item {item_id} differs from its indexed identity or order"
            );
            items.push(chat_preview_item(item, true)?);
        }
        Ok(HistoryPage {
            items,
            has_older,
            has_newer,
        })
    }

    /// 把 wire 锚点解析为 canonical ordinal。
    ///
    /// 接受版本化游标 token（校验 Thread/数据库身份、水位与条目身份）或原始 item
    /// identity；两者都只读数据库。
    async fn anchor_ordinal(&self, connection: &HistoryConnection, anchor: &str) -> Result<i64> {
        let Some(cursor) = TimelineCursor::decode(anchor) else {
            return ordinal(connection, anchor).await;
        };
        ensure!(
            cursor.thread_id == self.state.thread_id,
            "timeline cursor belongs to another Thread"
        );
        ensure!(
            cursor.database_id == connection.database_id,
            "timeline cursor belongs to a rebuilt history database; reload the window"
        );
        ensure!(
            cursor.applied_write_sequence <= connection_watermark(connection).await?,
            "timeline cursor was read from a newer history than this database"
        );
        let row = connection
            .db
            .query_one_raw(statement(
                "SELECT item_id FROM history_items WHERE ordinal=?",
                vec![integer(cursor.ordinal)?.into()],
            ))
            .await?
            .context("timeline cursor item no longer exists; reload the window")?;
        let item_id: String = row.try_get("", "item_id")?;
        ensure!(
            item_id == cursor.item_id,
            "timeline cursor item identity changed; reload the window"
        );
        integer(cursor.ordinal)
    }

    /// Reads one bounded Turn page, newest Turn first, without activating a Thread owner.
    ///
    /// The page is a keyset over `history_turns.last_ordinal` (index-backed, no offset, no full
    /// scan, no sort) and is bounded twice: by the caller's row limit and by [`PAGE_BYTE_BUDGET`]
    /// over the serialized entries, exactly like the item page. When the byte budget stops the page
    /// the returned cursor points at the last entry actually returned — inside the Turn it stopped
    /// in — so the next page resumes exactly there. Every item of every Turn is therefore reachable
    /// by following the cursors in ordinal order, and one page never grows with the size of one
    /// Turn: an oversized Turn is returned as consecutive windows instead of one unbounded body.
    ///
    /// 冷读缺失历史库时返回空 Turn 页，且绝不创建它。
    pub(crate) async fn turn_page(
        &self,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<pl_protocol::ThreadTurnPage> {
        let limit = limit.clamp(1, TURN_PAGE_ROW_LIMIT);
        let Some(connection) = self.reader().await? else {
            return Ok(pl_protocol::ThreadTurnPage {
                next_cursor: None,
                turns: Vec::new(),
            });
        };
        let watermark = connection_watermark(connection).await?;
        let resume = match cursor {
            Some(cursor) => {
                Some(turn_page_resume(connection, &self.state.thread_id, cursor, watermark).await?)
            }
            None => None,
        };
        let mut turns = Vec::new();
        let mut next_cursor = None;
        let mut used = 0_usize;
        // 本页真正返回的最后一条位置（Turn id 与窗口内的条目 ordinal）：续传阶段与本轮批量扫描
        // 都更新它，因此无论是字节预算还是行预算停下，`next_cursor` 都指向真实返回的位置。
        let mut last_returned: Option<(String, i64)> = None;
        // 续传：上一页的字节预算停在某条 Turn 内部时，先取回同一条 Turn 的剩余条目，再从它更旧的
        // Turn 继续。
        let mut older_than = None;
        if let Some(resume) = &resume {
            older_than = Some(resume.end_ordinal);
            if resume.after_ordinal < resume.end_ordinal {
                let entry = turn_page_entry(
                    connection,
                    &resume.turn_id,
                    resume.after_ordinal,
                    PAGE_BYTE_BUDGET,
                )
                .await?
                .context("history cursor does not belong to this Thread")?;
                used = used.saturating_add(entry.bytes);
                last_returned = Some((resume.turn_id.clone(), entry.after_ordinal));
                if entry.more {
                    next_cursor = Some(turn_page_cursor(
                        connection,
                        &self.state.thread_id,
                        &resume.turn_id,
                        entry.after_ordinal,
                        watermark,
                    )?);
                }
                turns.push(entry.history);
                if next_cursor.is_some() {
                    return Ok(pl_protocol::ThreadTurnPage { next_cursor, turns });
                }
            }
        }
        while turns.len() < limit {
            let take = limit - turns.len();
            // 多取一行只为判断“还有更旧的 Turn”；这一行不属于本页，下一页用严格更小的
            // `last_ordinal` 边界重新取回，因此不会跳过任何 Turn。
            let rows = match older_than {
                Some(bound) => {
                    connection
                        .db
                        .query_all_raw(statement(
                            "SELECT turn_id,first_ordinal,last_ordinal FROM history_turns
                             WHERE last_ordinal < ? ORDER BY last_ordinal DESC LIMIT ?",
                            vec![bound.into(), integer(take + 1)?.into()],
                        ))
                        .await?
                }
                None => {
                    connection
                        .db
                        .query_all_raw(statement(
                            "SELECT turn_id,first_ordinal,last_ordinal FROM history_turns
                             ORDER BY last_ordinal DESC LIMIT ?",
                            vec![integer(take + 1)?.into()],
                        ))
                        .await?
                }
            };
            if rows.is_empty() {
                break;
            }
            let more_turns = rows.len() > take;
            let mut last_end = older_than;
            let mut stopped = false;
            for row in rows.into_iter().take(take) {
                let turn_id: String = row.try_get("", "turn_id")?;
                let first_ordinal: i64 = row.try_get("", "first_ordinal")?;
                let end_ordinal: i64 = row.try_get("", "last_ordinal")?;
                let budget = PAGE_BYTE_BUDGET.saturating_sub(used);
                let Some(entry) = turn_page_entry(
                    connection,
                    &turn_id,
                    first_ordinal.saturating_sub(1),
                    budget,
                )
                .await?
                else {
                    continue;
                };
                // 整条 Turn 都装不进本页时留在下一页（游标指向本页最后返回的位置）。本页第一条
                // 永远返回，否则游标无法前进。
                if !turns.is_empty() && entry.bytes > budget {
                    stopped = true;
                    break;
                }
                used = used.saturating_add(entry.bytes);
                last_returned = Some((turn_id, entry.after_ordinal));
                last_end = Some(end_ordinal);
                let window_truncated = entry.more;
                turns.push(entry.history);
                if window_truncated {
                    // 字节或行预算在这条 Turn 内部用尽：游标停在窗口内的最后一条条目上。
                    stopped = true;
                    break;
                }
            }
            if !stopped && !more_turns {
                next_cursor = None;
                break;
            }
            let Some((turn_id, after_ordinal)) = &last_returned else {
                // 唯一一条候选 Turn 都取不到（历史被外部改写）：没有可续传的位置。
                next_cursor = None;
                break;
            };
            next_cursor = Some(turn_page_cursor(
                connection,
                &self.state.thread_id,
                turn_id,
                *after_ordinal,
                watermark,
            )?);
            if stopped || turns.len() >= limit {
                break;
            }
            older_than = last_end;
        }
        Ok(pl_protocol::ThreadTurnPage { next_cursor, turns })
    }

    pub(crate) async fn agent_page(
        &self,
        descending: bool,
        text_only: bool,
        anchor: Option<u64>,
        ceiling: Option<u64>,
        through: Option<u64>,
        limit: usize,
    ) -> Result<(u64, u64, Vec<ThreadItem>, bool)> {
        // 行上限与探测行必须用同一份 limit：早先的写法按调用方 limit 比较 `has_more`，却把
        // SQL LIMIT 截到 51，limit > 50 时会返回 51 条却宣称“没有更多”。这里先收敛 limit，再
        // 多取一行作为“确实还有匹配条目”的证据。
        let limit = limit.clamp(1, AGENT_PAGE_ROW_LIMIT);
        let Some(connection) = self.reader().await? else {
            // 缺失历史库：空窗口，且读操作绝不创建它。
            return Ok((0, 0, Vec::new(), false));
        };
        let watermark = connection_watermark(connection).await?;
        if let Some(through) = through {
            ensure!(
                through <= watermark,
                "agent session cursor is ahead of durable history"
            );
        }
        let ceiling = match ceiling {
            Some(ceiling) => ceiling,
            None => {
                let row = connection
                    .db
                    .query_one_raw(statement(
                        "SELECT COALESCE(MAX(ordinal),0) AS ordinal FROM history_items",
                        vec![],
                    ))
                    .await?
                    .context("history ceiling query returned no row")?;
                u64::try_from(row.try_get::<i64>("", "ordinal")?)?
            }
        };
        let mut predicates = vec!["ordinal <= ?"];
        let mut values = vec![integer(ceiling)?.into()];
        if let Some(anchor) = anchor {
            predicates.push(if descending {
                "ordinal < ?"
            } else {
                "ordinal > ?"
            });
            values.push(integer(anchor)?.into());
        }
        if text_only {
            predicates.push("kind = 'text'");
        }
        values.push(integer(limit + 1)?.into());
        let sql = format!(
            "SELECT payload FROM history_items WHERE {} ORDER BY ordinal {} LIMIT ?",
            predicates.join(" AND "),
            if descending { "DESC" } else { "ASC" }
        );
        let mut items = query_items(&connection.db, &sql, values).await?;
        let has_more = items.len() > limit;
        items.truncate(limit);
        Ok((watermark, ceiling, items, has_more))
    }
}

impl ChatHistory for HistoryStore {
    fn preview(&self, item: &ChatItem) -> std::result::Result<ChatItem, ChatError> {
        if item.body.len() <= TIMELINE_ITEM_PREVIEW_BYTES {
            return Ok(item.clone());
        }
        let decoded: ThreadItem = serde_json::from_str(&item.body)
            .map_err(anyhow::Error::from)
            .map_err(chat_history_error)?;
        let mut visible = chat_preview_item(decoded, item.saved).map_err(chat_history_error)?;
        visible.omitted_bytes = visible.omitted_bytes.saturating_add(item.omitted_bytes);
        Ok(visible)
    }

    async fn latest_allocated_order(&self) -> std::result::Result<u64, ChatError> {
        HistoryStore::latest_allocated_order(self)
            .await
            .map_err(chat_history_error)
    }

    async fn page(
        &self,
        query: ChatQuery,
        limit: usize,
    ) -> std::result::Result<HistoryPage, ChatError> {
        self.chat_page(query, limit)
            .await
            .map_err(chat_history_error)
    }

    async fn item(&self, item_id: &str) -> std::result::Result<Option<ChatItem>, ChatError> {
        self.read_chat_item(item_id)
            .await
            .map_err(chat_history_error)
    }

    async fn read_body(
        &self,
        item_id: &str,
    ) -> std::result::Result<Option<std::sync::Arc<str>>, ChatError> {
        let item = self
            .read_chat_item(item_id)
            .await
            .map_err(chat_history_error)?;
        Ok(item.map(|item| item.body))
    }
}

pub(crate) fn chat_item(item: ThreadItem, saved: bool) -> Result<ChatItem> {
    let body = serde_json::to_string(&item)?;
    Ok(ChatItem {
        item_id: item.id,
        turn_id: item.turn_id,
        order: item.ordinal,
        revision: item.revision,
        part_id: None,
        body: body.into(),
        omitted_bytes: 0,
        saved,
    })
}

fn chat_preview_item(item: ThreadItem, saved: bool) -> Result<ChatItem> {
    let (preview, reference) = preview_timeline_item(&item, TIMELINE_ITEM_PREVIEW_BYTES);
    let mut visible = chat_item(preview, saved)?;
    ensure!(
        visible.body.len() <= TIMELINE_ITEM_PREVIEW_BYTES,
        "chat item {} cannot be previewed within the byte budget",
        item.id
    );
    visible.omitted_bytes = reference.map_or(0, |reference| reference.omitted_bytes);
    Ok(visible)
}

fn chat_history_error(error: anyhow::Error) -> ChatError {
    ChatError::History(error.into_boxed_dyn_error())
}

async fn connect(url: String) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(url);
    options
        .max_connections(1)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .map_sqlx_sqlite_opts(|options| {
            options
                .journal_mode(SqliteJournalMode::Wal)
                .synchronous(SqliteSynchronous::Full)
                .busy_timeout(Duration::from_secs(5))
                .foreign_keys(true)
        })
        .sqlx_logging(false);
    Ok(Database::connect(options).await?)
}

/// 一次历史写事务在放弃之前额外重试的次数（每次都会先等 `busy_timeout`）。
const HISTORY_WRITE_RETRIES: u32 = 2;

/// Begins one history write transaction that takes the write lock up front.
///
/// `BEGIN DEFERRED` only upgrades its read lock when the first write statement runs, and SQLite
/// reports `SQLITE_BUSY` for a failed upgrade *without* invoking the busy handler — so the
/// configured `busy_timeout` cannot serialize two writers that both read before they write. Every
/// history write reads the applied waterline and then writes it in
/// the same transaction, so it must start as a writer: `BEGIN IMMEDIATE` makes the busy handler do
/// the waiting, and a transaction that still finds the lock held past the timeout is retried a
/// bounded number of times before it is reported. A structural failure (constraint, corruption)
/// is never retried.
async fn begin_write(db: &DatabaseConnection) -> Result<sea_orm::DatabaseTransaction> {
    let mut attempt: u32 = 0;
    loop {
        let started = db
            .begin_with_options(TransactionOptions {
                sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
                ..Default::default()
            })
            .await;
        match started {
            Ok(tx) => return Ok(tx),
            Err(error)
                if attempt < HISTORY_WRITE_RETRIES && is_retryable_write(&error.to_string()) =>
            {
                attempt += 1;
                // The busy handler already waited for the lock; a short pause only gives the other
                // writer a chance to commit before this attempt re-acquires it.
                tokio::time::sleep(Duration::from_millis(50 * u64::from(attempt))).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// 只有忙/锁/IO 类错误允许在写者内部有界重试；结构与约束错误必须立刻失败闭锁。
///
/// 与全局 call writer 的判定保持同一集合：一次瞬时 `SQLITE_BUSY`/`SQLITE_LOCKED`/`SQLITE_IOERR`
/// 既不丢事实也不得升级成终态持久化失败，而约束/损坏类错误必须原样上报。
pub(crate) fn is_retryable_write(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("database is locked")
        || message.contains("database table is locked")
        || message.contains("database is busy")
        || message.contains("disk i/o error")
        || message.contains("sqlite_busy")
        || message.contains("sqlite_locked")
        || message.contains("sqlite_ioerr")
}

/// Opens an *existing* history database read-only, without creating or upgrading anything.
///
/// `mode=ro` makes SQLite refuse to create a missing file, and no schema/journal PRAGMA that would
/// mutate the database is issued here; a concurrent WAL writer stays visible through its
/// checkpoint, so cold pagination is neither gated on nor able to disturb the writer.
async fn connect_read_only(path: &Path) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(crate::studio::paths::sqlite_read_only_url(path));
    options
        .max_connections(1)
        .min_connections(1)
        .connect_timeout(Duration::from_secs(8))
        .acquire_timeout(Duration::from_secs(8))
        .map_sqlx_sqlite_opts(|options| {
            // `journal_mode`/`synchronous` are write-side settings; only the read-safe
            // `busy_timeout` (not a PRAGMA) and `foreign_keys` are applied.
            options
                .busy_timeout(Duration::from_secs(5))
                .foreign_keys(true)
        })
        .sqlx_logging(false);
    Database::connect(options).await.with_context(|| {
        format!(
            "history database could not be opened read-only: {}",
            path.display()
        )
    })
}

/// Validates an existing history database's schema, Thread and database identity without writing.
///
/// A missing identity row, an unsupported schema version, a foreign Thread or an empty database id
/// fails closed so a cold read reports the damage instead of adopting or rebuilding the store.
async fn validate_existing(db: &DatabaseConnection, thread_id: &str) -> Result<(String, i64)> {
    let row = db
        .query_one_raw(statement(
            "SELECT schema_version,database_id,thread_id FROM history_meta WHERE id=1",
            vec![],
        ))
        .await
        .context("history database has no readable identity table")?;
    let row = row.context("history database identity row is missing")?;
    let version: i64 = row.try_get("", "schema_version")?;
    ensure!(
        (2..=HISTORY_SCHEMA_VERSION).contains(&version),
        "unsupported history schema {version}; existing data preserved"
    );
    let owner: String = row.try_get("", "thread_id")?;
    ensure!(
        owner == thread_id,
        "history database belongs to another Thread"
    );
    let database_id: String = row.try_get("", "database_id")?;
    ensure!(
        !database_id.is_empty(),
        "history database identity is empty"
    );
    Ok((database_id, version))
}

async fn write_tool_task(
    tx: &impl ConnectionTrait,
    write_seq: i64,
    task: &pl_core::thread::task::TaskRecord,
) -> Result<()> {
    let payload = serde_json::to_string(task)?;
    let existing = tx
        .query_one_raw(statement(
            "SELECT revision,task_payload FROM history_tool_tasks WHERE call_id=?",
            vec![task.call_id.clone().into()],
        ))
        .await?;
    if let Some(existing) = existing {
        let revision: i64 = existing.try_get("", "revision")?;
        if revision >= integer(task.revision)? {
            if revision == integer(task.revision)? {
                let stored: String = existing.try_get("", "task_payload")?;
                ensure!(
                    stored == payload,
                    "tool task {} conflicts with committed revision",
                    task.id
                );
            }
            return Ok(());
        }
    }
    tx.execute_raw(statement(
        "INSERT INTO history_tool_tasks(call_id,revision,task_payload,delivery_payload,last_write_seq)
         VALUES(?,?,?,NULL,?) ON CONFLICT(call_id) DO UPDATE SET
         revision=excluded.revision,task_payload=excluded.task_payload,
         last_write_seq=excluded.last_write_seq",
        vec![task.call_id.clone().into(), integer(task.revision)?.into(), payload.into(), write_seq.into()],
    ))
    .await?;
    Ok(())
}

async fn write_tool_delivery(
    tx: &impl ConnectionTrait,
    write_seq: i64,
    delivery: &pl_core::thread::ToolDelivery,
) -> Result<()> {
    let payload = serde_json::to_string(delivery)?;
    let existing = tx
        .query_one_raw(statement(
            "SELECT task_payload,delivery_payload FROM history_tool_tasks WHERE call_id=?",
            vec![delivery.call_id.clone().into()],
        ))
        .await?
        .with_context(|| format!("tool delivery {} has no admitted task", delivery.call_id))?;
    if let Some(stored) = existing.try_get::<Option<String>>("", "delivery_payload")? {
        ensure!(
            stored == payload,
            "tool delivery {} conflicts with committed body",
            delivery.call_id
        );
        return Ok(());
    }
    let task_payload: String = existing.try_get("", "task_payload")?;
    let mut task: pl_core::thread::task::TaskRecord = serde_json::from_str(&task_payload)?;
    if task.status == pl_core::thread::task::TaskStatus::Running {
        task.status = match delivery.outcome {
            pl_core::thread::ToolOutcome::Succeeded => pl_core::thread::task::TaskStatus::Succeeded,
            pl_core::thread::ToolOutcome::Failed(_) => pl_core::thread::task::TaskStatus::Failed,
            pl_core::thread::ToolOutcome::Cancelled => pl_core::thread::task::TaskStatus::Cancelled,
            pl_core::thread::ToolOutcome::Interrupted => {
                pl_core::thread::task::TaskStatus::Interrupted
            }
        };
        task.revision = task
            .revision
            .checked_add(1)
            .context("tool task revision exhausted")?;
        if task.status == pl_core::thread::task::TaskStatus::Cancelled {
            task.cancel_requested = true;
        }
    }
    tx.execute_raw(statement(
        "UPDATE history_tool_tasks SET revision=?,task_payload=?,delivery_payload=?,last_write_seq=? WHERE call_id=?",
        vec![integer(task.revision)?.into(), serde_json::to_string(&task)?.into(), payload.into(), write_seq.into(), delivery.call_id.clone().into()],
    ))
    .await?;
    Ok(())
}

/// Current durable write sequence of one already-open connection.
async fn connection_watermark(connection: &HistoryConnection) -> Result<u64> {
    u64::try_from(applied_write_seq(&connection.db).await?)
        .context("negative history write sequence")
}

/// 构造绑定本数据库当前位置的版本化游标。
fn cursor(
    connection: &HistoryConnection,
    thread_id: &str,
    item: &ThreadItem,
    watermark: u64,
) -> String {
    TimelineCursor::new(
        thread_id.to_owned(),
        connection.database_id.clone(),
        item.ordinal,
        item.id.clone(),
        watermark,
    )
    .encode()
}

/// Turn 页的续传位置。
///
/// `after_ordinal` 是本页最后返回的条目 ordinal，`end_ordinal` 是这条 Turn 的 durable 结尾；
/// 两者相等表示这条 Turn 已完整返回，下一页从更旧的 Turn 继续。
#[derive(Debug, Clone)]
struct TurnPageResume {
    turn_id: String,
    after_ordinal: i64,
    end_ordinal: i64,
}

/// 一条 Turn 在一个 Turn 页里的条目窗口。
struct TurnPageEntry {
    history: pl_protocol::ThreadTurnHistory,
    /// 窗口内最后一条条目的 ordinal；窗口为空时等于 Turn 结尾。
    after_ordinal: i64,
    /// 这条 Turn 是否还有未返回的条目。
    more: bool,
    /// 本条 entry 计入整页预算的序列化字节（与 item 页同一计量方式）。
    bytes: usize,
}

/// 构造 Turn 页的版本化游标：绑定 Thread、数据库身份、水位与 Turn 内的续传位置。
///
/// 复用 [`TimelineCursor`] 的自校验封装（ordinal 位置、`item_id` 位置存 Turn id），因此数据库
/// 重建、跨 Thread 或水位回退的游标都会被拒绝，而不是被当成“附近某条”静默沿用。
fn turn_page_cursor(
    connection: &HistoryConnection,
    thread_id: &str,
    turn_id: &str,
    after_ordinal: i64,
    watermark: u64,
) -> Result<String> {
    Ok(TimelineCursor::new(
        thread_id.to_owned(),
        connection.database_id.clone(),
        u64::try_from(after_ordinal)?,
        turn_id.to_owned(),
        watermark,
    )
    .encode())
}

/// 解析 Turn 页游标，返回它在本数据库内的续传位置。
///
/// 版本化 token 是唯一的合法输入：它逐项校验 Thread、数据库身份、水位，并要求游标落在该 Turn
/// 自己的条目上。任何非版本化输入（例如旧版原始 `turn_id`）都无法在数据库被替换后绑定到本库的
/// 身份与水位——同名 Turn 仍然存在时静默沿用等于把游标当成本库“附近某条”——因此显式失败，由调用
/// 方重新加载窗口，而不是回退兼容或猜测位置。
async fn turn_page_resume(
    connection: &HistoryConnection,
    thread_id: &str,
    cursor: &str,
    watermark: u64,
) -> Result<TurnPageResume> {
    let parsed = TimelineCursor::decode(cursor)
        .context("Turn cursor is not a versioned history cursor; reload the window")?;
    ensure!(
        parsed.thread_id == thread_id,
        "timeline cursor belongs to another Thread"
    );
    ensure!(
        parsed.database_id == connection.database_id,
        "timeline cursor belongs to a rebuilt history database; reload the window"
    );
    ensure!(
        parsed.applied_write_sequence <= watermark,
        "timeline cursor was read from a newer history than this database"
    );
    let row = connection
        .db
        .query_one_raw(statement(
            "SELECT turn_id,last_ordinal FROM history_turns WHERE turn_id=?",
            vec![parsed.item_id.clone().into()],
        ))
        .await?
        .context("timeline cursor Turn no longer exists; reload the window")?;
    let stored_turn: String = row.try_get("", "turn_id")?;
    ensure!(
        stored_turn == parsed.item_id,
        "timeline cursor Turn identity changed; reload the window"
    );
    let end_ordinal: i64 = row.try_get("", "last_ordinal")?;
    let after_ordinal = i64::try_from(parsed.ordinal).context("timeline cursor is out of range")?;
    ensure!(
        after_ordinal <= end_ordinal,
        "timeline cursor is ahead of the durable Turn end; reload the window"
    );
    // 游标必须落在该 Turn 自己的条目上：位置被改写时明确失败，绝不猜一个相邻位置。
    let anchor = connection
        .db
        .query_one_raw(statement(
            "SELECT turn_id FROM history_items WHERE ordinal=?",
            vec![after_ordinal.into()],
        ))
        .await?
        .context("timeline cursor item no longer exists; reload the window")?;
    let anchor_turn: String = anchor.try_get("", "turn_id")?;
    ensure!(
        anchor_turn == parsed.item_id,
        "timeline cursor item identity changed; reload the window"
    );
    Ok(TurnPageResume {
        turn_id: parsed.item_id,
        after_ordinal,
        end_ordinal,
    })
}

/// 取一条 Turn 在 `after_ordinal` 之后的条目窗口。
///
/// 条目按 ordinal 升序、连续前缀返回，受 [`TURN_PAGE_ITEM_LIMIT`] 行与 `budget` 字节约束；
/// 窗口为空时返回 `None`（这条 Turn 没有条目，通常意味着历史被外部改写）。至少返回一条条目，
/// 否则超大条目的截断点无法让游标前进。
async fn turn_page_entry(
    connection: &HistoryConnection,
    turn_id: &str,
    after_ordinal: i64,
    budget: usize,
) -> Result<Option<TurnPageEntry>> {
    let Some(row) = connection
        .db
        .query_one_raw(statement(
            "SELECT last_ordinal,payload FROM history_turns WHERE turn_id=?",
            vec![turn_id.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let end_ordinal: i64 = row.try_get("", "last_ordinal")?;
    let turn: TimelineTurn = serde_json::from_str(&row.try_get::<String>("", "payload")?)?;
    // 多取一行探测这条 Turn 是否还有更晚的条目。
    let rows = query_items(
        &connection.db,
        "SELECT payload FROM history_items
             WHERE turn_id=? AND ordinal > ? ORDER BY ordinal ASC LIMIT ?",
        vec![
            turn_id.into(),
            after_ordinal.into(),
            integer(TURN_PAGE_ITEM_LIMIT + 1)?.into(),
        ],
    )
    .await?;
    let mut more = rows.len() > TURN_PAGE_ITEM_LIMIT;
    let mut used = serde_json::to_string(&turn)?.len();
    let mut items = Vec::new();
    for item in rows.into_iter().take(TURN_PAGE_ITEM_LIMIT) {
        let size = serde_json::to_string(&item)?.len();
        if !items.is_empty() && used.saturating_add(size) > budget {
            more = true;
            break;
        }
        used = used.saturating_add(size);
        items.push(item);
    }
    let after_ordinal = match items.last() {
        Some(item) => integer(item.ordinal)?,
        // 这条 Turn 没有可返回的条目：直接标记为已完整返回，避免游标停在原地打转。
        None => end_ordinal,
    };
    Ok(Some(TurnPageEntry {
        history: pl_protocol::ThreadTurnHistory {
            turn: turn.turn,
            items,
            context_disposition: turn.context_disposition,
        },
        after_ordinal,
        more,
        bytes: used,
    }))
}

async fn query_items(
    db: &DatabaseConnection,
    sql: &str,
    values: Vec<Value>,
) -> Result<Vec<ThreadItem>> {
    db.query_all_raw(statement(sql, values))
        .await?
        .into_iter()
        .map(|row| {
            let payload: String = row.try_get("", "payload")?;
            Ok(serde_json::from_str(&payload)?)
        })
        .collect()
}

async fn query_chat_ids(
    db: &DatabaseConnection,
    sql: &str,
    values: Vec<Value>,
) -> Result<Vec<(u64, String)>> {
    db.query_all_raw(statement(sql, values))
        .await?
        .into_iter()
        .map(|row| {
            Ok((
                u64::try_from(row.try_get::<i64>("", "ordinal")?)?,
                row.try_get("", "item_id")?,
            ))
        })
        .collect()
}

async fn ordinal(connection: &HistoryConnection, item_id: &str) -> Result<i64> {
    connection
        .db
        .query_one_raw(statement(
            "SELECT ordinal FROM history_items WHERE item_id=?",
            vec![item_id.into()],
        ))
        .await?
        .context("timeline cursor does not belong to this Thread")?
        .try_get("", "ordinal")
        .map_err(Into::into)
}

async fn exists_before(connection: &HistoryConnection, ordinal: u64) -> Result<bool> {
    exists(connection, "ordinal < ?", ordinal).await
}

async fn exists_after(connection: &HistoryConnection, ordinal: u64) -> Result<bool> {
    exists(connection, "ordinal > ?", ordinal).await
}

async fn exists(connection: &HistoryConnection, predicate: &str, ordinal: u64) -> Result<bool> {
    let row = connection
        .db
        .query_one_raw(statement(
            &format!("SELECT 1 AS present FROM history_items WHERE {predicate} LIMIT 1"),
            vec![integer(ordinal)?.into()],
        ))
        .await?;
    Ok(row.is_some())
}

async fn turns_for(
    connection: &HistoryConnection,
    items: &[ThreadItem],
) -> Result<Vec<TimelineTurn>> {
    let ids = items
        .iter()
        .filter_map(|item| (!item.turn_id.is_empty()).then_some(item.turn_id.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    let mut turns = Vec::new();
    for id in ids {
        let row = connection
            .db
            .query_one_raw(statement(
                "SELECT payload FROM history_turns WHERE turn_id=?",
                vec![id.into()],
            ))
            .await?;
        if let Some(row) = row {
            let payload: String = row.try_get("", "payload")?;
            turns.push(serde_json::from_str(&payload)?);
        }
    }
    Ok(turns)
}

/// Creates or validates the schema and returns the owning database identity.
async fn initialize(db: &DatabaseConnection, thread_id: &str) -> Result<String> {
    db.execute_unprepared(
        "CREATE TABLE IF NOT EXISTS history_meta (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            schema_version INTEGER NOT NULL,
            database_id TEXT NOT NULL,
            thread_id TEXT NOT NULL,
            applied_write_seq INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS history_items (
            ordinal INTEGER PRIMARY KEY,
            item_id TEXT NOT NULL UNIQUE,
            turn_id TEXT NOT NULL,
            kind TEXT NOT NULL,
            revision INTEGER NOT NULL,
            lifecycle TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            payload TEXT NOT NULL CHECK (json_valid(payload)),
            last_write_seq INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS history_items_by_turn
            ON history_items(turn_id, ordinal);
        -- 终态 Turn 定位（`latest_terminal_turn`/`terminal_turns_after`）按 kind/lifecycle 过滤再
        -- 按 ordinal 排序取数；覆盖索引让空库与稀疏历史都不做全表逆扫+排序。
        CREATE INDEX IF NOT EXISTS history_items_by_kind_lifecycle
            ON history_items(kind, lifecycle, ordinal);
        -- `agent_page` 的 text-only 稀疏过滤同样带 ordinal 方向，单独一条 (kind, ordinal) 索引
        -- 才能同时满足过滤与排序。
        CREATE INDEX IF NOT EXISTS history_items_by_kind
            ON history_items(kind, ordinal);
        CREATE TABLE IF NOT EXISTS history_turns (
            turn_id TEXT PRIMARY KEY,
            first_ordinal INTEGER NOT NULL,
            last_ordinal INTEGER NOT NULL,
            revision INTEGER NOT NULL,
            payload TEXT NOT NULL CHECK (json_valid(payload)),
            last_write_seq INTEGER NOT NULL
        );
        -- Turn 页是 `last_ordinal` 上的 keyset 倒序扫描；没有这条索引时深翻页会退化成全表
        -- 扫描 + 排序。
        CREATE INDEX IF NOT EXISTS history_turns_by_last_ordinal
            ON history_turns(last_ordinal);
        CREATE TABLE IF NOT EXISTS history_input_identities (
            item_id TEXT PRIMARY KEY,
            ordinal INTEGER NOT NULL,
            revision INTEGER NOT NULL,
            digest TEXT NOT NULL,
            request_digest TEXT,
            presentation TEXT CHECK (presentation IN ('visible','hidden')),
            payload TEXT NOT NULL CHECK (json_valid(payload)),
            last_write_seq INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS history_fact_receipts (
            item_id TEXT NOT NULL,
            revision INTEGER NOT NULL,
            kind TEXT NOT NULL,
            digest TEXT NOT NULL,
            payload TEXT NOT NULL CHECK (json_valid(payload)),
            last_write_seq INTEGER NOT NULL,
            PRIMARY KEY(item_id, revision)
        );
        -- 已受理消息的最小身份索引：原始消息 id、时间线条目身份、受理序号与正文摘要。
        -- `digest` 可空，只用于迁移回填留下的“身份已知、正文不可验证”行（读取方 fail-closed）。
        CREATE TABLE IF NOT EXISTS history_message_identities (
            message_id TEXT PRIMARY KEY,
            item_id TEXT NOT NULL,
            sequence INTEGER NOT NULL,
            digest TEXT,
            last_write_seq INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS history_tool_tasks (
            call_id TEXT PRIMARY KEY,
            revision INTEGER NOT NULL,
            task_payload TEXT NOT NULL CHECK (json_valid(task_payload)),
            delivery_payload TEXT CHECK (delivery_payload IS NULL OR json_valid(delivery_payload)),
            last_write_seq INTEGER NOT NULL
        );",
    )
    .await?;
    // 幂等身份索引的 host 提交摘要列是加法扩展：已存在的索引保留原行，只补上可空的列。
    if let Err(error) = db
        .execute_unprepared("ALTER TABLE history_input_identities ADD COLUMN request_digest TEXT")
        .await
    {
        ensure!(
            error.to_string().contains("duplicate column name"),
            "history identity index cannot add its request digest column: {error}"
        );
    }
    let row = db
        .query_one_raw(statement(
            "SELECT schema_version,database_id,thread_id FROM history_meta WHERE id=1",
            vec![],
        ))
        .await?;
    match row {
        Some(row) => {
            let version: i64 = row.try_get("", "schema_version")?;
            ensure!(
                (1..=HISTORY_SCHEMA_VERSION).contains(&version),
                "unsupported history schema {version}; existing data preserved"
            );
            let owner: String = row.try_get("", "thread_id")?;
            ensure!(
                owner == thread_id,
                "history database belongs to another Thread"
            );
            let database_id: String = row.try_get("", "database_id")?;
            ensure!(
                !database_id.is_empty(),
                "history database identity is empty"
            );
            if version < HISTORY_SCHEMA_VERSION {
                let tx = begin_write(db).await?;
                if version < 3 {
                    if let Err(error) = tx
                        .execute_unprepared(
                            "ALTER TABLE history_input_identities ADD COLUMN presentation TEXT \
                         CHECK (presentation IN ('visible','hidden'))",
                        )
                        .await
                    {
                        ensure!(
                            error.to_string().contains("duplicate column name"),
                            "history identity index cannot add presentation: {error}"
                        );
                    }
                    backfill_legacy_hidden_inputs(&tx).await?;
                }
                tx.execute_raw(statement(
                    "UPDATE history_meta SET schema_version=? WHERE id=1 AND schema_version=?",
                    vec![HISTORY_SCHEMA_VERSION.into(), version.into()],
                ))
                .await?;
                tx.commit().await?;
            }
            Ok(database_id)
        }
        None => {
            let database_id = crate::studio::new_id("history-db");
            db.execute_raw(statement(
                "INSERT INTO history_meta(
                    id,schema_version,database_id,thread_id,applied_write_seq
                 ) VALUES(1,?,?,?,0)",
                vec![
                    HISTORY_SCHEMA_VERSION.into(),
                    database_id.clone().into(),
                    thread_id.to_owned().into(),
                ],
            ))
            .await?;
            Ok(database_id)
        }
    }
}

/// Legacy v2 rows have no presentation. Only a resolved interaction receipt that names the
/// continuation and records a hidden product option proves why its timeline row is absent.
async fn backfill_legacy_hidden_inputs(db: &impl ConnectionTrait) -> Result<()> {
    use pl_core::thread::interactions::{InteractionRecord, InteractionState};
    let mut cursor = 0_i64;
    loop {
        let rows = db
            .query_all_raw(statement(
                "SELECT rowid,payload FROM history_fact_receipts \
             WHERE kind='interaction' AND rowid>? ORDER BY rowid LIMIT 100",
                vec![cursor.into()],
            ))
            .await?;
        if rows.is_empty() {
            break;
        }
        for row in rows {
            cursor = row.try_get("", "rowid")?;
            let record: InteractionRecord =
                serde_json::from_str(&row.try_get::<String>("", "payload")?)
                    .context("legacy interaction receipt cannot be decoded")?;
            let Some(id) = record.continuation_id.as_deref() else {
                continue;
            };
            ensure!(
                id == format!("interaction:{}:continuation", record.request.id),
                "legacy interaction continuation identity conflicts with its receipt"
            );
            if !matches!(record.state, InteractionState::Resolved(_))
                || record.request.payload.version() != 1
            {
                continue;
            }
            let hidden = match record.request.payload.format() {
                "pl.tool.user-input" => true,
                "pl.studio.plan-confirmation" => {
                    let prompt: crate::plan_tool::PlanConfirmationPrompt =
                        serde_json::from_str(record.request.payload.content())
                            .context("legacy Plan confirmation cannot be decoded")?;
                    prompt.presentation == pl_protocol::MessagePresentation::Hidden
                }
                _ => false,
            };
            if !hidden {
                continue;
            }
            let identity = db
                .query_one_raw(statement(
                    "SELECT presentation FROM history_input_identities WHERE item_id=?",
                    vec![id.to_owned().into()],
                ))
                .await?;
            let Some(identity) = identity else {
                continue;
            };
            let presentation: Option<String> = identity.try_get("", "presentation")?;
            ensure!(
                presentation
                    .as_deref()
                    .is_none_or(|value| value == "hidden"),
                "legacy hidden continuation conflicts with its input identity"
            );
            ensure!(
                db.query_one_raw(statement(
                    "SELECT item_id FROM history_items WHERE item_id=?",
                    vec![id.to_owned().into()],
                ))
                .await?
                .is_none(),
                "legacy hidden continuation has a visible timeline item"
            );
            db.execute_raw(statement(
                "UPDATE history_input_identities SET presentation='hidden' WHERE item_id=?",
                vec![id.to_owned().into()],
            ))
            .await?;
        }
    }
    Ok(())
}

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

/// 页字节预算必须保留的那一端。
///
/// 页永远是一个连续的 ordinal 窗口；被预算丢弃的条目留在本页首/尾之外并与返回的首尾相邻，
/// 因此分页真实返回的 cursor 继续滚动时能重新取回它们，不会被跳过。
#[derive(Debug, Clone, Copy)]
enum BudgetEdge {
    /// 保留升序页中最新的一端（`Latest`、`Before`）。
    Newest,
    /// 保留升序页中最旧的一端（`After`）。
    Oldest,
    /// 保留 ordinal 不小于该锚点的第一条（`Around`），并优先从更旧的一侧收缩。
    Around(u64),
}

/// 按总 payload 字节预算收缩一个已按 ordinal 升序排好的页，并保持窗口连续；至少保留一条。
///
/// 请求方向决定丢弃哪一端：`Newest` 页紧贴锚点（丢弃更旧的一端），`Oldest` 页紧贴锚点（丢弃
/// 更新的一端），显式锚点页保留锚点行并从更旧的一侧先收缩。
fn apply_byte_budget(rows: &mut Vec<ThreadItem>, edge: BudgetEdge) -> Result<bool> {
    let sizes = rows
        .iter()
        .map(|item| serde_json::to_string(item).map(|payload| payload.len()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut total = sizes
        .iter()
        .fold(0_usize, |total, bytes| total.saturating_add(*bytes));
    let mut low = 0_usize;
    let mut high = rows.len();
    // 锚点行是升序页里第一条 ordinal 不小于锚点的条目；`Around` 的取数保证它存在，缺失时
    // 退化为保留最新端，绝不猜测位置。
    let anchor = match edge {
        BudgetEdge::Newest | BudgetEdge::Oldest => None,
        BudgetEdge::Around(ordinal) => Some(
            rows.iter()
                .position(|item| item.ordinal >= ordinal)
                .unwrap_or(rows.len()),
        ),
    };
    while total > PAGE_BYTE_BUDGET && high - low > 1 {
        let trim_newer = match edge {
            BudgetEdge::Newest => false,
            BudgetEdge::Oldest => true,
            // 锚点之前（更旧）的部分先收缩；收缩到锚点后改为丢弃锚点之后（更新）的一端。
            BudgetEdge::Around(_) => anchor.is_some_and(|anchor| low >= anchor),
        };
        if trim_newer {
            high -= 1;
            total = total.saturating_sub(sizes[high]);
        } else {
            total = total.saturating_sub(sizes[low]);
            low += 1;
        }
    }
    if low == 0 && high == rows.len() {
        return Ok(false);
    }
    rows.truncate(high);
    rows.drain(..low);
    Ok(true)
}

/// Writes one minimal input identity inside the transaction of the effect that produced it.
///
/// Only the small `(id, digest, delivery, state, ordinal, revision)` record is kept, never the
/// admitted body or its context, so the index grows far more slowly than the timeline and never
/// becomes a second copy of history. It exists so the runtime can answer a repeated submission
/// after the input left core state without loading historical entities back into core.
///
/// A repeated write of the same identity only refreshes its bookkeeping. A different digest for an
/// already-recorded id is an identity conflict: the accepted identity is never silently
/// overwritten, and the conflict fails the whole effect transaction instead.
async fn write_input_identity(
    db: &impl ConnectionTrait,
    write_seq: i64,
    identity: &InputIdentityWrite,
) -> Result<()> {
    let payload = serde_json::to_string(&identity.entry)?;
    let existing = db
        .query_one_raw(statement(
            "SELECT digest,presentation FROM history_input_identities WHERE item_id=?",
            vec![identity.entry.id.clone().into()],
        ))
        .await?;
    if let Some(row) = existing {
        let stored: String = row.try_get("", "digest")?;
        ensure!(
            stored == identity.entry.digest,
            "input identity {} conflicts with its durable record",
            identity.entry.id
        );
        let presentation: Option<String> = row.try_get("", "presentation")?;
        ensure!(
            presentation
                .as_deref()
                .is_none_or(|stored| stored == presentation_label(identity.presentation)),
            "input identity {} conflicts with its durable presentation",
            identity.entry.id
        );
        db.execute_raw(statement(
            "UPDATE history_input_identities
             SET ordinal=?,revision=?,request_digest=?,presentation=?,payload=?,last_write_seq=?
             WHERE item_id=?",
            vec![
                integer(identity.entry.ordinal)?.into(),
                integer(identity.entry.revision)?.into(),
                identity.request_digest.clone().into(),
                presentation_label(identity.presentation).into(),
                payload.into(),
                write_seq.into(),
                identity.entry.id.clone().into(),
            ],
        ))
        .await?;
        return Ok(());
    }
    db.execute_raw(statement(
        "INSERT INTO history_input_identities(
            item_id,ordinal,revision,digest,request_digest,presentation,payload,last_write_seq
         )
         VALUES(?,?,?,?,?,?,?,?)",
        vec![
            identity.entry.id.clone().into(),
            integer(identity.entry.ordinal)?.into(),
            integer(identity.entry.revision)?.into(),
            identity.entry.digest.clone().into(),
            identity.request_digest.clone().into(),
            presentation_label(identity.presentation).into(),
            payload.into(),
            write_seq.into(),
        ],
    ))
    .await?;
    Ok(())
}

fn presentation_label(presentation: pl_protocol::MessagePresentation) -> &'static str {
    match presentation {
        pl_protocol::MessagePresentation::Visible => "visible",
        pl_protocol::MessagePresentation::Hidden => "hidden",
    }
}

/// Reports whether one history database already carries the durable message identity index.
///
/// A database written before that index existed has no such table until the writer creates it on its
/// next commit, so a cold read must report "this identity is unknown" instead of failing (and must
/// never create the table itself). Reading the catalog is a plain read; any failure of it is a real
/// storage failure the caller must surface rather than swallow.
async fn message_identity_index_exists(connection: &HistoryConnection) -> Result<bool> {
    Ok(connection
        .db
        .query_one_raw(statement(
            "SELECT name FROM sqlite_master
             WHERE type='table' AND name='history_message_identities'",
            vec![],
        ))
        .await?
        .is_some())
}

/// Records the minimal durable identity of one message this effect admitted.
///
/// The row shares the effect's transaction, so the receipt can never be durable without the history
/// fact that admitted the message. A repeated write of the same identity with the same digest only
/// refreshes its bookkeeping. A different digest for an already-recorded identity never overwrites
/// the accepted identity: the conflict fails the whole effect transaction. A row that exists without
/// a digest (a migration backfill) is left untouched, so its readers keep failing closed instead of
/// silently replacing an unverifiable accepted identity.
async fn write_message_identity(
    db: &impl ConnectionTrait,
    write_seq: i64,
    identity: &MessageIdentityWrite,
) -> Result<()> {
    ensure!(
        !identity.message_id.is_empty() && !identity.item_id.is_empty(),
        "durable message identity must not be empty"
    );
    let existing = db
        .query_one_raw(statement(
            "SELECT digest FROM history_message_identities WHERE message_id=?",
            vec![identity.message_id.clone().into()],
        ))
        .await?;
    if let Some(row) = existing {
        let stored: Option<String> = row.try_get("", "digest")?;
        match (&stored, &identity.digest) {
            (Some(stored), Some(digest)) => ensure!(
                stored == digest,
                "message identity {} conflicts with its durable record",
                identity.message_id
            ),
            // A backfilled row proves the identity but not the body; it is never overwritten.
            (None, _) => return Ok(()),
            (Some(_), None) => return Ok(()),
        }
        db.execute_raw(statement(
            "UPDATE history_message_identities
             SET item_id=?,sequence=?,digest=?,last_write_seq=?
             WHERE message_id=?",
            vec![
                identity.item_id.clone().into(),
                integer(identity.sequence)?.into(),
                identity.digest.clone().into(),
                write_seq.into(),
                identity.message_id.clone().into(),
            ],
        ))
        .await?;
        return Ok(());
    }
    db.execute_raw(statement(
        "INSERT INTO history_message_identities(
            message_id,item_id,sequence,digest,last_write_seq
         )
         VALUES(?,?,?,?,?)",
        vec![
            identity.message_id.clone().into(),
            identity.item_id.clone().into(),
            integer(identity.sequence)?.into(),
            identity.digest.clone().into(),
            write_seq.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Writes one terminal fact receipt in the same transaction as the effect that produced it.
/// The digest is taken from the canonical source payload (the committed record), so a repeated
/// write of the same `(item_id, revision)` is idempotent while a different payload for an
/// already-recorded revision is an identity conflict that fails the whole effect transaction.
/// Receipts are immutable per revision: a newer revision is a new row, never an overwrite of an
/// older terminal fact.
async fn write_fact_receipt(
    db: &impl ConnectionTrait,
    write_seq: i64,
    receipt: &FactReceiptWrite,
) -> Result<()> {
    ensure!(
        !receipt.item_id.is_empty(),
        "durable fact receipt identity must not be empty"
    );
    let digest = pl_core::context::content_hash(receipt.payload.as_bytes());
    let revision = integer(receipt.revision)?;
    let existing = db
        .query_one_raw(statement(
            "SELECT digest FROM history_fact_receipts WHERE item_id=? AND revision=?",
            vec![receipt.item_id.clone().into(), revision.into()],
        ))
        .await?;
    if let Some(row) = existing {
        let stored: String = row.try_get("", "digest")?;
        ensure!(
            stored == digest,
            "durable fact receipt {}@{} conflicts with its recorded payload",
            receipt.item_id,
            receipt.revision
        );
        db.execute_raw(statement(
            "UPDATE history_fact_receipts SET kind=?,payload=?,last_write_seq=?
             WHERE item_id=? AND revision=?",
            vec![
                receipt.kind.to_owned().into(),
                receipt.payload.clone().into(),
                write_seq.into(),
                receipt.item_id.clone().into(),
                revision.into(),
            ],
        ))
        .await?;
        return Ok(());
    }
    db.execute_raw(statement(
        "INSERT INTO history_fact_receipts(item_id,revision,kind,digest,payload,last_write_seq)
         VALUES(?,?,?,?,?,?)",
        vec![
            receipt.item_id.clone().into(),
            revision.into(),
            receipt.kind.to_owned().into(),
            digest.into(),
            receipt.payload.clone().into(),
            write_seq.into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn applied_write_seq(db: &impl ConnectionTrait) -> Result<i64> {
    Ok(db
        .query_one_raw(statement(
            "SELECT applied_write_seq FROM history_meta WHERE id=1",
            vec![],
        ))
        .await?
        .context("history metadata is missing")?
        .try_get("", "applied_write_seq")?)
}

fn integer(value: impl TryInto<i64>) -> Result<i64> {
    value
        .try_into()
        .map_err(|_| anyhow::anyhow!("history integer exceeds SQLite range"))
}

fn kind_label(kind: ThreadItemKind) -> &'static str {
    match kind {
        ThreadItemKind::Raw => "raw",
        ThreadItemKind::Text => "text",
        ThreadItemKind::Thinking => "thinking",
        ThreadItemKind::Tool => "tool",
        ThreadItemKind::Agent => "agent",
        ThreadItemKind::Turn => "turn",
        ThreadItemKind::Inference => "inference",
        ThreadItemKind::Skill => "skill",
        ThreadItemKind::File => "file",
        ThreadItemKind::ContextCompaction => "contextCompaction",
    }
}

#[cfg(test)]
mod storage_fault_tests {
    use super::*;

    #[tokio::test]
    async fn legacy_plan_continuation_migrates_only_proven_hidden_identity() -> Result<()> {
        use pl_core::{
            context::OpaquePayload,
            thread::{
                input::{InputRecord, InputState, ThreadInput},
                interactions::{
                    InteractionRecord, InteractionRequest, InteractionResponse, InteractionState,
                },
            },
        };
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("history.sqlite");
        let store = HistoryStore::open(&path, "legacy-plan").await?;
        let prompt = crate::plan_tool::PlanConfirmationPrompt {
            question: pl_protocol::UserQuestion {
                id: "plan_confirmation".into(),
                header: "Plan".into(),
                question: "Approved plan".into(),
                is_other: false,
                is_secret: false,
                options: None,
            },
            expected_plan_revision: 1,
            plan_hash: "digest".into(),
            created_at: 1,
            presentation: pl_protocol::MessagePresentation::Hidden,
        };
        let id = "interaction:plan:continuation";
        let interaction = InteractionRecord {
            created_at: 1,
            updated_at: 2,
            continuation_id: Some(id.into()),
            request: InteractionRequest {
                id: "plan".into(),
                turn_id: "first-turn".into(),
                payload: OpaquePayload::new(
                    "pl.studio.plan-confirmation",
                    1,
                    serde_json::to_string(&prompt)?,
                )?,
            },
            revision: 2,
            state: InteractionState::Resolved(InteractionResponse {
                payload: OpaquePayload::text("approved"),
                context: vec![],
            }),
            extension_mutations: vec![],
        };
        let input = InputRecord {
            accepted_sequence: 1,
            delivery: Default::default(),
            input: ThreadInput {
                id: id.into(),
                payload: OpaquePayload::new(
                    "pl.studio.interaction-continuation",
                    1,
                    serde_json::json!({"interactionId":"plan","presentation":"hidden"}).to_string(),
                )?,
                context: vec![],
            },
            ordinal: 1,
            revision: 2,
            state: InputState::Consumed {
                turn_id: "next-turn".into(),
                attempt_id: "attempt".into(),
            },
        };
        let mut unproven = input.clone();
        unproven.input.id = "unproven-input".into();
        unproven.ordinal = 2;
        let identities = [
            InputIdentityWrite {
                entry: crate::studio::storage::state::InputIdentityEntry::new(
                    pl_core::thread::input::input_identity(&input),
                    1,
                ),
                request_digest: None,
                presentation: pl_protocol::MessagePresentation::Hidden,
            },
            InputIdentityWrite {
                entry: crate::studio::storage::state::InputIdentityEntry::new(
                    pl_core::thread::input::input_identity(&unproven),
                    1,
                ),
                request_digest: None,
                presentation: pl_protocol::MessagePresentation::Visible,
            },
        ];
        let receipts = [FactReceiptWrite {
            item_id: "interaction:4:plan".into(),
            revision: 2,
            kind: "interaction",
            payload: serde_json::to_string(&interaction)?,
        }];
        store
            .commit_effect(
                1,
                EffectCommit {
                    items: &[],
                    rolled_back_turns: &Default::default(),
                    identities: &identities,
                    messages: &[],
                    receipts: &receipts,
                    tasks: &[],
                    deliveries: &[],
                    attempt: None,
                },
            )
            .await?;
        let db = Database::connect(crate::studio::paths::sqlite_url(&path)).await?;
        db.execute_unprepared(
            "ALTER TABLE history_input_identities DROP COLUMN presentation; \
             UPDATE history_meta SET schema_version=2",
        )
        .await?;
        drop(db);
        drop(store);

        let migrated = HistoryStore::open(&path, "legacy-plan").await?;
        assert_eq!(migrated.watermark().await?, 1);
        let old = Database::connect(crate::studio::paths::sqlite_url(&path)).await?;
        let version = old
            .query_one_raw(statement(
                "SELECT schema_version FROM history_meta WHERE id=1",
                vec![],
            ))
            .await?
            .context("missing legacy metadata")?
            .try_get::<i64>("", "schema_version")?;
        assert_eq!(version, 2);
        drop(old);
        assert_eq!(
            migrated
                .hidden_input_identities([id.into(), "unproven-input".into()])
                .await?,
            [id.into()].into()
        );
        assert_eq!(
            migrated
                .reader()
                .await?
                .context("missing migrated history")?
                .schema_version,
            3
        );
        assert!(migrated.existing_items([id.into()]).await?.is_empty());
        assert_eq!(migrated.watermark().await?, 1);
        Ok(())
    }

    #[tokio::test]
    async fn session_orders_do_not_write_sqlite_reservations() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("history.sqlite");
        let store = HistoryStore::open(&path, "local-orders").await?;
        let session = pl_core::chat::Session::new(store.clone());
        assert_eq!(session.reserve_order("input-1").await?, 1);
        assert_eq!(session.reserve_order("input-1").await?, 1);
        assert_eq!(session.reserve_order("input-2").await?, 2);
        assert!(!tokio::fs::try_exists(&path).await?);

        let mut item = ThreadItem::completed_user_message(
            "input-1".to_owned(),
            "local-orders".to_owned(),
            "turn-1".to_owned(),
            "hello".to_owned(),
            vec![],
            1,
        );
        item.ordinal = 1;
        item.revision = 1;
        store.commit(1, &[item], &[]).await?;
        let row = store
            .reader()
            .await?
            .context("missing committed history")?
            .db
            .query_one_raw(statement(
                "SELECT COUNT(*) AS count FROM sqlite_master
                 WHERE type='table' AND name='history_ordinals'",
                vec![],
            ))
            .await?
            .context("missing schema query")?;
        assert_eq!(row.try_get::<i64>("", "count")?, 0);
        let mut missing_order = ThreadItem::completed_user_message(
            "missing-order".to_owned(),
            "local-orders".to_owned(),
            "turn-1".to_owned(),
            "not allocated".to_owned(),
            vec![],
            2,
        );
        missing_order.revision = 2;
        assert!(store.commit(2, &[missing_order], &[]).await.is_err());
        assert_eq!(store.watermark().await?, 1);
        let mut changed_order = store.read_item("input-1").await?.item;
        changed_order.ordinal = 2;
        changed_order.revision = 2;
        assert!(store.commit(2, &[changed_order], &[]).await.is_err());
        assert_eq!(store.watermark().await?, 1);
        assert_eq!(session.reserve_order("input-3").await?, 3);

        drop(session);
        let reopened =
            pl_core::chat::Session::new(HistoryStore::open(&path, "local-orders").await?);
        // Uncommitted order 2 was never durable and is not reconstructed after restart.
        assert_eq!(reopened.reserve_order("input-4").await?, 2);
        Ok(())
    }

    #[tokio::test]
    async fn latest_window_only_keeps_one_hundred_committed_items() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store =
            HistoryStore::open(&directory.path().join("history.sqlite"), "paged-thread").await?;
        let items = (0..120)
            .map(|index| {
                let mut item = ThreadItem::completed_user_message(
                    format!("message-{index}"),
                    "paged-thread".to_owned(),
                    "turn-1".to_owned(),
                    "hello".to_owned(),
                    vec![],
                    1,
                );
                item.ordinal = index + 1;
                item.revision = 1;
                item
            })
            .collect::<Vec<_>>();
        store.commit(1, &items, &[]).await?;
        let page = store.page(&TimelineQuery::Latest, 100).await?;
        assert_eq!(page.items.len(), 100);
        assert_eq!(page.first_item_id.as_deref(), Some("message-20"));
        assert_eq!(page.last_item_id.as_deref(), Some("message-119"));
        let before = store
            .page(
                &TimelineQuery::Before {
                    item_id: page.older_cursor.context("missing earlier history")?,
                },
                100,
            )
            .await?;
        assert_eq!(before.items.len(), 20);

        let session = pl_core::chat::Session::new(store.clone());
        let view = session.open_chat(pl_core::chat::ChatFocus::Latest).await?;
        let initial = view.snapshot();
        assert_eq!(initial.items.len(), 32);
        assert_eq!(initial.items.first().unwrap().item_id, "message-88");
        assert_eq!(initial.items.last().unwrap().item_id, "message-119");
        assert!(initial.has_older);
        let older = view.load(pl_core::chat::Direction::Older).await?;
        assert_eq!(older.items.len(), 64);
        assert_eq!(older.items.first().unwrap().item_id, "message-56");
        assert_eq!(older.items.last().unwrap().item_id, "message-119");

        // Only persisted items seed the next Session; a discarded live allocation has no
        // durable reservation to inflate the next ordinal after a restart.
        let reopened =
            HistoryStore::open(&directory.path().join("history.sqlite"), "paged-thread").await?;
        let resumed = pl_core::chat::Session::new(reopened);
        assert_eq!(resumed.reserve_order("message-120").await?, 121);
        let mut committed = ThreadItem::completed_user_message(
            "message-120".to_owned(),
            "paged-thread".to_owned(),
            "turn-1".to_owned(),
            "resumed".to_owned(),
            vec![],
            2,
        );
        committed.ordinal = 121;
        committed.revision = 2;
        store.commit(2, &[committed], &[]).await?;
        let final_session = pl_core::chat::Session::new(
            HistoryStore::open(&directory.path().join("history.sqlite"), "paged-thread").await?,
        );
        assert_eq!(final_session.reserve_order("message-121").await?, 122);
        Ok(())
    }

    async fn save_receipt(store: &HistoryStore, sequence: u64, id: &str) -> Result<()> {
        let receipts = [FactReceiptWrite {
            item_id: id.to_owned(),
            revision: sequence,
            kind: "interaction",
            payload: "{}".to_owned(),
        }];
        store
            .commit_effect(
                sequence,
                EffectCommit {
                    items: &[],
                    rolled_back_turns: &Default::default(),
                    identities: &[],
                    messages: &[],
                    receipts: &receipts,
                    tasks: &[],
                    deliveries: &[],
                    attempt: None,
                },
            )
            .await
    }

    #[tokio::test]
    async fn sqlite_rejection_retains_watermark_and_same_batch_can_retry() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let store =
            HistoryStore::open(&directory.path().join("history.sqlite"), "thread-1").await?;
        save_receipt(&store, 1, "first").await?;
        store
            .writer()
            .await?
            .db
            .execute_unprepared(
                "CREATE TRIGGER reject_receipt BEFORE INSERT ON history_fact_receipts \
             BEGIN SELECT RAISE(ABORT, 'controlled history rejection'); END",
            )
            .await?;
        assert!(save_receipt(&store, 2, "second").await.is_err());
        assert_eq!(store.watermark().await?, 1);
        assert!(store.latest_fact_receipt("second").await?.is_none());
        store
            .writer()
            .await?
            .db
            .execute_unprepared("DROP TRIGGER reject_receipt")
            .await?;
        save_receipt(&store, 2, "second").await?;
        save_receipt(&store, 2, "second").await?;
        assert_eq!(store.watermark().await?, 2);
        assert_eq!(
            store
                .latest_fact_receipt("second")
                .await?
                .context("missing retried receipt")?
                .payload,
            "{}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn task_identity_and_delivery_share_history_authority() -> Result<()> {
        use pl_core::thread::task::{TaskRecord, TaskStatus};
        use pl_core::thread::{ToolDelivery, ToolDeliveryTarget, ToolOutcome};

        let directory = tempfile::tempdir()?;
        let store =
            HistoryStore::open(&directory.path().join("history.sqlite"), "thread-2").await?;
        let tasks = [TaskRecord {
            id: "task:call-1".to_owned(),
            call_id: "call-1".to_owned(),
            turn_id: "turn-1".to_owned(),
            tool_id: "tool-1".to_owned(),
            revision: 1,
            status: TaskStatus::Running,
            cancel_requested: false,
            acknowledgement: None,
        }];
        let empty = std::collections::BTreeSet::new();
        store
            .commit_effect(
                1,
                EffectCommit {
                    items: &[],
                    rolled_back_turns: &empty,
                    identities: &[],
                    messages: &[],
                    receipts: &[],
                    tasks: &tasks,
                    deliveries: &[],
                    attempt: None,
                },
            )
            .await?;
        let delivery = ToolDelivery {
            target: ToolDeliveryTarget::CallResult,
            call_id: "call-1".to_owned(),
            tool_id: "tool-1".to_owned(),
            output: pl_core::tool::ToolOutput::new(
                pl_core::context::OpaquePayload::text("done"),
                vec![],
            ),
            delivered_context: vec![],
            outcome: ToolOutcome::Succeeded,
        };
        store
            .commit_effect(
                2,
                EffectCommit {
                    items: &[],
                    rolled_back_turns: &empty,
                    identities: &[],
                    messages: &[],
                    receipts: &[],
                    tasks: &[],
                    deliveries: &[delivery],
                    attempt: None,
                },
            )
            .await?;
        let result = store
            .tool_task("task:call-1")
            .await?
            .context("missing committed task")?;
        assert_eq!(result.task.status, TaskStatus::Succeeded);
        assert_eq!(
            result.delivery.context("missing delivery")?.call_id,
            "call-1"
        );
        Ok(())
    }
}
