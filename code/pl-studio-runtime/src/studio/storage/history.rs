//! Per-Thread durable timeline storage with keyset pagination.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
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

const HISTORY_SCHEMA_VERSION: i64 = 1;
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
    /// The history database is the single ordinal/identity allocator, so one connection keeps every
    /// batch a short single-writer transaction that cannot interleave its allocations.
    writer: tokio::sync::OnceCell<HistoryConnection>,
    /// A read-only connection to an *existing* database, opened on the first cold read.
    reader: tokio::sync::OnceCell<HistoryConnection>,
}

/// One live connection plus the database identity it validated.
struct HistoryConnection {
    db: DatabaseConnection,
    database_id: String,
}

/// 一条终态输入的最小身份记录，外加可重建的 host 提交摘要。
///
/// `request_digest` 只覆盖 host 提交身份（原始 request 与 presentation），因此重复提交可以在
/// 附件草稿已被消费之后重新计算它并校验正文；载荷无法证明正文身份时为 `None`。
pub(crate) struct InputIdentityWrite {
    pub entry: crate::studio::storage::state::InputIdentityEntry,
    pub request_digest: Option<String>,
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

    /// Whether two handles are the same logical connection state.
    ///
    /// A true result means one writer and one validated database identity, not two database clients:
    /// the effect sink and a live subscriber must see `true` for the same Thread.
    #[cfg(test)]
    pub(crate) fn is_same_handle(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.state, &other.state)
    }

    #[cfg(test)]
    async fn open_memory(thread_id: &str) -> Result<Self> {
        let db = connect("sqlite::memory:".to_owned()).await?;
        let database_id = initialize(&db, thread_id).await?;
        let store = Self::open(Path::new(":memory:"), thread_id).await?;
        // 内存库只能有一个连接：直接把写连接放进写槽，读写都复用它。
        store
            .state
            .writer
            .set(HistoryConnection { db, database_id })
            .ok()
            .context("history test store was already initialized")?;
        Ok(store)
    }

    /// The connection cold reads use, or `None` when no history database exists yet.
    ///
    /// The writer connection, once opened, is authoritative for reads too, so a hot writer and an
    /// in-memory test store never observe two divergent databases. Otherwise the existing file is
    /// opened read-only and validated; a missing file is reported as an empty history and is never
    /// created here, so a cold read leaves an absent store absent.
    async fn reader(&self) -> Result<Option<&HistoryConnection>> {
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
                let database_id = validate_existing(&db, &self.state.thread_id).await?;
                Ok::<HistoryConnection, anyhow::Error>(HistoryConnection { db, database_id })
            })
            .await?;
        Ok(Some(reader))
    }

    /// The single writer connection, created and upgraded on first use.
    async fn writer(&self) -> Result<&HistoryConnection> {
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
                Ok::<HistoryConnection, anyhow::Error>(HistoryConnection { db, database_id })
            })
            .await
    }

    /// Atomically applies one immutable writer batch and advances its fixed waterline.
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
        ensure!(
            write_seq >= current,
            "history write sequence moved backwards"
        );
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
        } = commit;
        ensure!(write_seq > 0, "history write sequence must be positive");
        let write_seq = integer(write_seq)?;
        let tx = begin_write(&self.writer().await?.db).await?;
        let current = applied_write_seq(&tx).await?;
        ensure!(
            write_seq >= current,
            "history write sequence moved backwards"
        );
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
        for id in ids {
            let row = connection
                .db
                .query_one_raw(statement(
                    "SELECT payload FROM history_items WHERE item_id=?",
                    vec![id.clone().into()],
                ))
                .await?;
            if let Some(row) = row {
                let payload: String = row.try_get("", "payload")?;
                items.insert(id, serde_json::from_str(&payload)?);
            }
        }
        Ok(items)
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

    /// Read-only: which of `ids` already hold a durable ordinal reservation.
    ///
    /// This is the shared "this item identity started" fact. The live projection reserves an
    /// ordinal the first time it previews a streaming channel, and the durable writer consults the
    /// same table before it finalizes a failed or cancelled attempt, so neither side fabricates a
    /// terminal item for a channel that never streamed.
    pub(crate) async fn reserved_ordinals(
        &self,
        ids: impl IntoIterator<Item = String>,
    ) -> Result<std::collections::BTreeMap<String, u64>> {
        let Some(connection) = self.reader().await? else {
            return Ok(std::collections::BTreeMap::new());
        };
        let mut reserved = std::collections::BTreeMap::new();
        for id in ids {
            if id.is_empty() {
                continue;
            }
            if let Some(ordinal) = reserved_ordinal(&connection.db, &id).await? {
                reserved.insert(id, ordinal);
            }
        }
        Ok(reserved)
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
            item.ordinal = u64::try_from(old_ordinal)?;
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
            return Ok(());
        }
        if item.ordinal == 0 {
            // 分配事实与实时订阅共用 `history_ordinals`：实时先预留、writer 后写入时复用同一
            // ordinal；writer 先写入时同时留下预留行，后续实时读取得到同一 ordinal。
            item.ordinal = match reserved_ordinal(db, &item.id).await? {
                Some(ordinal) => ordinal,
                None => {
                    let ordinal = next_free_ordinal(db).await?;
                    reserve_ordinal(db, &item.id, ordinal).await?;
                    ordinal
                }
            };
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

    /// Reserves the durable ordinal of every item identity that does not have one yet.
    ///
    /// Live streaming and the history writer allocate through this one table, so a live item and
    /// the durable item it becomes share a single ordinal even though the live frame is emitted
    /// before the effect is written. Repeated reservation of the same identity (multiple
    /// subscribers, window eviction, lagged replay) returns the same ordinal instead of
    /// renumbering it, and the whole batch is one transaction so two subscribers cannot interleave.
    ///
    /// The common steady state is that the writer already finalized the identities, so the durable
    /// table already answers every one of them. That case is served by a pure read: a live frame
    /// never opens a write transaction — and so never contends with the effect commit for the
    /// database write lock — unless it really is the first to allocate one of these identities.
    ///
    /// This whole-read-and-write form is for a caller that owns one handle exclusively (the offline
    /// migration export). A live subscriber instead reads the durable phase on its own read-only
    /// connection and calls [`Self::reserve_missing_ordinals`] on the Thread's single shared writer
    /// handle, so a streaming frame never opens a second SQLite writer.
    pub(crate) async fn reserve_ordinals(
        &self,
        item_ids: impl IntoIterator<Item = String>,
    ) -> Result<std::collections::BTreeMap<String, u64>> {
        let item_ids = item_ids
            .into_iter()
            .filter(|item_id| !item_id.is_empty())
            .collect::<Vec<_>>();
        let mut reserved = self.reserved_ordinals(item_ids.clone()).await?;
        let missing = item_ids
            .into_iter()
            .filter(|item_id| !reserved.contains_key(item_id))
            .collect::<Vec<_>>();
        reserved.extend(self.reserve_missing_ordinals(missing).await?);
        Ok(reserved)
    }

    /// Allocates the durable ordinal of every listed identity that still needs one.
    ///
    /// This is the write half shared by every producer of one Thread: the effect commit and a live
    /// ordinal reservation both call it on the *same* handle, so their allocations are serialized
    /// by one ordered writer (its pool holds a single connection) instead of two independent SQLite
    /// writers racing for `history.sqlite`'s write lock. An identity that was allocated meanwhile
    /// keeps the ordinal it already got, so re-reservation never renumbers it.
    pub(crate) async fn reserve_missing_ordinals(
        &self,
        item_ids: impl IntoIterator<Item = String>,
    ) -> Result<std::collections::BTreeMap<String, u64>> {
        let item_ids = item_ids
            .into_iter()
            .filter(|item_id| !item_id.is_empty())
            .collect::<Vec<_>>();
        if item_ids.is_empty() {
            return Ok(std::collections::BTreeMap::new());
        }
        let tx = begin_write(&self.writer().await?.db).await?;
        let mut reserved = std::collections::BTreeMap::new();
        for item_id in item_ids {
            // Re-read under the write lock: another producer (or the effect writer) may have
            // allocated this identity since the caller's read.
            let ordinal = match reserved_ordinal(&tx, &item_id).await? {
                Some(ordinal) => ordinal,
                None => {
                    let ordinal = next_free_ordinal(&tx).await?;
                    reserve_ordinal(&tx, &item_id, ordinal).await?;
                    ordinal
                }
            };
            reserved.insert(item_id, ordinal);
        }
        tx.commit().await?;
        Ok(reserved)
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
/// history write reads a row (the applied waterline, an ordinal reservation) and then writes it in
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
async fn validate_existing(db: &DatabaseConnection, thread_id: &str) -> Result<String> {
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
        version == HISTORY_SCHEMA_VERSION,
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
    Ok(database_id)
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
        CREATE TABLE IF NOT EXISTS history_ordinals (
            item_id TEXT PRIMARY KEY,
            ordinal INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS history_input_identities (
            item_id TEXT PRIMARY KEY,
            ordinal INTEGER NOT NULL,
            revision INTEGER NOT NULL,
            digest TEXT NOT NULL,
            request_digest TEXT,
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
                version == HISTORY_SCHEMA_VERSION,
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

fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

/// Ordinal already reserved for one item identity, if any.
async fn reserved_ordinal(db: &impl ConnectionTrait, item_id: &str) -> Result<Option<u64>> {
    let row = db
        .query_one_raw(statement(
            "SELECT ordinal FROM history_ordinals WHERE item_id=?",
            vec![item_id.to_owned().into()],
        ))
        .await?;
    match row {
        Some(row) => Ok(Some(u64::try_from(row.try_get::<i64>("", "ordinal")?)?)),
        None => Ok(None),
    }
}

/// Next ordinal no written item and no live reservation has taken yet.
async fn next_free_ordinal(db: &impl ConnectionTrait) -> Result<u64> {
    let row = db
        .query_one_raw(statement(
            "SELECT MAX(ordinal) AS ordinal FROM (
                 SELECT ordinal FROM history_items
                 UNION ALL
                 SELECT ordinal FROM history_ordinals
             )",
            vec![],
        ))
        .await?
        .context("history ordinal query returned no row")?;
    let current: Option<i64> = row.try_get("", "ordinal")?;
    Ok(u64::try_from(current.unwrap_or(0))? + 1)
}

/// Records one reservation without releasing an existing one for the same identity.
async fn reserve_ordinal(db: &impl ConnectionTrait, item_id: &str, ordinal: u64) -> Result<()> {
    db.execute_raw(statement(
        "INSERT INTO history_ordinals(item_id,ordinal) VALUES(?,?)
         ON CONFLICT(item_id) DO NOTHING",
        vec![item_id.to_owned().into(), integer(ordinal)?.into()],
    ))
    .await?;
    Ok(())
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
            "SELECT digest FROM history_input_identities WHERE item_id=?",
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
        db.execute_raw(statement(
            "UPDATE history_input_identities
             SET ordinal=?,revision=?,request_digest=?,payload=?,last_write_seq=?
             WHERE item_id=?",
            vec![
                integer(identity.entry.ordinal)?.into(),
                integer(identity.entry.revision)?.into(),
                identity.request_digest.clone().into(),
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
            item_id,ordinal,revision,digest,request_digest,payload,last_write_seq
         )
         VALUES(?,?,?,?,?,?,?)",
        vec![
            identity.entry.id.clone().into(),
            integer(identity.entry.ordinal)?.into(),
            integer(identity.entry.revision)?.into(),
            identity.entry.digest.clone().into(),
            identity.request_digest.clone().into(),
            payload.into(),
            write_seq.into(),
        ],
    ))
    .await?;
    Ok(())
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
mod tests {
    use super::*;
    use pl_protocol::{
        ThreadContentLifecycle, ThreadItemState, ThreadRawItem, ThreadRawPayload,
        ThreadTextChannel, ThreadTextItem,
    };
    use pretty_assertions::assert_eq;

    fn item(ordinal: u64, revision: u64, terminal: bool) -> ThreadItem {
        ThreadItem::new(
            format!("item-{ordinal}"),
            "thread".into(),
            "turn".into(),
            ordinal,
            revision,
            ordinal as i64,
            ordinal as i64 + revision as i64,
            ThreadItemState::Text(ThreadTextItem::new(
                ThreadTextChannel::Final,
                format!("line {ordinal} revision {revision}"),
                Vec::new(),
                if terminal {
                    ThreadContentLifecycle::completed(ordinal as i64 + 1)
                } else {
                    ThreadContentLifecycle::streaming()
                },
            )),
        )
    }

    /// 4 KiB 一段正文：序列化后不会再被单条预览预算截断。
    fn chunk(index: u64) -> String {
        format!("{index:04}").repeat(1024)
    }

    /// 一段一段大正文构成的条目；`chunks` 决定整条序列化大小。
    fn raw_item(ordinal: u64, chunks: u64) -> ThreadItem {
        ThreadItem::new(
            format!("item-{ordinal}"),
            "thread".into(),
            "turn".into(),
            ordinal,
            1,
            ordinal as i64,
            ordinal as i64,
            ThreadItemState::Raw(ThreadRawItem {
                payloads: (0..chunks)
                    .map(|index| ThreadRawPayload {
                        format: "test/chunk".into(),
                        version: 1,
                        content: chunk(index),
                    })
                    .collect(),
                notice: "bulk payload".into(),
                recorded_at: ordinal as i64,
            }),
        )
    }

    /// 页内大条目：单条远低于单条预览预算，但一页多条合计超过整页字节预算。
    fn large_item(ordinal: u64) -> ThreadItem {
        raw_item(ordinal, 8)
    }

    /// 单条超过单条预览预算：页里只能以同身份预览 + 引用出现。
    fn oversized_item(ordinal: u64) -> ThreadItem {
        raw_item(ordinal, 96)
    }

    /// 一页必须覆盖一个连续 ordinal 窗口，首尾之间不允许有洞。
    fn assert_contiguous(page: &TimelinePage) {
        let first = page.items.first().expect("页至少有首条").ordinal;
        let last = page.items.last().expect("页至少有尾条").ordinal;
        assert_eq!(
            first + page.items.len() as u64 - 1,
            last,
            "分页返回的窗口必须连续"
        );
        assert_eq!(
            page.first_item_id.as_deref(),
            page.items.first().map(|item| item.id.as_str())
        );
        assert_eq!(
            page.last_item_id.as_deref(),
            page.items.last().map(|item| item.id.as_str())
        );
    }

    #[test]
    fn byte_budget_trims_the_far_end_of_each_request_direction() {
        // 每条约 0.8 MiB：2 MiB 预算恰好保留两条，收缩方向因此可以精确断言。
        let rows = |ordinals: &[u64], chunks: u64| {
            ordinals
                .iter()
                .map(|ordinal| raw_item(*ordinal, chunks))
                .collect::<Vec<_>>()
        };
        let ordinals =
            |rows: &[ThreadItem]| rows.iter().map(|item| item.ordinal).collect::<Vec<_>>();

        let mut newest = rows(&[1, 2, 3, 4], 200);
        assert!(apply_byte_budget(&mut newest, BudgetEdge::Newest).unwrap());
        assert_eq!(ordinals(&newest), vec![3, 4], "向更旧滚动时窗口紧贴最新端");

        let mut oldest = rows(&[1, 2, 3, 4], 200);
        assert!(apply_byte_budget(&mut oldest, BudgetEdge::Oldest).unwrap());
        assert_eq!(ordinals(&oldest), vec![1, 2], "向更新滚动时窗口紧贴最旧端");

        let mut anchor_middle = rows(&[1, 2, 3, 4], 200);
        assert!(apply_byte_budget(&mut anchor_middle, BudgetEdge::Around(2)).unwrap());
        assert_eq!(
            ordinals(&anchor_middle),
            vec![2, 3],
            "锚点页保留锚点及其相邻行，并从更旧的一侧先收缩"
        );

        // 每条约 1.3 MiB：预算只够一条，锚点行仍必须活下来（旧实现会保留最旧的一条）。
        let mut anchor_only = rows(&[1, 2, 3, 4], 340);
        assert!(apply_byte_budget(&mut anchor_only, BudgetEdge::Around(4)).unwrap());
        assert_eq!(
            ordinals(&anchor_only),
            vec![4],
            "锚点页绝不丢掉锚点行，即使前面的大条目吃满预算"
        );

        let mut fitting = rows(&[1], 200);
        assert!(!apply_byte_budget(&mut fitting, BudgetEdge::Newest).unwrap());
        assert_eq!(ordinals(&fitting), vec![1]);
    }

    #[tokio::test]
    async fn before_page_stays_adjacent_to_the_anchor_when_the_byte_budget_truncates() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        let items = (1..=140).map(large_item).collect::<Vec<_>>();
        store.commit(1, &items, &[]).await.unwrap();

        let page = store
            .page(
                &TimelineQuery::Before {
                    item_id: "item-140".into(),
                },
                100,
            )
            .await
            .unwrap();
        assert!(page.truncated, "整页字节预算必须真正生效");
        assert!(page.previews.is_empty(), "本例条目未超过单条预览预算");
        // 请求方向是更旧：窗口紧贴锚点，被预算丢弃的是更远（更旧）的一端。
        assert_eq!(page.items.last().unwrap().ordinal, 139);
        assert_contiguous(&page);
        assert!(page.older_cursor.is_some());
        assert!(page.newer_cursor.is_some());

        // 从真实返回的旧游标继续滚动，正好取回紧邻窗口之前的条目，不跳号。
        let older = store
            .page(
                &TimelineQuery::Before {
                    item_id: page.older_cursor.clone().expect("还有更旧的条目"),
                },
                100,
            )
            .await
            .unwrap();
        assert_eq!(
            older.items.last().unwrap().ordinal + 1,
            page.items.first().unwrap().ordinal,
            "补页必须与上一页首条相邻，不能留下被跳过的条目"
        );
    }

    #[tokio::test]
    async fn latest_page_keeps_the_newest_items_when_the_byte_budget_truncates() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        let items = (1..=140).map(large_item).collect::<Vec<_>>();
        store.commit(1, &items, &[]).await.unwrap();

        let page = store.page(&TimelineQuery::Latest, 100).await.unwrap();
        assert!(page.truncated, "整页字节预算必须真正生效");
        assert_eq!(
            page.items.last().unwrap().ordinal,
            140,
            "最新页绝不能因为预算丢掉最新的条目"
        );
        assert_contiguous(&page);
        assert!(page.newer_cursor.is_none(), "最新端没有更新的内容");
        assert!(page.older_cursor.is_some());
    }

    #[tokio::test]
    async fn around_page_keeps_the_requested_anchor_when_preceding_items_consume_the_budget() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        let items = (1..=60).map(oversized_item).collect::<Vec<_>>();
        store.commit(1, &items, &[]).await.unwrap();

        let page = store
            .page(
                &TimelineQuery::Around {
                    item_id: "item-55".into(),
                },
                100,
            )
            .await
            .unwrap();
        assert!(page.truncated, "整页字节预算必须真正生效");
        let anchor = page
            .items
            .iter()
            .find(|item| item.id == "item-55")
            .expect("显式跳转必须能读到锚点本身");
        assert_eq!(anchor.ordinal, 55);
        assert_contiguous(&page);
        // 预览引用与实际返回行严格对齐：谁留在页里，谁就有同身份引用。
        assert_eq!(
            page.previews
                .iter()
                .map(|preview| preview.item_id.as_str())
                .collect::<Vec<_>>(),
            page.items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>()
        );
        assert!(page.older_cursor.is_some());

        // 预览不改变身份：完整正文仍可按 item 身份读回。
        let read = store.read_item("item-55").await.unwrap();
        assert_eq!(read.ordinal, 55);
        assert_eq!(read.item, oversized_item(55));
        assert!(
            serde_json::to_string(&read.item).unwrap().len() > TIMELINE_ITEM_PREVIEW_BYTES,
            "整页里的预览条目其实超过了单条预览预算"
        );
    }

    #[tokio::test]
    async fn oversized_item_page_returns_an_aligned_preview_reference() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        let big = oversized_item(3);
        store
            .commit(1, &[item(1, 1, true), item(2, 1, true), big.clone()], &[])
            .await
            .unwrap();

        let page = store.page(&TimelineQuery::Latest, 2).await.unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.first_item_id.as_deref(), Some("item-2"));
        assert_eq!(page.last_item_id.as_deref(), Some("item-3"));
        assert!(page.older_cursor.is_some(), "更旧的小条目仍可继续滚动");
        assert!(page.newer_cursor.is_none());
        assert!(!page.truncated, "单条预览后的条目尺寸不再触发整页预算截断");
        let preview = page.items.last().unwrap();
        assert_eq!(
            (preview.id.as_str(), preview.ordinal, preview.revision),
            ("item-3", 3, 1)
        );
        assert_eq!(page.previews.len(), 1, "超大条目必须带一条同身份引用");
        let reference = &page.previews[0];
        let full_bytes = serde_json::to_string(&big).unwrap().len() as u64;
        assert_eq!(reference.item_id, "item-3");
        assert_eq!((reference.ordinal, reference.revision), (3, 1));
        assert_eq!(reference.total_bytes, full_bytes);
        assert!(reference.preview_bytes < reference.total_bytes);
        assert_eq!(
            reference.omitted_bytes,
            full_bytes - reference.preview_bytes
        );
    }

    #[tokio::test]
    async fn sqlite_pages_large_history_without_materializing_an_index() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        let items = (1..=1103)
            .map(|ordinal| item(ordinal, 1, true))
            .collect::<Vec<_>>();
        store.commit(1, &items, &[]).await.unwrap();

        let mut query = TimelineQuery::Latest;
        let mut collected = Vec::new();
        loop {
            let page = store.page(&query, 100).await.unwrap();
            let mut previous = page.items;
            previous.extend(collected);
            collected = previous;
            let Some(item_id) = page.older_cursor else {
                break;
            };
            query = TimelineQuery::Before { item_id };
        }
        assert_eq!(collected, items);
    }

    #[tokio::test]
    async fn revisions_are_idempotent_and_terminal_items_reject_late_drafts() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        store.commit(1, &[item(1, 1, false)], &[]).await.unwrap();
        store.commit(1, &[item(1, 1, false)], &[]).await.unwrap();
        store.commit(2, &[item(1, 2, true)], &[]).await.unwrap();
        // A late draft may not reopen a terminal item.
        assert!(store.commit(3, &[item(1, 3, false)], &[]).await.is_err());
        // A newer canonical revision remains the newest committed fact for the same identity.
        store.commit(4, &[item(1, 4, true)], &[]).await.unwrap();
        let page = store.page(&TimelineQuery::Latest, 10).await.unwrap();
        assert_eq!(page.watermark, 4);
        assert_eq!(page.items, vec![item(1, 4, true)]);
    }

    #[tokio::test]
    async fn a_database_rejects_another_thread_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.sqlite");
        let store = HistoryStore::open(&path, "thread").await.unwrap();
        store.commit(1, &[item(1, 1, true)], &[]).await.unwrap();
        // 写者仍持有连接：只读的冷读路径必须能在 WAL 下并发打开同一个库，并校验 Thread 身份。
        let foreign = HistoryStore::open(&path, "other").await.unwrap();
        assert!(foreign.watermark().await.is_err());
        assert!(foreign.page(&TimelineQuery::Latest, 10).await.is_err());
        assert!(foreign.read_item("item-1").await.is_err());
    }

    #[tokio::test]
    async fn cold_query_never_creates_a_missing_history_store() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("thread-store").join("history.sqlite");
        // 只构造句柄：既不建目录，也不建数据库。
        let store = HistoryStore::open(&path, "thread").await.unwrap();
        assert!(!path.exists(), "打开句柄不得创建 history.sqlite");
        assert!(
            !path.parent().unwrap().exists(),
            "打开句柄不得创建 Thread 存储目录"
        );

        // 全部冷读入口都必须返回空事实，而不是把库重建出来。
        let page = store.page(&TimelineQuery::Latest, 20).await.unwrap();
        assert!(page.items.is_empty());
        assert!(page.turns.is_empty() && page.previews.is_empty());
        assert_eq!((page.watermark, page.truncated), (0, false));
        assert!(page.older_cursor.is_none() && page.newer_cursor.is_none());
        assert!(page.first_item_id.is_none() && page.last_item_id.is_none());
        assert_eq!(store.watermark().await.unwrap(), 0);
        assert!(store.turn_page(None, 20).await.unwrap().turns.is_empty());
        assert!(store.input_identity("input-1").await.unwrap().is_none());
        assert!(store.message_identity("message-1").await.unwrap().is_none());
        assert!(
            store
                .existing_items(["item-1".to_owned()])
                .await
                .unwrap()
                .is_empty()
        );
        assert!(store.items_for_turn("turn-1").await.unwrap().is_empty());
        assert!(store.latest_terminal_turn().await.unwrap().is_none());
        assert!(
            store
                .reserved_ordinals(["item-1".to_owned()])
                .await
                .unwrap()
                .is_empty()
        );
        assert!(store.terminal_turns_after(0, 10).await.unwrap().is_empty());
        assert!(
            store
                .agent_page(true, false, None, None, None, 10)
                .await
                .unwrap()
                .2
                .is_empty()
        );
        assert!(store.read_item("item-1").await.is_err());

        // 冷读之后目录与文件依旧不存在：查询没有复活缺失的历史库。
        assert!(!path.exists());
        assert!(!path.parent().unwrap().exists());
    }

    #[tokio::test]
    async fn cold_query_fails_closed_on_a_damaged_history_store_without_modifying_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.sqlite");
        // 既不是 SQLite 头、也不是合法库：读路径必须报错，绝不能重建或"修复"它。
        let damaged = b"history.sqlite is not a sqlite database".to_vec();
        tokio::fs::write(&path, &damaged).await.unwrap();

        let store = HistoryStore::open(&path, "thread").await.unwrap();
        assert!(store.page(&TimelineQuery::Latest, 10).await.is_err());
        assert!(store.watermark().await.is_err());
        assert!(store.turn_page(None, 10).await.is_err());
        assert!(store.read_item("item-1").await.is_err());
        assert!(store.message_identity("message-1").await.is_err());

        // 损坏库的字节必须保持原样：读不是写，也不是重建。
        assert_eq!(tokio::fs::read(&path).await.unwrap(), damaged);
    }

    fn message_identity(id: &str, sequence: u64, digest: Option<&str>) -> MessageIdentityWrite {
        MessageIdentityWrite {
            message_id: id.to_owned(),
            item_id: crate::studio::thread_projection::order::message_id(id),
            sequence,
            digest: digest.map(str::to_owned),
        }
    }

    /// 身份与它的 effect 同事务：命中回原受理序号，同身份异正文让整个事务失败并回滚。
    #[tokio::test]
    async fn durable_message_identity_matches_and_rejects_a_conflicting_body() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        store
            .commit_effect(
                1,
                EffectCommit {
                    items: &[item(1, 1, true)],
                    rolled_back_turns: &std::collections::BTreeSet::new(),
                    identities: &[],
                    messages: &[message_identity("message-a", 1, Some("sha256:aaa"))],
                    receipts: &[],
                },
            )
            .await
            .unwrap();
        let accepted = store.message_identity("message-a").await.unwrap().unwrap();
        assert_eq!(
            (accepted.sequence, accepted.digest.as_deref()),
            (1, Some("sha256:aaa"))
        );

        // 同身份同正文的重复提交幂等：受理序号仍是第一次的，不新增第二条身份。
        store
            .commit_effect(
                2,
                EffectCommit {
                    items: &[],
                    rolled_back_turns: &std::collections::BTreeSet::new(),
                    identities: &[],
                    messages: &[message_identity("message-a", 1, Some("sha256:aaa"))],
                    receipts: &[],
                },
            )
            .await
            .unwrap();
        let repeated = store.message_identity("message-a").await.unwrap().unwrap();
        assert_eq!(
            (repeated.sequence, repeated.digest.as_deref()),
            (1, Some("sha256:aaa"))
        );

        // 同身份不同正文：整个 effect 事务失败，已受理身份与这次提交的时间线条目都不落库。
        let conflict = store
            .commit_effect(
                3,
                EffectCommit {
                    items: &[item(2, 3, true)],
                    rolled_back_turns: &std::collections::BTreeSet::new(),
                    identities: &[],
                    messages: &[message_identity("message-a", 1, Some("sha256:bbb"))],
                    receipts: &[],
                },
            )
            .await;
        let conflict = conflict.expect_err("同一身份的不同正文必须让 effect 事务失败");
        assert!(
            conflict
                .to_string()
                .contains("message identity message-a conflicts"),
            "失败原因必须是身份摘要冲突而不是别的条目错误: {conflict}"
        );
        let unchanged = store.message_identity("message-a").await.unwrap().unwrap();
        assert_eq!(
            (unchanged.sequence, unchanged.digest.as_deref()),
            (1, Some("sha256:aaa"))
        );
        assert!(
            store.read_item("item-2").await.is_err(),
            "冲突事务的时间线条目必须一并回滚"
        );
        assert_eq!(store.watermark().await.unwrap(), 2);
    }

    /// 回填行只证明身份、不证明正文：读取方必须 fail-closed，后来的摘要不得把它升级成已验证。
    #[tokio::test]
    async fn a_backfilled_message_identity_stays_unverifiable() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        store
            .commit_effect(
                1,
                EffectCommit {
                    items: &[],
                    rolled_back_turns: &std::collections::BTreeSet::new(),
                    identities: &[],
                    messages: &[message_identity("message-legacy", 7, None)],
                    receipts: &[],
                },
            )
            .await
            .unwrap();
        let backfilled = store
            .message_identity("message-legacy")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(backfilled.sequence, 7);
        assert!(
            backfilled.digest.is_none(),
            "回填行没有可验证的正文摘要，读取方必须继续 fail-closed"
        );

        store
            .commit_effect(
                2,
                EffectCommit {
                    items: &[],
                    rolled_back_turns: &std::collections::BTreeSet::new(),
                    identities: &[],
                    messages: &[message_identity("message-legacy", 7, Some("sha256:ccc"))],
                    receipts: &[],
                },
            )
            .await
            .unwrap();
        let still_unverifiable = store
            .message_identity("message-legacy")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(still_unverifiable.sequence, 7);
        assert!(
            still_unverifiable.digest.is_none(),
            "不可验证的身份不得被后来的摘要覆盖"
        );
    }

    /// 升级前写入的库还没有身份索引表：冷读报告“身份未知”，既不报错也不替它建表。
    #[tokio::test]
    async fn a_missing_message_identity_index_reads_as_unknown() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        store
            .commit_effect(
                1,
                EffectCommit {
                    items: &[item(1, 1, true)],
                    rolled_back_turns: &std::collections::BTreeSet::new(),
                    identities: &[],
                    messages: &[message_identity("message-a", 1, Some("sha256:aaa"))],
                    receipts: &[],
                },
            )
            .await
            .unwrap();
        store
            .writer()
            .await
            .unwrap()
            .db
            .execute_unprepared("DROP TABLE history_message_identities")
            .await
            .unwrap();

        assert!(store.message_identity("message-a").await.unwrap().is_none());
        let catalog = store
            .writer()
            .await
            .unwrap()
            .db
            .query_all_raw(statement(
                "SELECT name FROM sqlite_master
                 WHERE type='table' AND name='history_message_identities'",
                vec![],
            ))
            .await
            .unwrap();
        assert!(catalog.is_empty(), "冷读不得重建缺失的身份索引表");
    }

    #[tokio::test]
    async fn cold_read_handle_pages_an_existing_history_database() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.sqlite");
        // 写者显式创建并落库（唯一会建库/升级 schema 的路径）。
        let writer = HistoryStore::open(&path, "thread").await.unwrap();
        writer
            .commit(1, &[item(1, 1, true), item(2, 1, true)], &[])
            .await
            .unwrap();
        assert!(path.exists(), "写入必须真正创建 history.sqlite");

        // 冷读句柄只读打开同一个已存在的库：不激活 owner、不建库、分页与按身份读取都可用。
        let reader = HistoryStore::open(&path, "thread").await.unwrap();
        let page = reader.page(&TimelineQuery::Latest, 10).await.unwrap();
        assert_eq!(page.watermark, 1);
        assert_eq!(page.items, vec![item(1, 1, true), item(2, 1, true)]);
        assert!(!page.database_id.is_empty(), "冷读必须报告数据库身份");
        let read = reader.read_item("item-2").await.unwrap();
        assert_eq!((read.ordinal, read.item), (2, item(2, 1, true)));
        assert_eq!(reader.watermark().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn keyset_page_plan_uses_the_ordinal_key_without_materializing_a_sort() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        let items = (1..=32)
            .map(|ordinal| item(ordinal, 1, true))
            .collect::<Vec<_>>();
        store.commit(1, &items, &[]).await.unwrap();

        // 只读句柄上的有界 keyset：按 ordinal 主键范围检索，不物化排序临时 B-Tree。
        let connection = store.reader().await.unwrap().expect("内存库已初始化");
        let rows = connection
            .db
            .query_all_raw(statement(
                "EXPLAIN QUERY PLAN SELECT payload FROM history_items
                 WHERE ordinal < ? ORDER BY ordinal DESC LIMIT ?",
                vec![integer(100).unwrap().into(), integer(10).unwrap().into()],
            ))
            .await
            .unwrap();
        let plan = rows
            .iter()
            .map(|row| row.try_get::<String>("", "detail").unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(plan.contains("INTEGER PRIMARY KEY"), "{plan}");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
    }

    /// 指定 identity 与 Turn 的条目；`chunks` 决定序列化大小（0 表示小正文）。
    fn turn_scoped_item(id: &str, ordinal: u64, turn_id: &str, chunks: u64) -> ThreadItem {
        let state = if chunks == 0 {
            ThreadItemState::Text(ThreadTextItem::new(
                ThreadTextChannel::Final,
                format!("line {ordinal}"),
                Vec::new(),
                ThreadContentLifecycle::completed(ordinal as i64),
            ))
        } else {
            ThreadItemState::Raw(ThreadRawItem {
                payloads: (0..chunks)
                    .map(|index| ThreadRawPayload {
                        format: "test/chunk".into(),
                        version: 1,
                        content: chunk(index),
                    })
                    .collect(),
                notice: "bulk payload".into(),
                recorded_at: ordinal as i64,
            })
        };
        ThreadItem::new(
            id.into(),
            "thread".into(),
            turn_id.into(),
            ordinal,
            1,
            ordinal as i64,
            ordinal as i64,
            state,
        )
    }

    /// 直接落库一条 Turn 行：与 `commit_effect` 的投影一致，first/last 来自已写入的条目。
    async fn publish_turn(store: &HistoryStore, turn_id: &str, last_item_id: &str) {
        let mut turn = pl_protocol::Turn::queued(turn_id, "thread", 1);
        turn.revision = 1;
        let turn = TimelineTurn {
            turn,
            last_item_id: last_item_id.into(),
            context_disposition: pl_protocol::ThreadContextDisposition::Active,
        };
        let writer = store.writer().await.unwrap();
        store.upsert_turn(&writer.db, 1, &turn).await.unwrap();
    }

    /// Turn 页里的条目 identity，按返回顺序。
    fn page_item_ids(page: &pl_protocol::ThreadTurnPage) -> Vec<String> {
        page.turns
            .iter()
            .flat_map(|entry| entry.items.iter().map(|item| item.id.clone()))
            .collect()
    }

    /// Turn 页里的 Turn identity，按返回顺序。
    fn page_turn_ids(page: &pl_protocol::ThreadTurnPage) -> Vec<String> {
        page.turns
            .iter()
            .map(|entry| entry.turn.id.clone())
            .collect()
    }

    /// 手工构造一条 Turn 行 payload（用于 `upsert_turn` 的聚焦测试）。
    fn timeline_turn(
        turn_id: &str,
        last_item_id: &str,
        context_disposition: pl_protocol::ThreadContextDisposition,
    ) -> TimelineTurn {
        let mut turn = pl_protocol::Turn::queued(turn_id, "thread", 1);
        turn.revision = 1;
        TimelineTurn {
            turn,
            last_item_id: last_item_id.into(),
            context_disposition,
        }
    }

    /// 把 `EXPLAIN QUERY PLAN` 的结果拼成一行文本。
    async fn query_plan(connection: &HistoryConnection, sql: &str, values: Vec<Value>) -> String {
        connection
            .db
            .query_all_raw(statement(&format!("EXPLAIN QUERY PLAN {sql}"), values))
            .await
            .unwrap()
            .iter()
            .map(|row| row.try_get::<String>("", "detail").unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Turn 行只允许“同一条事实的投影前进”，同 revision 的重写与其他处置回退都必须失败。
    #[tokio::test]
    async fn a_turn_row_never_replaces_its_fact_at_the_same_revision() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        store
            .commit(1, &[turn_scoped_item("item-a", 1, "turn-a", 0)], &[])
            .await
            .unwrap();
        let writer = store.writer().await.unwrap();
        let turn = timeline_turn(
            "turn-a",
            "item-a",
            pl_protocol::ThreadContextDisposition::Active,
        );
        store.upsert_turn(&writer.db, 2, &turn).await.unwrap();
        // 同 revision 同内容仍然幂等。
        store.upsert_turn(&writer.db, 3, &turn).await.unwrap();

        // 同 revision 但 Turn 本体不同（换了关联输入）：显式冲突，不得用更大的 write_seq 覆盖。
        let mut replaced = timeline_turn(
            "turn-a",
            "item-a",
            pl_protocol::ThreadContextDisposition::Active,
        );
        replaced.turn.input_id = Some("other-input".into());
        let conflict = store
            .upsert_turn(&writer.db, 4, &replaced)
            .await
            .expect_err("同 revision 的 Turn 事实不得被替换");
        assert!(
            conflict.to_string().contains("the Turn fact changed"),
            "{conflict}"
        );

        // 同 revision 的正常投影前进：结尾向后延伸仍然允许，条目仍然可读。
        store
            .commit(2, &[turn_scoped_item("item-b", 2, "turn-a", 0)], &[])
            .await
            .unwrap();
        store
            .upsert_turn(
                &writer.db,
                5,
                &timeline_turn(
                    "turn-a",
                    "item-b",
                    pl_protocol::ThreadContextDisposition::Active,
                ),
            )
            .await
            .unwrap();
        let page = store.turn_page(None, 10).await.unwrap();
        assert_eq!(page_turn_ids(&page), vec!["turn-a"]);
        assert_eq!(page_item_ids(&page), vec!["item-a", "item-b"]);

        // 处置只能由 Active 落定为 RolledBack，不得复活成 Active。
        store
            .upsert_turn(
                &writer.db,
                6,
                &timeline_turn(
                    "turn-a",
                    "item-b",
                    pl_protocol::ThreadContextDisposition::RolledBack,
                ),
            )
            .await
            .unwrap();
        let regressed = store
            .upsert_turn(
                &writer.db,
                7,
                &timeline_turn(
                    "turn-a",
                    "item-b",
                    pl_protocol::ThreadContextDisposition::Active,
                ),
            )
            .await
            .expect_err("rolled back 的 Turn 不得复活成 Active");
        assert!(
            regressed.to_string().contains("disposition regressed"),
            "{regressed}"
        );
    }

    /// 字节预算在 Turn 内部用尽时，游标停在真实返回的条目上，下一页接着取回剩余条目。
    #[tokio::test]
    async fn a_turn_page_is_a_bounded_keyset_that_never_skips_a_turn_or_an_item() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        // turn-1 的三条大条目合计超过整页预算（每条约 0.8 MiB），其余 Turn 只有一个很小的条目。
        let mut items = vec![
            turn_scoped_item("bulk-1", 1, "turn-1", 200),
            turn_scoped_item("bulk-2", 2, "turn-1", 200),
            turn_scoped_item("bulk-3", 3, "turn-1", 200),
        ];
        for (offset, turn) in (2..=5u64).enumerate() {
            let ordinal = 4 + offset as u64;
            items.push(turn_scoped_item(
                &format!("small-{turn}"),
                ordinal,
                &format!("turn-{turn}"),
                0,
            ));
        }
        store.commit(1, &items, &[]).await.unwrap();
        for turn in 1..=5u64 {
            let last = if turn == 1 {
                "bulk-3".to_string()
            } else {
                format!("small-{turn}")
            };
            publish_turn(&store, &format!("turn-{turn}"), &last).await;
        }

        let first = store.turn_page(None, 200).await.unwrap();
        assert_eq!(
            page_turn_ids(&first),
            vec!["turn-5", "turn-4", "turn-3", "turn-2", "turn-1"],
            "Turn 页按 last_ordinal 倒序 keyset 返回"
        );
        assert_eq!(
            page_item_ids(&first),
            vec![
                "small-5", "small-4", "small-3", "small-2", "bulk-1", "bulk-2"
            ],
            "字节预算在 Turn 内部停住，只返回装得下的条目窗口"
        );
        // 生产计量是条目与 Turn 元数据序列化字节之和；这里留一点元数据差异的余量，而不受预算
        // 约束时这一页会到 2.4 MiB，仍会被这条断言抓住。
        assert!(
            serde_json::to_string(&first).unwrap().len() <= PAGE_BYTE_BUDGET + 64 * 1024,
            "一页必须受整页字节预算约束"
        );
        let cursor = first
            .next_cursor
            .clone()
            .expect("超大 Turn 还有未返回的条目");

        let second = store.turn_page(Some(&cursor), 200).await.unwrap();
        assert_eq!(page_turn_ids(&second), vec!["turn-1"]);
        assert_eq!(page_item_ids(&second), vec!["bulk-3"], "剩余条目被续传取回");
        assert!(second.next_cursor.is_none(), "历史已取完");

        let mut seen = page_item_ids(&first);
        seen.extend(page_item_ids(&second));
        seen.sort();
        let mut expected = items.iter().map(|item| item.id.clone()).collect::<Vec<_>>();
        expected.sort();
        assert_eq!(seen, expected, "沿游标链取回的条目必须不重不漏");
    }

    /// 单条 Turn 本身就超过整页预算时：先留给下一页，再作为一条不可再拆的有界记录返回。
    #[tokio::test]
    async fn an_oversized_turn_is_deferred_to_its_own_page_and_stays_reachable() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        let mut items = vec![turn_scoped_item("huge-1", 1, "turn-1", 525)];
        for (offset, turn) in (2..=5u64).enumerate() {
            let ordinal = 2 + offset as u64;
            items.push(turn_scoped_item(
                &format!("small-{turn}"),
                ordinal,
                &format!("turn-{turn}"),
                0,
            ));
        }
        store.commit(1, &items, &[]).await.unwrap();
        for turn in 1..=5u64 {
            let last = if turn == 1 {
                "huge-1".to_string()
            } else {
                format!("small-{turn}")
            };
            publish_turn(&store, &format!("turn-{turn}"), &last).await;
        }

        let first = store.turn_page(None, 200).await.unwrap();
        assert_eq!(
            page_turn_ids(&first),
            vec!["turn-5", "turn-4", "turn-3", "turn-2"],
            "装不下的大 Turn 留给下一页，而不是撑大本页"
        );
        assert!(
            serde_json::to_string(&first).unwrap().len() <= PAGE_BYTE_BUDGET,
            "小 Turn 组成的一页必须在预算内"
        );
        let cursor = first.next_cursor.clone().expect("大 Turn 尚未返回");

        let second = store.turn_page(Some(&cursor), 200).await.unwrap();
        assert_eq!(page_turn_ids(&second), vec!["turn-1"]);
        assert_eq!(page_item_ids(&second), vec!["huge-1"]);
        assert!(
            second.next_cursor.is_none(),
            "单条超预算条目仍必须返回，否则游标无法前进"
        );
    }

    /// Turn 游标绑定 Thread 与数据库身份：旧库、跨 Thread 或位置被改写的游标都必须明确失败。
    #[tokio::test]
    async fn a_turn_cursor_is_bound_to_the_thread_and_database_identity() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        let items = (1..=4u64)
            .map(|ordinal| {
                turn_scoped_item(
                    &format!("item-{ordinal}"),
                    ordinal,
                    &format!("turn-{ordinal}"),
                    0,
                )
            })
            .collect::<Vec<_>>();
        store.commit(1, &items, &[]).await.unwrap();
        for turn in 1..=4u64 {
            publish_turn(&store, &format!("turn-{turn}"), &format!("item-{turn}")).await;
        }
        let connection = store.reader().await.unwrap().expect("内存库已初始化");
        let database_id = connection.database_id.clone();
        let watermark = store.watermark().await.unwrap();

        let page = store.turn_page(None, 2).await.unwrap();
        assert_eq!(page_turn_ids(&page), vec!["turn-4", "turn-3"]);
        let cursor = page.next_cursor.clone().expect("还有更旧的 Turn");
        assert!(
            pl_protocol::thread::TimelineCursor::decode(&cursor).is_some(),
            "Turn 页续传游标必须是版本化 token"
        );
        let next = store.turn_page(Some(&cursor), 2).await.unwrap();
        assert_eq!(page_turn_ids(&next), vec!["turn-2", "turn-1"]);
        assert!(next.next_cursor.is_none());

        // 数据库重建：同名 Turn 仍然存在，但身份不符的游标必须拒绝而不是静默沿用。
        let rebuilt = pl_protocol::thread::TimelineCursor::new(
            "thread",
            "other-database",
            3,
            "turn-3",
            watermark,
        )
        .encode();
        assert!(
            store.turn_page(Some(&rebuilt), 2).await.is_err(),
            "数据库身份不符的 Turn 游标必须失败"
        );
        // 跨 Thread。
        let foreign = pl_protocol::thread::TimelineCursor::new(
            "other-thread",
            database_id.clone(),
            3,
            "turn-3",
            watermark,
        )
        .encode();
        assert!(
            store.turn_page(Some(&foreign), 2).await.is_err(),
            "跨 Thread 的 Turn 游标必须失败"
        );
        // 水位比本库更新的游标（来自更新的历史）。
        let ahead = pl_protocol::thread::TimelineCursor::new(
            "thread",
            database_id.clone(),
            3,
            "turn-3",
            watermark + 1,
        )
        .encode();
        assert!(
            store.turn_page(Some(&ahead), 2).await.is_err(),
            "水位领先的 Turn 游标必须失败"
        );
        // 游标位置被改写：该 ordinal 上不是这条 Turn 的条目。
        let moved = pl_protocol::thread::TimelineCursor::new(
            "thread",
            database_id.clone(),
            2,
            "turn-4",
            watermark,
        )
        .encode();
        assert!(
            store.turn_page(Some(&moved), 2).await.is_err(),
            "落在别的 Turn 条目上的游标必须失败"
        );
    }

    /// 非版本化游标（旧版原始 turn_id）必须显式失败，而不是从"同名的别处"静默续传。
    #[tokio::test]
    async fn a_pre_versioned_raw_turn_cursor_is_rejected() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        let items = (1..=3u64)
            .map(|ordinal| {
                turn_scoped_item(
                    &format!("item-{ordinal}"),
                    ordinal,
                    &format!("turn-{ordinal}"),
                    0,
                )
            })
            .collect::<Vec<_>>();
        store.commit(1, &items, &[]).await.unwrap();
        for turn in 1..=3u64 {
            publish_turn(&store, &format!("turn-{turn}"), &format!("item-{turn}")).await;
        }

        // 同名 Turn 确实存在，但原始 turn_id 无法绑定 Thread/数据库身份/水位，必须报错。
        let rejected = store
            .turn_page(Some("turn-2"), 10)
            .await
            .expect_err("原始 turn_id 不得被当作 Turn 页游标");
        assert!(
            rejected
                .to_string()
                .contains("not a versioned history cursor"),
            "{rejected}"
        );
        // 版本化游标仍然是唯一可续传的输入。
        let page = store.turn_page(None, 2).await.unwrap();
        let cursor = page.next_cursor.clone().expect("还有更旧的 Turn");
        assert!(store.turn_page(Some(&cursor), 2).await.is_ok());
    }

    /// agent 会话窗口：行上限与 `has_more` 用同一份 limit，稀疏过滤按匹配行计算。
    #[tokio::test]
    async fn agent_page_reports_more_rows_at_the_row_limit_and_for_sparse_filters() {
        let store = HistoryStore::open_memory("thread").await.unwrap();
        let items = (1..=60u64)
            .map(|ordinal| {
                if ordinal % 3 == 0 {
                    raw_item(ordinal, 1)
                } else {
                    item(ordinal, 1, true)
                }
            })
            .collect::<Vec<_>>();
        store.commit(1, &items, &[]).await.unwrap();

        // limit 超过行上限时收敛到上限，但仍必须报告“还有更多”，而不是返回 51 条却说没有了。
        let (_, _, page, has_more) = store
            .agent_page(true, false, None, None, None, 500)
            .await
            .unwrap();
        assert_eq!(page.len(), AGENT_PAGE_ROW_LIMIT);
        assert!(has_more, "超出单页行上限时必须报告还有更多条目");
        assert_eq!(page.first().unwrap().ordinal, 60, "倒序从最新条目开始");

        // 稀疏的 text-only 过滤：只按匹配行计算页与 has_more。
        let (_, _, text, has_more) = store
            .agent_page(true, true, None, None, None, 50)
            .await
            .unwrap();
        assert_eq!(text.len(), 40, "只有 text 条目参与本页");
        assert!(!has_more, "text 条目已全部返回");
        assert!(
            text.iter()
                .all(|item| item.kind() == pl_protocol::ThreadItemKind::Text)
        );

        let (_, _, first, has_more) = store
            .agent_page(true, true, None, None, None, 10)
            .await
            .unwrap();
        assert_eq!(first.len(), 10);
        assert!(has_more, "还有更多 text 条目可续传");
        let anchor = first.last().unwrap().ordinal;
        let (_, _, rest, has_more) = store
            .agent_page(true, true, Some(anchor), None, None, 50)
            .await
            .unwrap();
        assert_eq!(rest.len(), 30, "续传取回剩余的 text 条目");
        assert!(!has_more);
        let ordinals = first
            .iter()
            .chain(rest.iter())
            .map(|item| item.ordinal)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ordinals.len(), 40, "text 窗口不重不漏");
    }

    /// 深翻页与稀疏过滤都必须走声明的索引；空库只要求不材料化排序（没有行可读时任选访问路径）。
    #[tokio::test]
    async fn checked_plans_use_the_declared_indexes_for_sparse_history() {
        let sparse = HistoryStore::open_memory("thread").await.unwrap();
        // 稀疏历史：64 条条目分属 64 个 Turn，一半是 raw；没有 kind='turn' 的条目，也没有一条
        // 终态 Turn，所以 terminal 与 text-only 过滤都是稀疏的。
        let items = (1..=64u64)
            .map(|ordinal| {
                turn_scoped_item(
                    &format!("item-{ordinal}"),
                    ordinal,
                    &format!("turn-{ordinal}"),
                    if ordinal % 2 == 0 { 1 } else { 0 },
                )
            })
            .collect::<Vec<_>>();
        sparse.commit(1, &items, &[]).await.unwrap();
        for ordinal in 1..=64u64 {
            publish_turn(
                &sparse,
                &format!("turn-{ordinal}"),
                &format!("item-{ordinal}"),
            )
            .await;
        }

        let turn_keyset = "SELECT turn_id,first_ordinal,last_ordinal FROM history_turns
             WHERE last_ordinal < ? ORDER BY last_ordinal DESC LIMIT ?";
        let latest_terminal = "SELECT payload FROM history_items
             WHERE kind='turn' AND lifecycle='terminal' ORDER BY ordinal DESC LIMIT 1";
        let terminal_after = "SELECT payload FROM history_items
             WHERE kind='turn' AND lifecycle='terminal' AND ordinal > ?
             ORDER BY ordinal ASC LIMIT ?";
        let agent_text = "SELECT payload FROM history_items
             WHERE ordinal <= ? AND kind = 'text' ORDER BY ordinal DESC LIMIT ?";

        let connection = sparse.reader().await.unwrap().expect("内存库已初始化");
        for (sql, values, index) in [
            (
                turn_keyset,
                vec![integer(100).unwrap().into(), integer(10).unwrap().into()],
                "history_turns_by_last_ordinal",
            ),
            (latest_terminal, vec![], "history_items_by_kind_lifecycle"),
            (
                terminal_after,
                vec![integer(0).unwrap().into(), integer(10).unwrap().into()],
                "history_items_by_kind_lifecycle",
            ),
            (
                agent_text,
                vec![integer(100).unwrap().into(), integer(10).unwrap().into()],
                "history_items_by_kind",
            ),
        ] {
            let plan = query_plan(connection, sql, values).await;
            assert!(
                !plan.contains("TEMP B-TREE"),
                "{index} 材料化了排序: {plan}"
            );
            assert!(plan.contains(index), "查询计划未使用 {index}: {plan}");
        }

        // 空库：没有行可读，planner 可以任选访问路径，但同样不得材料化排序。
        let empty = HistoryStore::open_memory("thread").await.unwrap();
        let connection = empty.reader().await.unwrap().expect("内存库已初始化");
        for (sql, values) in [
            (
                turn_keyset,
                vec![integer(100).unwrap().into(), integer(10).unwrap().into()],
            ),
            (latest_terminal, vec![]),
            (
                terminal_after,
                vec![integer(0).unwrap().into(), integer(10).unwrap().into()],
            ),
            (
                agent_text,
                vec![integer(100).unwrap().into(), integer(10).unwrap().into()],
            ),
        ] {
            let plan = query_plan(connection, sql, values).await;
            assert!(
                !plan.contains("TEMP B-TREE"),
                "空库计划材料化了排序: {plan}"
            );
        }
        assert!(empty.turn_page(None, 10).await.unwrap().turns.is_empty());
        let (_, _, page, has_more) = empty
            .agent_page(true, true, None, None, None, 50)
            .await
            .unwrap();
        assert!(page.is_empty() && !has_more);
    }

    /// 忙/锁/IO 类错误允许有界重试；结构或约束错误必须立刻失败闭锁。
    #[test]
    fn only_transient_locking_errors_are_retryable() {
        for retryable in [
            "Execution Error: error returned from database: (code: 5) database is locked",
            "database table is locked",
            "database is busy",
            "disk I/O error",
            "SQLITE_BUSY",
            "SQLITE_LOCKED",
            "SQLITE_IOERR",
        ] {
            assert!(is_retryable_write(retryable), "{retryable} 应可重试");
        }
        for terminal in [
            "UNIQUE constraint failed: history_items.item_id",
            "history item revision conflict",
            "history write sequence moved backwards",
            "no such table: history_items",
        ] {
            assert!(!is_retryable_write(terminal), "{terminal} 不得重试");
        }
    }

    /// 写事务必须等住外部写锁，而不是在 deferred 升级时立刻以 `SQLITE_BUSY` 失败。
    ///
    /// 现场根因同型：sink 的 effect commit 与实时订阅的 ordinal 预留各自打开一条写连接，两条
    /// 事务都先读后写；若用 `BEGIN DEFERRED`，第二条的写升级会绕过 busy handler 立即失败。这里
    /// 让另一个连接先持有写锁，历史写事务必须等它提交后成功。
    #[tokio::test]
    async fn a_history_write_waits_for_a_held_writer_lock_instead_of_failing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.sqlite");
        let store = HistoryStore::open(&path, "thread").await.unwrap();
        store.commit(1, &[item(1, 1, true)], &[]).await.unwrap();

        // 外部连接先取得写锁：`BEGIN IMMEDIATE` + 一条写语句确保锁已经真正拿到。
        let holder = connect(crate::studio::paths::sqlite_url(&path))
            .await
            .unwrap();
        let held = begin_write(&holder).await.unwrap();
        held.execute_raw(statement(
            "UPDATE history_meta SET applied_write_seq=applied_write_seq WHERE id=1",
            vec![],
        ))
        .await
        .unwrap();
        let release = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            held.commit().await.unwrap();
        });

        // 锁被持有的这段时间里，写事务必须等待并最终成功；deferred 升级会在这里立刻失败。
        let committed = tokio::time::timeout(
            Duration::from_secs(4),
            store.commit(2, &[item(2, 2, true)], &[]),
        )
        .await
        .expect("写事务既没有失败也没有一直阻塞");
        committed.expect("持锁者释放后写事务必须成功");
        release.await.unwrap();
        assert_eq!(store.watermark().await.unwrap(), 2);
    }

    /// 已经预留过的身份重复预留必须是纯读，不能在实时帧上抢 `history.sqlite` 的写锁。
    ///
    /// 该用例锁定「`BEGIN IMMEDIATE` 写事务 + 已预留快路径」这一对的相互作用：一旦快路径退化
    /// 成每次都开写事务，持锁期间这条预留就会阻塞（并最终 busy），实时流会与 effect commit 互撞。
    #[tokio::test]
    async fn reserving_an_already_reserved_identity_is_read_only() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.sqlite");
        let store = HistoryStore::open(&path, "thread").await.unwrap();
        // 第一次预留建库并分配 ordinal：现场里订阅先看见这条流。
        let first = store.reserve_ordinals(["item-1".to_owned()]).await.unwrap();
        assert_eq!(first.get("item-1").copied(), Some(1));

        // 外部连接持有写锁期间，同一身份的重复预留必须立刻返回原 ordinal，而不是等写锁。
        let holder = connect(crate::studio::paths::sqlite_url(&path))
            .await
            .unwrap();
        let held = begin_write(&holder).await.unwrap();
        held.execute_raw(statement(
            "UPDATE history_meta SET applied_write_seq=applied_write_seq WHERE id=1",
            vec![],
        ))
        .await
        .unwrap();
        let repeated = tokio::time::timeout(
            Duration::from_millis(500),
            store.reserve_ordinals(["item-1".to_owned()]),
        )
        .await
        .expect("已预留身份的重复预留不得等待写锁")
        .unwrap();
        assert_eq!(repeated.get("item-1").copied(), Some(1));
        held.rollback().await.unwrap();
    }

    /// effect commit 与实时 ordinal 预留共用一个写者句柄（同一条写连接）时，并发写入也不产生
    /// 重复 ordinal，且预留结果可复用、不重新编号。
    ///
    /// 这正是现场的结构：同一 Thread 的 sink 与 live 预览都必须从同一个句柄出发，因此这里把
    /// `commit_effect` 与 `reserve_missing_ordinals` 并发放在同一句柄上。
    #[tokio::test]
    async fn concurrent_commit_and_reservation_share_one_writer_without_duplicate_ordinals() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("history.sqlite");
        // 该 Thread 唯一的有序写者句柄：effect commit 与实时预留都从它出发。
        let writer = HistoryStore::open(&path, "thread").await.unwrap();
        let live = writer.clone();
        assert!(live.is_same_handle(&writer));

        // 并发数据必须先绑定为具名局部量：`tokio::join!` 的展开会在自身作用域内推进两个 future，
        // 直接写 `&[item(..)]`/`&BTreeSet::new()` 会在 `join!` 尚未完成时先丢掉临时值。
        // ordinal 为 0 的条目由这条事务自己分配 ordinal（与预留共用 `history_ordinals`）。
        let items = [item(0, 1, true)];
        let rolled_back_turns = std::collections::BTreeSet::new();
        let (committed, reserved) = tokio::join!(
            writer.commit_effect(
                1,
                EffectCommit {
                    items: &items,
                    rolled_back_turns: &rolled_back_turns,
                    identities: &[],
                    messages: &[],
                    receipts: &[],
                },
            ),
            live.reserve_missing_ordinals(["reasoning-1".to_owned(), "text-1".to_owned()]),
        );
        committed.expect("并发 effect commit 必须成功");
        let reserved = reserved.expect("并发 ordinal 预留必须成功");
        assert_eq!(reserved.len(), 2);
        assert_ne!(
            reserved["reasoning-1"], reserved["text-1"],
            "同一事务里的两个身份必须拿到不同 ordinal"
        );

        // effect 写入的条目与实时预留的身份绝不共用同一个 ordinal。
        let item_ordinal = writer.read_item("item-0").await.unwrap().ordinal;
        for ordinal in reserved.values() {
            assert_ne!(*ordinal, item_ordinal, "实时预留与落库条目 ordinal 冲突");
        }

        // 复用：同一身份再预留仍是原 ordinal，绝不重新编号。
        let repeated = writer
            .reserved_ordinals(["reasoning-1".to_owned(), "text-1".to_owned()])
            .await
            .unwrap();
        assert_eq!(repeated, reserved);
        assert_eq!(writer.watermark().await.unwrap(), 1);
    }
}
