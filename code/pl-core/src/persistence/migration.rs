//! Explicit offline migration. Normal store opening never resets or converts history.
use super::{
    SessionStoreError, SqliteSessionOptions, history, sqlite, thread_checkpoint, thread_history,
};
use crate::{
    context::OpaquePayload,
    storage::{SessionEntry, SessionEntryChange},
    thread::ThreadEffectBatch,
};
use sea_orm::{ConnectionTrait, Database, TransactionTrait};
use std::sync::Arc;

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
    migrate_v8(options).await
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
        if version == 7 || version == 8 || version == 9 { tx.commit().await?; return Ok(()); }
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
        if version == 8 || version == 9 {
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
        if version == 9 {
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
                let mut checkpoint = thread_checkpoint::decode_checkpoint(row, &thread_id)?;
                let mut expected = checkpoint.state.usage_summary.applied_sequence + 1;
                let upper = format!(
                    "pl.resource.thread-commit.{:020}",
                    checkpoint.state_revision
                );
                while expected <= checkpoint.state_revision {
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
                    for row in effects {
                        let effect = thread_history::decode_effect(row, &thread_id)?;
                        if effect.sequence != expected {
                            return Err(SessionStoreError::Invalid(format!(
                                "Thread {thread_id} usage history has a sequence gap at {expected}"
                            )));
                        }
                        crate::thread::usage::fold_effect(
                            &mut checkpoint.state.usage_summary,
                            &effect,
                        );
                        expected += 1;
                    }
                }
                let envelope = serde_json::to_string(&checkpoint)?;
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
