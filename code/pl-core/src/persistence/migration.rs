//! Explicit offline migration. Normal store opening never resets or converts history.
use super::{SessionStoreError, SqliteSessionOptions, history, sqlite};
use crate::{
    context::OpaquePayload,
    storage::{SessionEntry, SessionEntryChange},
    thread::journal::ThreadCommit,
};
use sea_orm::{ConnectionTrait, Database, TransactionTrait};
use std::path::Path;
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

/// Verification receipt for one session copied or inspected during a layout migration.
///
/// Counts and the digest are derived from the immutable `envelope`/`payload_hash`
/// bytes and index metadata only, so equals-diff proves byte-for-byte preservation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCopyReport {
    pub entries: u64,
    pub history: u64,
    pub head_sequence: Option<u64>,
    pub digest: String,
}

/// Lists the durable session identities in one session database without activating any actor.
///
/// Works for both a legacy aggregate database and a per-Thread database; it never rewrites data.
///
/// # Errors
/// Rejects an unsupported schema or a storage failure. Existing data is preserved.
pub async fn session_ids(path: &Path) -> Result<Vec<String>, SessionStoreError> {
    let db = connect(path, true).await?;
    let result = async {
        let rows = db
            .query_all_raw(sqlite::statement(
                "SELECT session_id FROM session_entries \
                 UNION SELECT session_id FROM session_entry_history \
                 UNION SELECT session_id FROM session_history_heads \
                 ORDER BY session_id",
                vec![],
            ))
            .await?;
        rows.into_iter()
            .map(|row| Ok(row.try_get::<String>("", "session_id")?))
            .collect()
    }
    .await;
    finish(db, result).await
}

/// Streams one session's current entries, history and head from `source` into `target`.
///
/// The encoded `envelope`, `payload_hash` and index metadata (`ordinal`, `turn_id`,
/// sequence, timestamps inside the envelope) are copied verbatim: no projection or
/// journal export replaces historical revisions. `target` is created with the current
/// session schema, must not already hold this session, and is verified by digest.
///
/// # Errors
/// Rejects unsupported source/target versions, corrupt or mismatched source records,
/// a non-empty target, and any copy whose digest does not equal the source digest.
pub async fn copy_session(
    source: &Path,
    target: &Path,
    session_id: &str,
) -> Result<SessionCopyReport, SessionStoreError> {
    let source_db = connect(source, true).await?;
    let target_db = connect(target, false).await?;
    let result = copy_session_inner(&source_db, &target_db, session_id).await;
    let target_close = target_db.close().await;
    match (result, source_db.close().await, target_close) {
        (Ok(report), Ok(()), Ok(())) => Ok(report),
        (Err(error), _, _) => Err(error),
        (Ok(_), Err(error), _) | (Ok(_), Ok(()), Err(error)) => Err(error.into()),
    }
}

/// Recomputes the verification receipt of a session already stored in `path`.
///
/// # Errors
/// Rejects an unsupported schema, a corrupt record, or a storage failure.
pub async fn verify_session(
    path: &Path,
    session_id: &str,
) -> Result<SessionCopyReport, SessionStoreError> {
    let db = connect(path, true).await?;
    let result = measure(&db, session_id).await;
    finish(db, result).await
}

/// Ordered content fingerprint over every session in a session database.
///
/// Unlike a size/time fingerprint it detects same-length content changes, and it can
/// bind a consistent backup to the exact source it was taken from.
///
/// # Errors
/// Rejects an unsupported schema, corrupt records, or a storage failure.
pub async fn database_fingerprint(path: &Path) -> Result<String, SessionStoreError> {
    let db = connect(path, true).await?;
    let result = async {
        let rows = db
            .query_all_raw(sqlite::statement(
                "SELECT session_id FROM session_entries \
                 UNION SELECT session_id FROM session_entry_history \
                 UNION SELECT session_id FROM session_history_heads \
                 ORDER BY session_id",
                vec![],
            ))
            .await?;
        let mut canonical = String::new();
        for row in rows {
            let id: String = row.try_get("", "session_id")?;
            let report = measure(&db, &id).await?;
            canonical.push_str(&format!(
                "{id}|{}|{}|{:?}|{}\n",
                report.entries, report.history, report.head_sequence, report.digest
            ));
        }
        Ok(crate::context::content_hash(canonical.as_bytes()))
    }
    .await;
    finish(db, result).await
}

async fn copy_session_inner(
    source: &sea_orm::DatabaseConnection,
    target: &sea_orm::DatabaseConnection,
    session_id: &str,
) -> Result<SessionCopyReport, SessionStoreError> {
    // The whole migration target database must be empty, not merely free of this session.
    if count_rows(target).await? != (0, 0, 0) {
        return Err(SessionStoreError::Invalid(
            "layout migration target database is not empty".into(),
        ));
    }
    // Full structural validation of the source session before any bytes are copied.
    validate_session(source, session_id).await?;
    let entries = source
        .query_all_raw(sqlite::statement(
            "SELECT id,type_id,ordinal,turn_id,envelope,payload_hash FROM session_entries \
             WHERE session_id=? ORDER BY id",
            vec![session_id.into()],
        ))
        .await?;
    let history = source
        .query_all_raw(sqlite::statement(
            "SELECT sequence,envelope,payload_hash FROM session_entry_history \
             WHERE session_id=? ORDER BY sequence",
            vec![session_id.into()],
        ))
        .await?;
    let head = source
        .query_one_raw(sqlite::statement(
            "SELECT sequence,payload_hash FROM session_history_heads WHERE session_id=?",
            vec![session_id.into()],
        ))
        .await?;

    let tx = target.begin().await?;
    for row in &entries {
        let id: String = row.try_get("", "id")?;
        let type_id: String = row.try_get("", "type_id")?;
        let ordinal: i64 = row.try_get("", "ordinal")?;
        let turn_id: Option<String> = row.try_get("", "turn_id")?;
        let envelope: String = row.try_get("", "envelope")?;
        let payload_hash: String = row.try_get("", "payload_hash")?;
        validate_entry(session_id, &id, &type_id, &envelope, &payload_hash)?;
        tx.execute_raw(sqlite::statement(
            "INSERT INTO session_entries(session_id,id,type_id,ordinal,turn_id,envelope,payload_hash) \
             VALUES(?,?,?,?,?,?,?)",
            vec![
                session_id.into(),
                id.into(),
                type_id.into(),
                ordinal.into(),
                turn_id.into(),
                envelope.into(),
                payload_hash.into(),
            ],
        ))
        .await?;
    }
    for row in &history {
        let sequence: i64 = row.try_get("", "sequence")?;
        let envelope: String = row.try_get("", "envelope")?;
        let payload_hash: String = row.try_get("", "payload_hash")?;
        if crate::context::content_hash(envelope.as_bytes()) != payload_hash {
            return Err(SessionStoreError::Invalid(
                "history content digest mismatch".into(),
            ));
        }
        tx.execute_raw(sqlite::statement(
            "INSERT INTO session_entry_history(session_id,sequence,envelope,payload_hash) VALUES(?,?,?,?)",
            vec![session_id.into(), sequence.into(), envelope.into(), payload_hash.into()],
        ))
        .await?;
    }
    if let Some(row) = head {
        let sequence: i64 = row.try_get("", "sequence")?;
        let payload_hash: String = row.try_get("", "payload_hash")?;
        tx.execute_raw(sqlite::statement(
            "INSERT INTO session_history_heads(session_id,sequence,payload_hash) VALUES(?,?,?)",
            vec![session_id.into(), sequence.into(), payload_hash.into()],
        ))
        .await?;
    }
    tx.commit().await?;

    // Full structural validation of the copied session; a hash-correct but structurally
    // wrong, truncated, head-only or mismatched copy is rejected here.
    validate_session(target, session_id).await?;
    let source_report = measure(source, session_id).await?;
    let target_report = measure(target, session_id).await?;
    if source_report != target_report {
        return Err(SessionStoreError::Invalid(format!(
            "session {session_id} copy digest mismatch during layout migration"
        )));
    }
    Ok(target_report)
}

/// Total row counts across every session; a migration target must be entirely empty.
async fn count_rows(db: &impl ConnectionTrait) -> Result<(u64, u64, u64), SessionStoreError> {
    let row = db
        .query_one_raw(sqlite::statement(
            "SELECT (SELECT COUNT(*) FROM session_entries) AS entries, \
             (SELECT COUNT(*) FROM session_entry_history) AS history, \
             (SELECT COUNT(*) FROM session_history_heads) AS heads",
            vec![],
        ))
        .await?
        .ok_or_else(|| SessionStoreError::Invalid("missing migration row count".into()))?;
    let count = |value: i64| {
        u64::try_from(value)
            .map_err(|_| SessionStoreError::Invalid("negative migration row count".into()))
    };
    Ok((
        count(row.try_get::<i64>("", "entries")?)?,
        count(row.try_get::<i64>("", "history")?)?,
        count(row.try_get::<i64>("", "heads")?)?,
    ))
}

/// Full structural validation of one session using core's own committed-history validators:
/// envelope integrity and ownership, history sequence continuity and head digest, and
/// `current == replay` (so a truncated tail, head-only or mismatched session is rejected).
async fn validate_session(
    db: &impl ConnectionTrait,
    session_id: &str,
) -> Result<(), SessionStoreError> {
    let history = history::read(db, session_id, None).await?;
    let mut current = sqlite::entries(db, session_id, None).await?;
    current.sort_by(|left, right| left.id.cmp(&right.id));
    let mut replay = crate::storage::replay_entries(&history)?;
    replay.sort_by(|left, right| left.id.cmp(&right.id));
    if replay != current {
        return Err(SessionStoreError::Invalid(format!(
            "session {session_id} history and current entries disagree"
        )));
    }
    let mut commits = Vec::new();
    for entry in &current {
        if entry.id.starts_with("pl.resource.thread-commit.") {
            let payload = OpaquePayload::new(
                entry.type_id.clone(),
                entry.schema_version,
                entry.payload.clone(),
            )
            .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
            let commit = ThreadCommit::decode(&payload)
                .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
            if commit.thread_id != session_id
                || entry.id != format!("pl.resource.thread-commit.{:020}", commit.sequence)
            {
                return Err(SessionStoreError::Invalid(
                    "Thread commit ownership or sequence key mismatch".into(),
                ));
            }
            commits.push(Arc::new(commit));
        }
    }
    crate::thread::journal::replay(&commits)
        .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
    Ok(())
}

async fn measure(
    db: &impl ConnectionTrait,
    session_id: &str,
) -> Result<SessionCopyReport, SessionStoreError> {
    let mut canonical = String::new();
    let entries = db
        .query_all_raw(sqlite::statement(
            "SELECT id,type_id,ordinal,turn_id,envelope,payload_hash FROM session_entries \
             WHERE session_id=? ORDER BY id",
            vec![session_id.into()],
        ))
        .await?;
    for row in &entries {
        let id: String = row.try_get("", "id")?;
        let type_id: String = row.try_get("", "type_id")?;
        let ordinal: i64 = row.try_get("", "ordinal")?;
        let turn_id: Option<String> = row.try_get("", "turn_id")?;
        let envelope: String = row.try_get("", "envelope")?;
        let payload_hash: String = row.try_get("", "payload_hash")?;
        validate_entry(session_id, &id, &type_id, &envelope, &payload_hash)?;
        canonical.push_str(&format!(
            "e|{id}|{type_id}|{ordinal}|{turn_id:?}|{envelope}|{payload_hash}\n"
        ));
    }
    let history = db
        .query_all_raw(sqlite::statement(
            "SELECT sequence,envelope,payload_hash FROM session_entry_history \
             WHERE session_id=? ORDER BY sequence",
            vec![session_id.into()],
        ))
        .await?;
    for row in &history {
        let sequence: i64 = row.try_get("", "sequence")?;
        let envelope: String = row.try_get("", "envelope")?;
        let payload_hash: String = row.try_get("", "payload_hash")?;
        if crate::context::content_hash(envelope.as_bytes()) != payload_hash {
            return Err(SessionStoreError::Invalid(
                "history content digest mismatch".into(),
            ));
        }
        canonical.push_str(&format!("h|{sequence}|{envelope}|{payload_hash}\n"));
    }
    let head = db
        .query_one_raw(sqlite::statement(
            "SELECT sequence,payload_hash FROM session_history_heads WHERE session_id=?",
            vec![session_id.into()],
        ))
        .await?;
    let head_sequence = match head {
        Some(row) => {
            let sequence: i64 = row.try_get("", "sequence")?;
            let payload_hash: String = row.try_get("", "payload_hash")?;
            canonical.push_str(&format!("s|{sequence}|{payload_hash}\n"));
            Some(
                u64::try_from(sequence)
                    .map_err(|_| SessionStoreError::Invalid("negative history head".into()))?,
            )
        }
        None => None,
    };
    Ok(SessionCopyReport {
        entries: u64::try_from(entries.len())
            .map_err(|_| SessionStoreError::Invalid("entry count overflow".into()))?,
        history: u64::try_from(history.len())
            .map_err(|_| SessionStoreError::Invalid("history count overflow".into()))?,
        head_sequence,
        digest: crate::context::content_hash(canonical.as_bytes()),
    })
}

fn validate_entry(
    session_id: &str,
    id: &str,
    type_id: &str,
    envelope: &str,
    payload_hash: &str,
) -> Result<(), SessionStoreError> {
    if crate::context::content_hash(envelope.as_bytes()) != payload_hash {
        return Err(SessionStoreError::Invalid(
            "session entry integrity check failed".into(),
        ));
    }
    let entry: SessionEntry = serde_json::from_str(envelope)?;
    if entry.session_id != session_id
        || entry.id != id
        || entry.type_id != type_id
        || entry.schema_version == 0
    {
        return Err(SessionStoreError::Invalid(
            "session entry envelope/index mismatch".into(),
        ));
    }
    Ok(())
}

async fn connect(
    path: &Path,
    require_existing: bool,
) -> Result<sea_orm::DatabaseConnection, SessionStoreError> {
    sqlite::open(
        Some(SqliteSessionOptions {
            path: path.to_path_buf(),
        }),
        require_existing,
    )
    .await
}

async fn finish<T>(
    db: sea_orm::DatabaseConnection,
    result: Result<T, SessionStoreError>,
) -> Result<T, SessionStoreError> {
    match (result, db.close().await) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
    }
}
