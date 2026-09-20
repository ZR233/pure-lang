//! Transactional write adapter for the Studio timeline index.
//!
//! [`TimelineWriter`] is the explicit actor (write-behind worker, index builder or migration) that
//! owns the derived tables. It offers two atomic operations:
//! - [`TimelineWriter::persist_commit`] advances an existing index by exactly one committed delta.
//! - [`TimelineWriter::write_state`] writes a whole projection at one watermark, replacing any
//!   existing rows for that Thread; the index builder uses it for a fresh build or an explicit
//!   rebuild.
//!
//! Both validate the watermark against a core commit that is already durable in the same session
//! database, so the index can never lead the canonical journal; a caller-supplied watermark alone is
//! never trusted.

use super::content;
use super::schema::{self, TIMELINE_DDL};
use super::{TIMELINE_SCHEMA_VERSION, TimelineStoreError};
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::paths::sqlite_url;
use crate::studio::thread_projection::engine::{
    PersistedSlot, ProjectionDelta, ProjectionFactKey, ProjectionFactRow, ProjectionState, SlotKind,
};
use pl_core::thread::journal::ThreadCommit;
use pl_protocol::{ThreadItem, ThreadItemState, Turn};
use sea_orm::sqlx::sqlite::{SqliteJournalMode, SqliteSynchronous};
use sea_orm::{
    ConnectOptions, ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement,
    TransactionTrait, Value,
};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

const ITEM_INSERT: &str = "INSERT OR REPLACE INTO timeline_items(thread_id, slot_key, ordinal, kind, generation_from, generation_to, revision, item_id, created_at, updated_at, turn_id, preview, preview_truncated, content_ref, content_digest, content_total, content_revision) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)";
const SLOT_UPSERT: &str = "INSERT INTO timeline_slots(thread_id, slot_key, ordinal, created_at) VALUES(?,?,?,?) ON CONFLICT(thread_id,slot_key) DO UPDATE SET ordinal=excluded.ordinal, created_at=excluded.created_at";
const FACT_UPSERT: &str = "INSERT INTO timeline_facts(thread_id, kind, key, seq, aux_turn, aux_call, row) VALUES(?,?,?,?,?,?,?) ON CONFLICT(thread_id,kind,key) DO UPDATE SET seq=excluded.seq, aux_turn=excluded.aux_turn, aux_call=excluded.aux_call, row=excluded.row";
const HEAD_UPSERT: &str = "INSERT INTO timeline_head(thread_id, watermark, next_ordinal, generation, updated_at) VALUES(?,?,?,?,?) ON CONFLICT(thread_id) DO UPDATE SET watermark=excluded.watermark, next_ordinal=excluded.next_ordinal, generation=excluded.generation, updated_at=excluded.updated_at";
const PANEL_UPSERT: &str = "INSERT INTO timeline_panel(thread_id, watermark, panel) VALUES(?,?,?) ON CONFLICT(thread_id) DO UPDATE SET watermark=excluded.watermark, panel=excluded.panel";
/// A Turn item write: it is the only source of the Turn's display json and stable sort ordinal.
const TURN_UPSERT: &str = "INSERT INTO timeline_turns(thread_id, turn_id, turn_json, last_ordinal, last_item_id, context_disposition, turn_ordinal, admitted_watermark) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(thread_id,turn_id) DO UPDATE SET turn_json=excluded.turn_json, turn_ordinal=excluded.turn_ordinal, admitted_watermark=COALESCE(timeline_turns.admitted_watermark, excluded.admitted_watermark)";
/// Any item of a Turn advances its end boundary monotonically without touching the sort ordinal.
const TURN_END_UPSERT: &str = "INSERT INTO timeline_turns(thread_id, turn_id, turn_json, last_ordinal, last_item_id, context_disposition, turn_ordinal, admitted_watermark) VALUES(?,?,'',?,?,?,NULL,?) ON CONFLICT(thread_id,turn_id) DO UPDATE SET last_ordinal=excluded.last_ordinal, last_item_id=excluded.last_item_id WHERE excluded.last_ordinal > timeline_turns.last_ordinal";
/// A saved rollback disposition. A rewind targets an existing Turn, so this is update-first; the
/// insert branch only fires for a Turn with no row yet and records `admitted_watermark` as NULL
/// ("not admitted by any known watermark") instead of fabricating a sequence.
const TURN_DISPOSITION_UPSERT: &str = "INSERT INTO timeline_turns(thread_id, turn_id, turn_json, last_ordinal, last_item_id, context_disposition, turn_ordinal, admitted_watermark) VALUES(?,?,'',0,'',?,NULL,NULL) ON CONFLICT(thread_id,turn_id) DO UPDATE SET context_disposition=excluded.context_disposition";

/// Derived tables this writer owns; a rebuild clears them for one Thread only.
const THREAD_TABLES: [&str; 8] = [
    "timeline_content",
    "timeline_content_meta",
    "timeline_items",
    "timeline_slots",
    "timeline_facts",
    "timeline_turns",
    "timeline_panel",
    "timeline_head",
];

/// A bounded, read-write handle to one per-Thread session database's Studio timeline index.
#[derive(Debug)]
pub(crate) struct TimelineWriter {
    db: DatabaseConnection,
}

impl TimelineWriter {
    /// Opens the session database read-write and ensures the derived index schema exists.
    ///
    /// The database file must already exist; opening never creates an empty session database.
    ///
    /// # Errors
    /// Fails when the file is missing, the existing index declares a future schema version, or the
    /// connection or schema creation fails.
    pub(crate) async fn open(path: impl AsRef<Path>) -> Result<Self, TimelineStoreError> {
        let path = path.as_ref();
        if !tokio::fs::try_exists(path).await? {
            return Err(TimelineStoreError::MissingDatabase {
                path: path.to_path_buf(),
            });
        }
        let mut options = ConnectOptions::new(sqlite_url(path));
        options
            .max_connections(1)
            .min_connections(1)
            .connect_timeout(Duration::from_secs(8))
            .acquire_timeout(Duration::from_secs(8))
            .map_sqlx_sqlite_opts(|options| {
                options
                    .journal_mode(SqliteJournalMode::Wal)
                    .synchronous(SqliteSynchronous::Normal)
                    .busy_timeout(Duration::from_secs(5))
            })
            .sqlx_logging(false);
        let db = Database::connect(options).await?;
        let writer = Self { db };
        match writer.ensure_schema().await {
            Ok(()) => Ok(writer),
            Err(error) => {
                let _ = writer.db.close().await;
                Err(error)
            }
        }
    }

    /// Drains and closes the bounded write connection.
    ///
    /// # Errors
    /// Returns the underlying database error if the handle could not be released.
    pub(crate) async fn close(self) -> Result<(), TimelineStoreError> {
        self.db.close().await?;
        Ok(())
    }

    /// Creates the Studio timeline tables if absent, adds post-v1 columns, and pins the version.
    ///
    /// An older index is upgraded additively in place (no row rewrite, no data loss); a future
    /// version is refused so it is never rewritten by an older writer.
    ///
    /// # Errors
    /// Fails when an existing index declares a newer schema version, or on a database failure.
    pub(crate) async fn ensure_schema(&self) -> Result<(), TimelineStoreError> {
        match self.stored_schema_version().await? {
            // The steady state must not run DDL: a writer is opened per Thread while the core store
            // keeps writing the same file, and DDL would take a write lock on every open.
            Some(TIMELINE_SCHEMA_VERSION) => return Ok(()),
            Some(found) if found > TIMELINE_SCHEMA_VERSION => {
                return Err(TimelineStoreError::UnsupportedSchema {
                    found,
                    supported: TIMELINE_SCHEMA_VERSION,
                });
            }
            _ => {}
        }
        self.db.execute_unprepared(TIMELINE_DDL).await?;
        self.apply_additive_columns().await?;
        self.db
            .execute_raw(statement(
                "INSERT INTO timeline_meta(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                vec![
                    schema::META_SCHEMA_KEY.into(),
                    TIMELINE_SCHEMA_VERSION.to_string().into(),
                ],
            ))
            .await?;
        Ok(())
    }

    async fn stored_schema_version(&self) -> Result<Option<i64>, TimelineStoreError> {
        // The marker table may not exist yet; that is a fresh index, not an error.
        let row = match self
            .db
            .query_one_raw(statement(
                "SELECT value FROM timeline_meta WHERE key=?",
                vec![schema::META_SCHEMA_KEY.into()],
            ))
            .await
        {
            Ok(row) => row,
            Err(_) => return Ok(None),
        };
        match row {
            Some(row) => {
                let value: String = row.try_get("", "value")?;
                Ok(Some(value.parse::<i64>().map_err(|_| {
                    TimelineStoreError::Corrupt(format!(
                        "timeline index schema marker `{value}` is not an integer"
                    ))
                })?))
            }
            None => Ok(None),
        }
    }

    /// Adds post-v1 columns to an existing index without rewriting any row or dropping data.
    async fn apply_additive_columns(&self) -> Result<(), TimelineStoreError> {
        for (table, column, definition) in schema::TIMELINE_ADDITIVE_COLUMNS {
            if !self.table_has_column(table, column).await? {
                self.db
                    .execute_unprepared(&format!("ALTER TABLE {table} ADD COLUMN {definition}"))
                    .await?;
            }
        }
        Ok(())
    }

    async fn table_has_column(
        &self,
        table: &str,
        column: &str,
    ) -> Result<bool, TimelineStoreError> {
        let rows = self
            .db
            .query_all_raw(statement(&format!("PRAGMA table_info({table})"), vec![]))
            .await?;
        for row in rows {
            if row.try_get::<String>("", "name")? == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Persists one committed projection delta atomically, validated against the durable journal.
    ///
    /// `commit` is the canonical commit and `delta` the value `ProjectionState::apply` returned for
    /// it. An ephemeral preview delta from `set_model_progress`/`set_tool_progress` must never be
    /// passed: it carries no new commit, so the head-advance check rejects it instead of persisting
    /// a preview as durable history.
    ///
    /// # Errors
    /// Fails on a non-contiguous or non-durable watermark, a mismatched commit identity, or any
    /// database failure inside the transaction; the transaction is rolled back so no half-written
    /// index state is left behind.
    pub(crate) async fn persist_commit(
        &self,
        commit: &ThreadCommit,
        state: &ProjectionState,
        delta: &ProjectionDelta,
    ) -> Result<(), TimelineStoreError> {
        let sequence = delta.watermark;
        if sequence == 0 || commit.sequence != sequence {
            return Err(TimelineStoreError::InvalidRequest(format!(
                "commit {} does not match the applied watermark {sequence}",
                commit.sequence
            )));
        }
        let head = state.head();
        if commit.thread_id != head.thread_id {
            return Err(TimelineStoreError::InvalidRequest(format!(
                "commit Thread {} does not own projection {}",
                commit.thread_id, head.thread_id
            )));
        }
        let thread_id = head.thread_id;
        let tx = self.db.begin().await?;
        let outcome = self
            .persist_in_transaction(&tx, &thread_id, state, delta, sequence)
            .await;
        match outcome {
            Ok(()) => {
                tx.commit().await?;
                Ok(())
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    /// Writes a whole projection at its watermark, replacing any existing rows for the Thread.
    ///
    /// This is the fresh-build and explicit-rebuild primitive: it clears this Thread's `timeline_*`
    /// rows and rewrites head, facts, slots (with chunked content), Turn metadata and panel in one
    /// transaction. It never touches other Threads' rows or any core table except the durability
    /// probe.
    ///
    /// # Errors
    /// Fails when the state watermark is not a durable core commit, or on any database failure
    /// inside the transaction; the transaction is rolled back so the previous rows survive.
    pub(crate) async fn write_state(
        &self,
        thread_id: &str,
        state: &ProjectionState,
    ) -> Result<(), TimelineStoreError> {
        let tx = self.db.begin().await?;
        let outcome = self.write_state_in_transaction(&tx, thread_id, state).await;
        match outcome {
            Ok(()) => {
                tx.commit().await?;
                Ok(())
            }
            Err(error) => {
                let _ = tx.rollback().await;
                Err(error)
            }
        }
    }

    async fn persist_in_transaction(
        &self,
        tx: &sea_orm::DatabaseTransaction,
        thread_id: &str,
        state: &ProjectionState,
        delta: &ProjectionDelta,
        sequence: u64,
    ) -> Result<(), TimelineStoreError> {
        self.require_durable(tx, thread_id, sequence).await?;

        let head = state.head();
        let generation = match tx
            .query_one_raw(statement(
                "SELECT watermark, generation FROM timeline_head WHERE thread_id=?",
                vec![thread_id.into()],
            ))
            .await?
        {
            Some(row) => {
                let watermark = unsigned(row.try_get::<i64>("", "watermark")?)?;
                let expected = watermark.saturating_add(1);
                if expected != sequence {
                    return Err(TimelineStoreError::SequenceGap {
                        expected,
                        found: sequence,
                    });
                }
                row.try_get::<String>("", "generation")?
            }
            None => {
                if sequence != 1 {
                    return Err(TimelineStoreError::SequenceGap {
                        expected: 1,
                        found: sequence,
                    });
                }
                new_id("gen")
            }
        };
        self.write_head(tx, thread_id, sequence, head.next_ordinal, &generation)
            .await?;

        for (key, row) in &delta.fact_writes {
            self.write_fact_row(tx, thread_id, key, row.as_ref()).await?;
        }

        let mut touched: BTreeSet<String> = BTreeSet::new();
        touched.extend(delta.new_slots.iter().cloned());
        touched.extend(delta.changed.iter().cloned());
        touched.extend(delta.removed.iter().cloned());
        for slot_key in touched {
            if let Some(slot) = state.slot(&slot_key) {
                self.write_slot_row(tx, thread_id, sequence, &slot, true).await?;
            }
        }

        // Turn dispositions changed by this commit's saved rewind replacements come from the
        // projector's own delta, so the store and the runtime share exactly one rollback rule.
        for (turn_id, disposition) in &delta.context_disposition {
            self.write_turn_disposition(tx, thread_id, turn_id, *disposition)
                .await?;
        }

        self.write_panel(tx, thread_id, sequence, state).await?;
        Ok(())
    }

    async fn write_state_in_transaction(
        &self,
        tx: &sea_orm::DatabaseTransaction,
        thread_id: &str,
        state: &ProjectionState,
    ) -> Result<(), TimelineStoreError> {
        let head = state.head();
        let sequence = head.watermark;
        if sequence > 0 {
            self.require_durable(tx, thread_id, sequence).await?;
        }
        self.delete_thread_rows(tx, thread_id).await?;
        let generation = new_id("gen");
        self.write_head(tx, thread_id, sequence, head.next_ordinal, &generation)
            .await?;
        for (key, row) in state.facts().rows() {
            self.write_fact_row(tx, thread_id, &key, Some(&row)).await?;
        }
        for slot in state.persisted_slots() {
            // The rows were cleared above, so there is no current version to close.
            self.write_slot_row(tx, thread_id, sequence, &slot, false).await?;
        }
        for turn_id in state.rolled_back() {
            self.write_turn_disposition(
                tx,
                thread_id,
                turn_id,
                pl_protocol::ThreadContextDisposition::RolledBack,
            )
            .await?;
        }
        self.write_panel(tx, thread_id, sequence, state).await?;
        Ok(())
    }

    /// Deletes every derived row this writer owns for one Thread.
    async fn delete_thread_rows(
        &self,
        tx: &sea_orm::DatabaseTransaction,
        thread_id: &str,
    ) -> Result<(), TimelineStoreError> {
        for table in THREAD_TABLES {
            tx.execute_raw(statement(
                &format!("DELETE FROM {table} WHERE thread_id=?"),
                vec![thread_id.into()],
            ))
            .await?;
        }
        Ok(())
    }

    /// Fails unless the core journal already holds the commit for `sequence` in this database.
    async fn require_durable(
        &self,
        tx: &sea_orm::DatabaseTransaction,
        thread_id: &str,
        sequence: u64,
    ) -> Result<(), TimelineStoreError> {
        let commit_id = format!("pl.resource.thread-commit.{sequence:020}");
        let durable = tx
            .query_one_raw(statement(
                "SELECT 1 AS present FROM session_entries WHERE session_id=? AND id=?",
                vec![thread_id.into(), commit_id.into()],
            ))
            .await?;
        if durable.is_none() {
            return Err(TimelineStoreError::WatermarkNotDurable {
                thread_id: thread_id.to_owned(),
                sequence,
            });
        }
        Ok(())
    }

    async fn write_head(
        &self,
        tx: &sea_orm::DatabaseTransaction,
        thread_id: &str,
        watermark: u64,
        next_ordinal: u64,
        generation: &str,
    ) -> Result<(), TimelineStoreError> {
        tx.execute_raw(statement(
            HEAD_UPSERT,
            vec![
                thread_id.into(),
                (watermark as i64).into(),
                (next_ordinal as i64).into(),
                generation.into(),
                unix_seconds().into(),
            ],
        ))
        .await?;
        Ok(())
    }

    async fn write_fact_row(
        &self,
        tx: &sea_orm::DatabaseTransaction,
        thread_id: &str,
        key: &ProjectionFactKey,
        row: Option<&ProjectionFactRow>,
    ) -> Result<(), TimelineStoreError> {
        let kind = schema::fact_kind_label(key);
        let identity = schema::fact_key_string(key);
        match row {
            Some(row) => {
                let (seq, aux_turn, aux_call) = schema::fact_index_columns(key, row);
                let json = serde_json::to_string(row)?;
                tx.execute_raw(statement(
                    FACT_UPSERT,
                    vec![
                        thread_id.into(),
                        kind.into(),
                        identity.into(),
                        seq.into(),
                        Value::String(aux_turn),
                        Value::String(aux_call),
                        json.into(),
                    ],
                ))
                .await?;
            }
            None => {
                tx.execute_raw(statement(
                    "DELETE FROM timeline_facts WHERE thread_id=? AND kind=? AND key=?",
                    vec![thread_id.into(), kind.into(), identity.into()],
                ))
                .await?;
            }
        }
        Ok(())
    }

    async fn write_panel(
        &self,
        tx: &sea_orm::DatabaseTransaction,
        thread_id: &str,
        sequence: u64,
        state: &ProjectionState,
    ) -> Result<(), TimelineStoreError> {
        let panel = serde_json::to_string(state.panel_state())?;
        tx.execute_raw(statement(
            PANEL_UPSERT,
            vec![thread_id.into(), (sequence as i64).into(), panel.into()],
        ))
        .await?;
        Ok(())
    }

    async fn write_turn_disposition(
        &self,
        tx: &sea_orm::DatabaseTransaction,
        thread_id: &str,
        turn_id: &str,
        disposition: pl_protocol::ThreadContextDisposition,
    ) -> Result<(), TimelineStoreError> {
        tx.execute_raw(statement(
            TURN_DISPOSITION_UPSERT,
            vec![
                thread_id.into(),
                turn_id.into(),
                schema::disposition_label(disposition).into(),
            ],
        ))
        .await?;
        Ok(())
    }

    /// Writes one stable slot position plus its current item version and chunked content.
    async fn write_slot_row(
        &self,
        tx: &sea_orm::DatabaseTransaction,
        thread_id: &str,
        sequence: u64,
        slot: &PersistedSlot,
        close_current: bool,
    ) -> Result<(), TimelineStoreError> {
        if close_current {
            tx.execute_raw(statement(
                "UPDATE timeline_items SET generation_to=? WHERE thread_id=? AND slot_key=? AND generation_to IS NULL AND generation_from<>?",
                vec![
                    (sequence as i64).into(),
                    thread_id.into(),
                    slot.key.as_str().into(),
                    (sequence as i64).into(),
                ],
            ))
            .await?;
        }
        tx.execute_raw(statement(
            SLOT_UPSERT,
            vec![
                thread_id.into(),
                slot.key.as_str().into(),
                (slot.ordinal as i64).into(),
                slot.created_at.into(),
            ],
        ))
        .await?;
        match &slot.item {
            Some(item) => {
                let kind = slot.kind.ok_or_else(|| {
                    TimelineStoreError::Corrupt(format!("visible slot {} has no category", slot.key))
                })?;
                let encoded = serde_json::to_vec(item)?;
                let plan = content::plan_content(thread_id, &slot.key, item.revision, &encoded)?;
                if let Some(reference) = &plan.reference {
                    tx.execute_raw(statement(
                        "INSERT OR REPLACE INTO timeline_content_meta(thread_id, ref_id, digest, total_bytes, revision, item_id, slot_key) VALUES(?,?,?,?,?,?,?)",
                        vec![
                            thread_id.into(),
                            reference.ref_id.as_str().into(),
                            reference.digest.as_str().into(),
                            (reference.total_bytes as i64).into(),
                            (reference.revision as i64).into(),
                            item.id.as_str().into(),
                            slot.key.as_str().into(),
                        ],
                    ))
                    .await?;
                    for (index, chunk) in plan.chunks.iter().enumerate() {
                        tx.execute_raw(statement(
                            "INSERT OR IGNORE INTO timeline_content(thread_id, ref_id, chunk, bytes) VALUES(?,?,?,?)",
                            vec![
                                thread_id.into(),
                                reference.ref_id.as_str().into(),
                                (index as i64).into(),
                                Value::Bytes(Some(chunk.clone())),
                            ],
                        ))
                        .await?;
                    }
                }
                let reference = plan.reference.as_ref();
                tx.execute_raw(statement(
                    ITEM_INSERT,
                    vec![
                        thread_id.into(),
                        slot.key.as_str().into(),
                        (slot.ordinal as i64).into(),
                        schema::slot_kind_label(kind).into(),
                        (sequence as i64).into(),
                        Value::BigInt(None),
                        (item.revision as i64).into(),
                        item.id.as_str().into(),
                        item.created_at.into(),
                        item.updated_at.into(),
                        item.turn_id.as_str().into(),
                        Value::String(Some(plan.preview)),
                        (i64::from(plan.truncated)).into(),
                        Value::String(reference.map(|reference| reference.ref_id.clone())),
                        Value::String(reference.map(|reference| reference.digest.clone())),
                        Value::BigInt(reference.map(|reference| reference.total_bytes as i64)),
                        Value::BigInt(reference.map(|reference| reference.revision as i64)),
                    ],
                ))
                .await?;
                self.record_turn(tx, thread_id, sequence, kind, item).await?;
            }
            None => {
                tx.execute_raw(statement(
                    ITEM_INSERT,
                    vec![
                        thread_id.into(),
                        slot.key.as_str().into(),
                        (slot.ordinal as i64).into(),
                        Value::String(None),
                        (sequence as i64).into(),
                        Value::BigInt(None),
                        0_i64.into(),
                        String::new().into(),
                        slot.created_at.into(),
                        slot.created_at.into(),
                        String::new().into(),
                        Value::String(None),
                        0_i64.into(),
                        Value::String(None),
                        Value::String(None),
                        Value::BigInt(None),
                        Value::BigInt(None),
                    ],
                ))
                .await?;
            }
        }
        Ok(())
    }

    async fn record_turn(
        &self,
        tx: &sea_orm::DatabaseTransaction,
        thread_id: &str,
        sequence: u64,
        kind: SlotKind,
        item: &ThreadItem,
    ) -> Result<(), TimelineStoreError> {
        if item.turn_id.is_empty() || kind == SlotKind::Compaction {
            return Ok(());
        }
        let active = schema::disposition_label(pl_protocol::ThreadContextDisposition::Active);
        if let ThreadItemState::Turn(turn_item) = item.state() {
            let turn = Turn {
                input_id: turn_item.input_id().map(str::to_owned),
                id: item.turn_id.clone(),
                thread_id: item.thread_id.clone(),
                revision: item.revision,
                state: turn_item.state().clone(),
                updated_at: item.updated_at,
            };
            let json = serde_json::to_string(&turn)?;
            // The Turn item alone carries the Turn's display json and its stable sort ordinal.
            tx.execute_raw(statement(
                TURN_UPSERT,
                vec![
                    thread_id.into(),
                    item.turn_id.as_str().into(),
                    json.into(),
                    0_i64.into(),
                    String::new().into(),
                    active.into(),
                    (item.ordinal as i64).into(),
                    (sequence as i64).into(),
                ],
            ))
            .await?;
        }
        // Any item of the Turn advances its end boundary; the sort ordinal and admission watermark
        // of an existing row are left untouched.
        tx.execute_raw(statement(
            TURN_END_UPSERT,
            vec![
                thread_id.into(),
                item.turn_id.as_str().into(),
                (item.ordinal as i64).into(),
                item.id.as_str().into(),
                active.into(),
                (sequence as i64).into(),
            ],
        ))
        .await?;
        Ok(())
    }
}

pub(super) fn unsigned(value: i64) -> Result<u64, TimelineStoreError> {
    u64::try_from(value).map_err(|_| TimelineStoreError::Corrupt(format!("negative counter {value}")))
}

pub(super) fn statement(sql: &str, values: Vec<Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::timeline_store::fixture;
    use crate::studio::timeline_store::{TimelineBudget, TimelinePageQuery, TimelineReader};
    use pretty_assertions::assert_eq;

    #[tokio::test]
    async fn a_non_durable_watermark_rolls_back_without_a_half_written_index() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let mut first = fixture::commit(1);
        first.turn = Some(fixture::turn(None));
        let mut second = fixture::commit(2);
        second.inputs = vec![fixture::accepted("input2", 2, "second")].into();
        // Only the first commit is durable in the core journal.
        fixture::seed(&path, "thread", std::slice::from_ref(&first)).await;

        let writer = TimelineWriter::open(&path).await.unwrap();
        let mut state = ProjectionState::new("thread", None);
        let delta = state.apply(&first).unwrap();
        writer.persist_commit(&first, &state, &delta).await.unwrap();

        let delta = state.apply(&second).unwrap();
        let error = writer
            .persist_commit(&second, &state, &delta)
            .await
            .unwrap_err();
        assert!(
            matches!(error, TimelineStoreError::WatermarkNotDurable { sequence: 2, .. }),
            "unexpected {error:?}"
        );
        writer.close().await.unwrap();

        let reader = TimelineReader::open(&path).await.unwrap();
        assert_eq!(reader.read_head("thread").await.unwrap().watermark, 1);
        let window = reader
            .page(
                "thread",
                &TimelinePageQuery::Latest,
                &TimelineBudget::default(),
            )
            .await
            .unwrap();
        assert!(window.entries.iter().all(|entry| entry.item_id != "input2"));
        reader.close().await.unwrap();
    }

    #[tokio::test]
    async fn a_rewind_commit_marks_its_turns_rolled_back_without_core_replay() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let mut first = fixture::commit(1);
        first.turn = Some(fixture::turn(None));
        let mut rewind = fixture::commit(2);
        rewind.replacements = vec![fixture::rewind("t1")].into();
        fixture::indexed(&path, "thread", &[first, rewind]).await;

        let reader = TimelineReader::open(&path).await.unwrap();
        let window = reader
            .page(
                "thread",
                &TimelinePageQuery::Latest,
                &TimelineBudget::default(),
            )
            .await
            .unwrap();
        assert_eq!(window.turns.len(), 1);
        assert_eq!(
            window.turns[0].context_disposition,
            pl_protocol::ThreadContextDisposition::RolledBack
        );
        reader.close().await.unwrap();
    }

    #[tokio::test]
    async fn restart_restores_the_projection_from_persisted_rows_and_continues() {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let mut journal = vec![{
            let mut first = fixture::commit(1);
            first.inputs = vec![fixture::accepted("input1", 1, "hello")].into();
            first.turn = Some(fixture::turn(Some("input1")));
            first
        }];
        let mut second = fixture::commit(2);
        second.attempt = Some(fixture::running_attempt("a1"));
        journal.push(second);
        let mut third = fixture::commit(3);
        third.attempt = Some(fixture::committed("a1", "answer"));
        journal.push(third);
        let original = fixture::indexed(&path, "thread", &journal).await;

        let reader = TimelineReader::open(&path).await.unwrap();
        let source = reader.restore_source("thread", None).await.unwrap();
        assert_eq!(source.head.watermark, 3);
        assert_eq!(source.head.next_ordinal, original.next_ordinal());
        let mut restored =
            ProjectionState::restore(source.head, source.facts, source.panel, source.slots);
        assert_eq!(restored.materialize(), original.materialize());
        reader.close().await.unwrap();

        let mut next = fixture::commit(4);
        next.attempt = Some(fixture::committed("a2", "follow up"));
        let mut expected = original;
        expected.apply(&next).unwrap();
        restored.apply(&next).unwrap();
        assert_eq!(restored.materialize(), expected.materialize());
    }

    #[tokio::test]
    async fn idle_index_connections_do_not_block_the_core_journal_writer() {
        // The runtime keeps one index writer and reader open per Thread for the Thread's lifetime,
        // while the core store keeps flushing the same file. Idle index connections must never hold
        // a lock that makes a concurrent core journal write fail with SQLITE_BUSY.
        let dir = tempfile::tempdir().unwrap();
        let path = fixture::session_path(&dir);
        let mut first = fixture::commit(1);
        first.turn = Some(fixture::turn(None));
        let mut second = fixture::commit(2);
        second.attempt = Some(fixture::committed("a1", "answer"));
        fixture::seed(&path, "thread", &[first.clone(), second.clone()]).await;

        // Keep both index connections open and idle across a core flush.
        let writer = TimelineWriter::open(&path).await.unwrap();
        let reader = TimelineReader::open(&path).await.unwrap();

        let mut third = fixture::commit(3);
        third.inputs = vec![fixture::accepted("input3", 3, "third")].into();
        fixture::seed(&path, "thread", &[first, second, third]).await;

        reader.close().await.unwrap();
        writer.close().await.unwrap();
}
}
