//! Explicit offline migration. Normal store opening never resets or converts history.
use super::{SessionStoreError, SqliteSessionOptions, history, sqlite};
use crate::{
    context::OpaquePayload,
    storage::{SessionEntry, SessionEntryChange},
    thread::journal::ThreadCommit,
};
use sea_orm::{ConnectionTrait, Database, TransactionTrait};
use std::sync::Arc;

/// Migrates version 6 to 7 in one transaction, preserving entry identities and all history.
/// The host must hold exclusive runtime/database ownership and make a consistent backup first.
/// The callback converts only product-owned payloads; it must not perform external side effects.
/// # Errors
/// Rejects unsupported versions, damaged history, conversion failures and storage failures.
pub async fn migrate_v6(
    options: SqliteSessionOptions,
    transform: impl Fn(&mut ThreadCommit) -> Result<(), SessionStoreError> + Send + Sync,
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
        if version == super::SESSION_SCHEMA_VERSION { tx.commit().await?; return Ok(()); }
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
                    let commit = ThreadCommit::decode(&payload).map_err(|e|SessionStoreError::Invalid(e.to_string()))?;
                    if commit.thread_id != id || entry.id != format!("pl.resource.thread-commit.{:020}",commit.sequence) { return Err(SessionStoreError::Invalid("commit ownership mismatch".into())); }
                    commits.push(Arc::new(commit));
                }
            }
            crate::thread::journal::replay(&commits).map_err(|e|SessionStoreError::Invalid(e.to_string()))?;
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

fn convert(
    entry: &mut SessionEntry,
    transform: &impl Fn(&mut ThreadCommit) -> Result<(), SessionStoreError>,
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
    let mut commit: ThreadCommit = serde_json::from_value(value)?;
    transform(&mut commit)?;
    entry.schema_version = 3;
    entry.payload = serde_json::to_string(&commit)?;
    Ok(())
}
