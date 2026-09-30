//! Explicit offline migration. Normal store opening never resets or converts history.
use super::{SessionStoreError, SqliteSessionOptions, history, sqlite, thread_history};
use crate::{
    context::OpaquePayload,
    storage::{SessionEntry, SessionEntryChange},
    thread::ThreadEffectBatch,
};
use sea_orm::{ConnectionTrait, Database, TransactionTrait};
use std::sync::Arc;

/// Minimal view of a stored checkpoint the usage backfill needs.
///
/// It deliberately omits the resident attempts so a schema-1/2 envelope decodes without the current
/// read path, which no longer interprets that layout; the attempt bodies stay byte-for-byte intact
/// for the later conversion step.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageBackfillEnvelope {
    thread_id: String,
    state_revision: u64,
    history_fence: u64,
    #[serde(default)]
    external_bodies: Vec<serde_json::Value>,
    state: UsageBackfillState,
}

/// The two checkpoint state facts the usage backfill reads and rewrites.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageBackfillState {
    commit_sequence: u64,
    #[serde(default)]
    usage_summary: crate::thread::UsageSummary,
}

/// Upgrades an existing session database through every supported schema step.
///
/// The caller owns the surrounding runtime and makes a consistent backup before invoking this
/// method. This method holds the database's exclusive writer lease across the full upgrade; each
/// version step commits separately, so retrying after an interruption resumes from the last
/// committed schema. Normal [`super::SqliteSessionStore::open`] never runs migrations.
///
/// # Errors
/// Rejects a missing database, an active writer, an unsupported version, invalid history,
/// conversion failure or storage failure without clearing existing data.
pub async fn migrate_to_current(
    options: SqliteSessionOptions,
    transform: impl Fn(&mut ThreadEffectBatch) -> Result<(), SessionStoreError> + Send + Sync,
) -> Result<(), SessionStoreError> {
    if !tokio::fs::try_exists(&options.path).await? {
        return Err(SessionStoreError::Invalid(
            "session database does not exist".into(),
        ));
    }
    let _lease = sqlite::acquire_database_lock(options.path.clone()).await?;
    migrate_v6(options.clone(), transform).await?;
    migrate_v7(options.clone()).await?;
    migrate_v8(options.clone()).await?;
    migrate_v9(options).await
}

/// Migrates version 6 to 7 in one transaction, preserving entry identities and all history.
/// Called only while `migrate_to_current` holds the database's exclusive lease.
/// The callback converts only product-owned payloads; it must not perform external side effects.
/// # Errors
/// Rejects unsupported versions, damaged history, conversion failures and storage failures.
async fn migrate_v6(
    options: SqliteSessionOptions,
    transform: impl Fn(&mut ThreadEffectBatch) -> Result<(), SessionStoreError> + Send + Sync,
) -> Result<(), SessionStoreError> {
    let mut url =
        url::Url::parse("sqlite:///").map_err(|e| SessionStoreError::Invalid(e.to_string()))?;
    url.set_path(
        options
            .path
            .to_str()
            .ok_or_else(|| SessionStoreError::Invalid("non-UTF8 database path".into()))?,
    );
    url.set_query(Some("mode=rw"));
    let db = Database::connect(url.to_string()).await?;
    let result = async {
        let tx = db.begin().await?;
        let version = tx.query_one_raw(sqlite::statement("PRAGMA user_version", vec![])).await?.ok_or_else(|| SessionStoreError::Invalid("missing version".into()))?.try_get::<i64>("", "user_version")?;
        // A versioned step accepts only its own target or later steps this facade knows about.
        // Do not tie its no-op range to SESSION_SCHEMA_VERSION: a future schema bump must add a
        // new step to migrate_to_current, not silently redefine what this migration did.
        if version == 7 || version == 8 || version == 9 || version == 10 { tx.commit().await?; return Ok(()); }
        if version != 6 { return Err(SessionStoreError::UnsupportedSchema { found: version, supported: super::SESSION_SCHEMA_VERSION }); }
        let sessions = tx.query_all_raw(sqlite::statement("SELECT session_id FROM session_history_heads UNION SELECT session_id FROM session_entries UNION SELECT session_id FROM session_entry_history", vec![])).await?;
        for row in sessions {
            let id: String = row.try_get("", "session_id")?;
            let mut history = history::read(&tx, &id, None).await?;
            let mut current = sqlite::entries(&tx, &id, None).await?;
            let sort = |entries: &mut Vec<SessionEntry>| entries.sort_by(|a,b| a.id.cmp(&b.id));
            sort(&mut current);
            let mut replay = crate::storage::replay_entries(&history)?;
            sort(&mut replay);
            if replay != current { return Err(SessionStoreError::Invalid("history and current entries disagree".into())); }
            for batch in &mut history {
                for change in &mut batch.changes {
                    if let SessionEntryChange::Put { entry } = change { convert(entry, &transform)?; }
                }
                let encoded = serde_json::to_string(batch)?;
                let hash = crate::context::content_hash(encoded.as_bytes());
                tx.execute_raw(sqlite::statement("UPDATE session_entry_history SET envelope=?,payload_hash=? WHERE session_id=? AND sequence=?", vec![encoded.into(), hash.clone().into(), id.clone().into(), (batch.sequence as i64).into()])).await?;
                tx.execute_raw(sqlite::statement("UPDATE session_history_heads SET payload_hash=? WHERE session_id=? AND sequence=?", vec![hash.into(),id.clone().into(),(batch.sequence as i64).into()])).await?;
            }
            for entry in &mut current {
                convert(entry, &transform)?;
                let encoded = serde_json::to_string(entry)?;
                let hash = crate::context::content_hash(encoded.as_bytes());
                tx.execute_raw(sqlite::statement("UPDATE session_entries SET envelope=?,payload_hash=? WHERE session_id=? AND id=?", vec![encoded.into(),hash.into(),id.clone().into(),entry.id.clone().into()])).await?;
            }
            let mut replay = crate::storage::replay_entries(&history)?;
            sort(&mut replay);
            if replay != current { return Err(SessionStoreError::Invalid("migration changed current/history equivalence".into())); }
            let mut commits = Vec::new();
            for entry in &current {
                if entry.id.starts_with("pl.resource.thread-commit.") {
                    let payload = OpaquePayload::new(entry.type_id.clone(),entry.schema_version,entry.payload.clone()).map_err(|e|SessionStoreError::Invalid(e.to_string()))?;
                    let commit = ThreadEffectBatch::decode(&payload).map_err(|e|SessionStoreError::Invalid(e.to_string()))?;
                    if commit.thread_id != id || entry.id != format!("pl.resource.thread-commit.{:020}",commit.sequence) { return Err(SessionStoreError::Invalid("commit ownership mismatch".into())); }
                    commits.push(Arc::new(commit));
                }
            }
            crate::thread::journal::legacy_migration::replay(&commits).map_err(|e|SessionStoreError::Invalid(e.to_string()))?;
        }
        tx.execute_unprepared("PRAGMA user_version=7").await?;
        tx.commit().await?;
        Ok(())
    }.await;
    match (result, db.close().await) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
        (Err(initialization), Err(cleanup)) => Err(SessionStoreError::InitializationCleanup {
            initialization: Box::new(initialization),
            cleanup: Box::new(cleanup),
        }),
    }
}

/// Migrates version 7 to 8 in one transaction, preserving entries and all history.
///
/// Schema 8 adds the per-Thread current-state checkpoint table that the effect writer fills in the
/// same transaction as the effect it belongs to (see `design/15` §15.5). The upgrade only creates
/// that table and moves the version marker; it never rewrites, truncates or re-derives existing
/// effect, entry or history rows, so an upgraded database keeps every fact it already held. Threads
/// saved before the upgrade simply report no checkpoint until they next write one. This step runs
/// only while `migrate_to_current` holds the database's exclusive lease.
///
/// # Errors
/// Rejects unsupported versions, a database missing the version-7 tables, and storage failures.
async fn migrate_v7(options: SqliteSessionOptions) -> Result<(), SessionStoreError> {
    let mut url =
        url::Url::parse("sqlite:///").map_err(|e| SessionStoreError::Invalid(e.to_string()))?;
    url.set_path(
        options
            .path
            .to_str()
            .ok_or_else(|| SessionStoreError::Invalid("non-UTF8 database path".into()))?,
    );
    url.set_query(Some("mode=rw"));
    let db = Database::connect(url.to_string()).await?;
    let result = async {
        let tx = db.begin().await?;
        let version = tx
            .query_one_raw(sqlite::statement("PRAGMA user_version", vec![]))
            .await?
            .ok_or_else(|| SessionStoreError::Invalid("missing version".into()))?
            .try_get::<i64>("", "user_version")?;
        if version == 8 || version == 9 || version == 10 {
            tx.commit().await?;
            return Ok(());
        }
        if version != 7 {
            return Err(SessionStoreError::UnsupportedSchema {
                found: version,
                supported: super::SESSION_SCHEMA_VERSION,
            });
        }
        let base = tx
            .query_one_raw(sqlite::statement(
                "SELECT COUNT(*) AS present FROM sqlite_schema WHERE type='table' AND name IN ('session_entries','session_entry_history','session_history_heads')",
                vec![],
            ))
            .await?
            .ok_or_else(|| SessionStoreError::Invalid("missing schema probe".into()))?
            .try_get::<i64>("", "present")?;
        if base != 3 {
            return Err(SessionStoreError::UnsupportedSchema {
                found: version,
                supported: super::SESSION_SCHEMA_VERSION,
            });
        }
        tx.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS thread_checkpoints (
                thread_id TEXT PRIMARY KEY, schema_version INTEGER NOT NULL,
                state_revision INTEGER NOT NULL, history_fence INTEGER NOT NULL,
                envelope TEXT NOT NULL CHECK(json_valid(envelope)), payload_hash TEXT NOT NULL);",
        )
        .await?;
        tx.execute_unprepared("PRAGMA user_version=8").await?;
        tx.commit().await?;
        Ok(())
    }
    .await;
    match (result, db.close().await) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
        (Err(initialization), Err(cleanup)) => Err(SessionStoreError::InitializationCleanup {
            initialization: Box::new(initialization),
            cleanup: Box::new(cleanup),
        }),
    }
}

/// Backfills schema-8 checkpoints from their saved effect history in one transaction.
/// The effect sequence must be contiguous through each checkpoint revision; a gap or damaged
/// envelope fails the migration without changing either the checkpoint or the version marker.
async fn migrate_v8(options: SqliteSessionOptions) -> Result<(), SessionStoreError> {
    let mut url =
        url::Url::parse("sqlite:///").map_err(|e| SessionStoreError::Invalid(e.to_string()))?;
    url.set_path(
        options
            .path
            .to_str()
            .ok_or_else(|| SessionStoreError::Invalid("non-UTF8 database path".into()))?,
    );
    url.set_query(Some("mode=rw"));
    let db = Database::connect(url.to_string()).await?;
    let result = async {
        let tx = db.begin().await?;
        let version = tx
            .query_one_raw(sqlite::statement("PRAGMA user_version", vec![]))
            .await?
            .ok_or_else(|| SessionStoreError::Invalid("missing version".into()))?
            .try_get::<i64>("", "user_version")?;
        if version == 9 || version == 10 {
            tx.commit().await?;
            return Ok(());
        }
        if version != 8 {
            return Err(SessionStoreError::UnsupportedSchema {
                found: version,
                supported: super::SESSION_SCHEMA_VERSION,
            });
        }
        let mut after: Option<String> = None;
        loop {
            let rows = match &after {
                Some(after) => {
                    tx.query_all_raw(sqlite::statement(
                        "SELECT * FROM thread_checkpoints WHERE thread_id>? ORDER BY thread_id LIMIT 128",
                        vec![after.clone().into()],
                    ))
                    .await?
                }
                None => {
                    tx.query_all_raw(sqlite::statement(
                        "SELECT * FROM thread_checkpoints ORDER BY thread_id LIMIT 128",
                        vec![],
                    ))
                    .await?
                }
            };
            if rows.is_empty() {
                break;
            }
            for row in rows {
                let thread_id: String = row.try_get("", "thread_id")?;
                after = Some(thread_id.clone());
                // A schema-8 database still stores checkpoints in the schema-1/2 attempt layout,
                // which this build no longer interprets on its normal read path. The backfill only
                // needs the usage summary and revision, so it reifies exactly those facts and writes
                // the summary back into the original envelope: every other byte, including the
                // resident attempt bodies, is preserved for the step that converts the layout.
                let envelope: String = row.try_get("", "envelope")?;
                let stored_hash: String = row.try_get("", "payload_hash")?;
                let state_revision = row.try_get::<i64>("", "state_revision")?;
                let state_revision = u64::try_from(state_revision)
                    .map_err(|_| SessionStoreError::Invalid("negative revision".into()))?;
                let history_fence = row.try_get::<i64>("", "history_fence")?;
                let history_fence = u64::try_from(history_fence)
                    .map_err(|_| SessionStoreError::Invalid("negative fence".into()))?;
                if crate::context::content_hash(envelope.as_bytes()) != stored_hash {
                    return Err(SessionStoreError::Invalid(format!(
                        "Thread {thread_id} checkpoint integrity check failed"
                    )));
                }
                let mut value: serde_json::Value = serde_json::from_str(&envelope)?;
                let header: UsageBackfillEnvelope = serde_json::from_value(value.clone())?;
                if header.thread_id != thread_id
                    || header.state_revision != state_revision
                    || header.history_fence != history_fence
                    || header.state_revision != header.state.commit_sequence
                    || !header.external_bodies.is_empty()
                    || header.state.usage_summary.applied_sequence > header.state_revision
                {
                    return Err(SessionStoreError::Invalid(format!(
                        "Thread {thread_id} checkpoint is inconsistent with its fence"
                    )));
                }
                let mut summary = header.state.usage_summary;
                let mut expected = summary.applied_sequence + 1;
                let upper = format!("pl.resource.thread-commit.{:020}", header.state_revision);
                while expected <= header.state_revision {
                    let lower = format!("pl.resource.thread-commit.{expected:020}");
                    let effects = tx
                        .query_all_raw(sqlite::statement(
                            "SELECT * FROM session_entries WHERE session_id=? AND id>=? AND id<=? ORDER BY id LIMIT 256",
                            vec![thread_id.clone().into(), lower.into(), upper.clone().into()],
                        ))
                        .await?;
                    if effects.is_empty() {
                        return Err(SessionStoreError::Invalid(format!(
                            "Thread {thread_id} usage history has a sequence gap at {expected}"
                        )));
                    }
                    for effect_row in effects {
                        let effect = thread_history::decode_effect(effect_row, &thread_id)?;
                        if effect.sequence != expected {
                            return Err(SessionStoreError::Invalid(format!(
                                "Thread {thread_id} usage history has a sequence gap at {expected}"
                            )));
                        }
                        crate::thread::usage::fold_effect(&mut summary, &effect);
                        expected += 1;
                    }
                }
                value["state"]["usageSummary"] = serde_json::to_value(&summary)?;
                let envelope = serde_json::to_string(&value)?;
                let hash = crate::context::content_hash(envelope.as_bytes());
                tx.execute_raw(sqlite::statement(
                    "UPDATE thread_checkpoints SET envelope=?,payload_hash=? WHERE thread_id=?",
                    vec![envelope.into(), hash.into(), thread_id.into()],
                ))
                .await?;
            }
        }
        tx.execute_unprepared("PRAGMA user_version=9").await?;
        tx.commit().await?;
        Ok(())
    }
    .await;
    match (result, db.close().await) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
        (Err(initialization), Err(cleanup)) => Err(SessionStoreError::InitializationCleanup {
            initialization: Box::new(initialization),
            cleanup: Box::new(cleanup),
        }),
    }
}

/// Rewrites every stored Thread checkpoint into the current lightweight-attempt schema.
///
/// The effect rows are the durable history query contract and are never touched: this step only
/// re-encodes the per-Thread current-state checkpoint rows. A schema-1/2 envelope is converted
/// through the explicit [`crate::thread::ThreadCheckpoint::decode_legacy`] path, and an
/// already-current envelope is re-encoded from its parsed state so the stored schema marker matches
/// the bytes. A legacy checkpoint whose external body manifest is not empty fails the whole step,
/// because the exact bytes belong to the owning blob store and must be materialized before the
/// checkpoint can be converted; nothing is rewritten or version-bumped on failure.
///
/// # Errors
/// Rejects an unsupported version, a damaged envelope hash, a row whose thread identity disagrees
/// with its envelope, an unmaterialized legacy body manifest, and storage failures.
async fn migrate_v9(options: SqliteSessionOptions) -> Result<(), SessionStoreError> {
    let mut url =
        url::Url::parse("sqlite:///").map_err(|e| SessionStoreError::Invalid(e.to_string()))?;
    url.set_path(
        options
            .path
            .to_str()
            .ok_or_else(|| SessionStoreError::Invalid("non-UTF8 database path".into()))?,
    );
    url.set_query(Some("mode=rw"));
    let db = Database::connect(url.to_string()).await?;
    let result = async {
        let tx = db.begin().await?;
        let version = tx
            .query_one_raw(sqlite::statement("PRAGMA user_version", vec![]))
            .await?
            .ok_or_else(|| SessionStoreError::Invalid("missing version".into()))?
            .try_get::<i64>("", "user_version")?;
        if version == 10 {
            tx.commit().await?;
            return Ok(());
        }
        if version != 9 {
            return Err(SessionStoreError::UnsupportedSchema {
                found: version,
                supported: super::SESSION_SCHEMA_VERSION,
            });
        }
        let current = i64::from(crate::thread::ThreadCheckpoint::SCHEMA_VERSION);
        let mut after = String::new();
        loop {
        let rows = tx
            .query_all_raw(sqlite::statement(
                "SELECT * FROM thread_checkpoints WHERE thread_id>? ORDER BY thread_id LIMIT 128",
                vec![after.clone().into()],
            ))
            .await?;
        if rows.is_empty() { break; }
        for row in rows {
            let thread_id: String = row.try_get("", "thread_id")?;
            after.clone_from(&thread_id);
            let stored_schema: i64 = row.try_get("", "schema_version")?;
            let envelope: String = row.try_get("", "envelope")?;
            let hash: String = row.try_get("", "payload_hash")?;
            if crate::context::content_hash(envelope.as_bytes()) != hash {
                return Err(SessionStoreError::Invalid(format!(
                    "Thread {thread_id} checkpoint integrity check failed"
                )));
            }
            let header: serde_json::Value = serde_json::from_str(&envelope)?;
            if header.get("schemaVersion").and_then(serde_json::Value::as_i64) != Some(stored_schema) {
                return Err(SessionStoreError::Invalid("checkpoint schema index/envelope mismatch".into()));
            }
            let mut checkpoint = if stored_schema == current {
                serde_json::from_str(&envelope)?
            } else {
                crate::thread::ThreadCheckpoint::decode_legacy(&envelope).map_err(|error| {
                    SessionStoreError::Invalid(format!(
                        "Thread {thread_id} legacy checkpoint conversion failed: {error}"
                    ))
                })?
            };
            if checkpoint.thread_id != thread_id {
                return Err(SessionStoreError::Invalid(
                    "Thread checkpoint ownership mismatch".into(),
                ));
            }
            if checkpoint.state_revision != u64::try_from(row.try_get::<i64>("", "state_revision")?).map_err(|_| SessionStoreError::Invalid("negative revision".into()))?
                || checkpoint.history_fence != u64::try_from(row.try_get::<i64>("", "history_fence")?).map_err(|_| SessionStoreError::Invalid("negative fence".into()))?
                || checkpoint.state_revision != checkpoint.state.commit_sequence
                || checkpoint.history_fence > checkpoint.state_revision
                || !checkpoint.external_bodies.is_empty() {
                return Err(SessionStoreError::Invalid("checkpoint index/envelope fence mismatch".into()));
            }
            // Compaction may have removed every old call from the current context. Rebuild
            // identities from the reliable history, without retaining its request/result bodies.
            let mut expected = 1;
            let upper = format!("pl.resource.thread-commit.{:020}", checkpoint.history_fence);
            while expected <= checkpoint.history_fence {
                let lower = format!("pl.resource.thread-commit.{expected:020}");
                let effects = tx.query_all_raw(sqlite::statement(
                    "SELECT * FROM session_entries WHERE session_id=? AND id>=? AND id<=? ORDER BY id LIMIT 256",
                    vec![thread_id.clone().into(), lower.into(), upper.clone().into()],
                )).await?;
                if effects.is_empty() {
                    return Err(SessionStoreError::Invalid(format!("Thread {thread_id} identity history has a sequence gap at {expected}")));
                }
                for row in effects {
                    let effect = thread_history::decode_effect(row, &thread_id)?;
                    if effect.sequence != expected {
                        return Err(SessionStoreError::Invalid(format!("Thread {thread_id} identity history has a sequence gap at {expected}")));
                    }
                    if let Some(attempt) = &effect.attempt {
                        checkpoint.state.attempt_ids.insert(attempt.attempt_id.clone(), attempt.turn_id.clone());
                        if let crate::thread::AttemptOutcome::Committed(output) = &attempt.outcome {
                            for call in output.tool_calls.iter() {
                                checkpoint.state.live_calls.insert(call.call_id.clone(), attempt.turn_id.clone());
                            }
                        }
                    }
                    expected += 1;
                }
            }
            checkpoint.schema_version = crate::thread::ThreadCheckpoint::SCHEMA_VERSION;
            checkpoint.external_bodies = Vec::new();
            let encoded = serde_json::to_string(&checkpoint)?;
            let encoded_hash = crate::context::content_hash(encoded.as_bytes());
            tx.execute_raw(sqlite::statement(
                "UPDATE thread_checkpoints SET schema_version=?,envelope=?,payload_hash=? WHERE thread_id=?",
                vec![
                    current.into(),
                    encoded.into(),
                    encoded_hash.into(),
                    thread_id.into(),
                ],
            ))
            .await?;
        }
        }
        tx.execute_unprepared("PRAGMA user_version=10").await?;
        tx.commit().await?;
        Ok(())
    }
    .await;
    match (result, db.close().await) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
        (Err(initialization), Err(cleanup)) => Err(SessionStoreError::InitializationCleanup {
            initialization: Box::new(initialization),
            cleanup: Box::new(cleanup),
        }),
    }
}

fn convert(
    entry: &mut SessionEntry,
    transform: &impl Fn(&mut ThreadEffectBatch) -> Result<(), SessionStoreError>,
) -> Result<(), SessionStoreError> {
    if entry.type_id != "pl.core.thread-commit" {
        return Ok(());
    }
    if entry.schema_version != 2 {
        return Err(SessionStoreError::Invalid(
            "unsupported migration commit version".into(),
        ));
    }
    let mut value: serde_json::Value = serde_json::from_str(&entry.payload)?;
    if let Some(state) = value.get_mut("turn").and_then(|turn| turn.get_mut("state"))
        && state.get("kind").and_then(|v| v.as_str()) == Some("finished")
        && state.get("value").and_then(|v| v.as_str()) == Some("toolCompleted")
    {
        state["value"] = "completed".into();
    }
    let mut commit: ThreadEffectBatch = serde_json::from_value(value)?;
    transform(&mut commit)?;
    entry.schema_version = 3;
    entry.payload = serde_json::to_string(&commit)?;
    Ok(())
}
