//! Task records in the existing session-entry transaction, separate from actor headers.

use std::collections::{BTreeMap, BTreeSet};

use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};
use serde::{Deserialize, Serialize};

use crate::ThreadActorState;
use crate::session::entry::SessionEntry;
use crate::session_runtime::{SessionTasks, TaskRecord, TaskResultRecord};

use super::task_results::{self, TaskResultReference};
use super::{SessionStoreError, sqlite};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredTaskRecord {
    record: TaskRecord,
    result: Option<TaskResultReference>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TaskManifest {
    revision: u64,
    task_ids: BTreeSet<String>,
}

pub(super) fn encode(state: &ThreadActorState) -> Result<Vec<SessionEntry>, SessionStoreError> {
    let session_id = state.snapshot.identity.id.as_str();
    let mut entries = Vec::new();
    let mut task_ids = BTreeSet::new();
    for (id, record) in state.session.tasks.storage_records() {
        if record.snapshot.receipt.thread_id != session_id {
            return Err(SessionStoreError::Invalid(
                "task belongs to another session".into(),
            ));
        }
        task_ids.insert(id.clone());
        let mut entry = SessionEntry {
            session_id: session_id.to_owned(),
            id: format!("pl.toolTask.{id}"),
            type_id: "pl.toolTask".into(),
            schema_version: 1,
            ordinal: entries.len() as u64,
            turn_id: Some(record.snapshot.receipt.turn_id.clone()),
            revision: state.snapshot.revision,
            created_at: record.snapshot.created_at,
            updated_at: record.snapshot.updated_at,
            payload: serde_json::Value::Null,
        };
        let mut record = record.clone();
        let result = if let Some(complete) = record.complete_result.take() {
            if let Some(content) = complete.content {
                let (reference, result_entry) = task_results::encode(&entry, (*content).clone())?;
                if reference != complete.identity {
                    return Err(SessionStoreError::Invalid(
                        "task result identity changed".into(),
                    ));
                }
                entries.push(result_entry);
            }
            Some(complete.identity)
        } else {
            None
        };
        entry.payload = serde_json::to_value(StoredTaskRecord { record, result })?;
        entries.push(entry);
    }
    entries.push(SessionEntry {
        session_id: session_id.to_owned(),
        id: "pl.taskManifest".into(),
        type_id: "pl.taskManifest".into(),
        schema_version: 1,
        ordinal: 0,
        turn_id: None,
        revision: state.snapshot.revision,
        created_at: state.snapshot.updated_at,
        updated_at: state.snapshot.updated_at,
        payload: serde_json::to_value(TaskManifest {
            revision: state.snapshot.revision,
            task_ids,
        })?,
    });
    Ok(entries)
}

pub(super) async fn restore(
    db: &impl ConnectionTrait,
    session_id: &str,
    revision: u64,
) -> Result<SessionTasks, SessionStoreError> {
    let manifest = sqlite::read::<TaskManifest>(db, session_id, "pl.taskManifest")
        .await?
        .ok_or_else(|| SessionStoreError::Invalid("missing task manifest".into()))?;
    if manifest.revision != revision {
        return Err(SessionStoreError::Invalid(
            "task manifest revision differs from actor".into(),
        ));
    }
    let mut tasks = BTreeMap::new();
    for entry in sqlite::entries(db, session_id, Some("pl.toolTask")).await? {
        let id = entry
            .id
            .strip_prefix("pl.toolTask.")
            .ok_or_else(|| SessionStoreError::Invalid("invalid task entry identity".into()))?
            .to_owned();
        let turn_id = entry.turn_id.clone();
        if entry.revision != revision {
            return Err(SessionStoreError::Invalid(
                "task entry revision differs from actor".into(),
            ));
        }
        let stored: StoredTaskRecord = sqlite::decode_builtin(entry.clone())?;
        let mut record = stored.record;
        if record.complete_result.is_some() {
            return Err(SessionStoreError::Invalid(
                "task contains embedded complete output".into(),
            ));
        }
        record.complete_result = match stored.result {
            Some(reference) => {
                let result = task_results::restore(db, &entry, &reference).await?;
                let mut full = record.snapshot.clone();
                full.result = Some(result);
                full.result_reference = None;
                let preview = crate::session_runtime::model_task_view(&full)
                    .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
                if preview != record.snapshot {
                    return Err(SessionStoreError::Invalid(
                        "task preview disagrees with complete output".into(),
                    ));
                }
                Some(TaskResultRecord {
                    identity: reference,
                    content: None,
                })
            }
            None => None,
        };
        if record.snapshot.receipt.thread_id != session_id
            || turn_id.as_deref() != Some(record.snapshot.receipt.turn_id.as_str())
        {
            return Err(SessionStoreError::Invalid(
                "task entry has inconsistent session or turn identity".into(),
            ));
        }
        if tasks.insert(id, record).is_some() {
            return Err(SessionStoreError::Invalid("duplicate task entry".into()));
        }
    }
    if tasks.keys().cloned().collect::<BTreeSet<_>>() != manifest.task_ids {
        return Err(SessionStoreError::Invalid(
            "task records disagree with manifest".into(),
        ));
    }
    SessionTasks::restore_records(tasks)
        .map_err(|error| SessionStoreError::Invalid(error.to_string()))
}

/// One-time schema migration; normal restore never falls back to embedded output.
pub(super) async fn migrate(
    db: &DatabaseConnection,
    version: i64,
) -> Result<(), SessionStoreError> {
    let tx = db.begin().await?;
    let actors = tx
        .query_all_raw(sqlite::statement(
            "SELECT * FROM session_entries WHERE type_id='pl.actor'",
            vec![],
        ))
        .await?;
    for row in actors {
        let mut actor = sqlite::decode_row(row)?;
        // Only the migration boundary reads the old embedded-task layout.
        let mut payload = actor.payload.clone();
        let legacy_tasks = payload
            .get_mut("session")
            .and_then(|session| session.get_mut("tasks"))
            .map(|tasks| std::mem::replace(tasks, serde_json::json!({"entries": {}})))
            .unwrap_or_else(|| serde_json::json!({"entries": {}}));
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct LegacyTasks {
            entries: BTreeMap<String, TaskRecord>,
        }
        let embedded: LegacyTasks = serde_json::from_value(legacy_tasks)?;
        let mut state: ThreadActorState = serde_json::from_value(payload)?;
        if state.snapshot.identity.id.as_str() != actor.session_id {
            return Err(SessionStoreError::Invalid(
                "actor belongs to another session".into(),
            ));
        }
        let mut tasks = embedded.entries;
        if version >= 2 {
            if !tasks.is_empty() {
                return Err(SessionStoreError::Invalid(
                    "actor contains embedded tasks".into(),
                ));
            }
            for entry in sqlite::entries(&tx, &actor.session_id, Some("pl.toolTask")).await? {
                let mut record: TaskRecord = if version == 2 {
                    sqlite::decode_builtin(entry.clone())?
                } else {
                    let stored: StoredTaskRecord = sqlite::decode_builtin(entry.clone())?;
                    let mut record = stored.record;
                    if record.snapshot.result.is_some() || record.complete_result.is_some() {
                        return Err(SessionStoreError::Invalid(
                            "schema 3 task contains embedded output".into(),
                        ));
                    }
                    record.snapshot.result = match stored.result {
                        Some(reference) => {
                            Some(task_results::restore(&tx, &entry, &reference).await?)
                        }
                        None => None,
                    };
                    record
                };
                let id = record.snapshot.receipt.task_id.clone();
                if entry.id != format!("pl.toolTask.{id}")
                    || entry.revision != state.snapshot.revision
                    || entry.turn_id.as_deref() != Some(record.snapshot.receipt.turn_id.as_str())
                {
                    return Err(SessionStoreError::Invalid(
                        "task migration identity or revision mismatch".into(),
                    ));
                }
                upgrade_result(&mut record)?;
                if tasks.insert(id, record).is_some() {
                    return Err(SessionStoreError::Invalid(
                        "duplicate task during migration".into(),
                    ));
                }
            }
            let manifest = sqlite::read::<TaskManifest>(&tx, &actor.session_id, "pl.taskManifest")
                .await?
                .ok_or_else(|| {
                    SessionStoreError::Invalid("missing task manifest during migration".into())
                })?;
            if manifest.revision != state.snapshot.revision
                || tasks.keys().cloned().collect::<BTreeSet<_>>() != manifest.task_ids
            {
                return Err(SessionStoreError::Invalid(
                    "task migration manifest mismatch".into(),
                ));
            }
        } else {
            for record in tasks.values_mut() {
                upgrade_result(record)?;
            }
        }
        state.session.tasks = SessionTasks::restore_records(tasks)
            .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
        state
            .session
            .inbox
            .migrate_task_previews()
            .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
        state
            .session
            .inbox
            .validate_task_references(&state.session.tasks)
            .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
        for entry in encode(&state)? {
            sqlite::put(&tx, &entry).await?;
        }
        state.session.tasks = SessionTasks::default();
        actor.payload = serde_json::to_value(state)?;
        sqlite::put(&tx, &actor).await?;
    }
    tx.execute_unprepared("PRAGMA user_version=4").await?;
    tx.commit().await?;
    Ok(())
}

fn upgrade_result(record: &mut TaskRecord) -> Result<(), SessionStoreError> {
    if record.complete_result.is_some() || record.snapshot.result_reference.is_some() {
        return Err(SessionStoreError::Invalid(
            "unexpected result reference in legacy task".into(),
        ));
    }
    if let Some(result) = &record.snapshot.result {
        record.complete_result = Some(TaskResultRecord::new(
            &record.snapshot.receipt.task_id,
            result.clone(),
        )?);
        record.snapshot = crate::session_runtime::model_task_view(&record.snapshot)
            .map_err(|error| SessionStoreError::Invalid(error.to_string()))?;
    }
    Ok(())
}

pub(super) async fn read_result(
    db: &impl ConnectionTrait,
    session_id: &str,
    task_id: &str,
) -> Result<Option<crate::session_runtime::ToolTaskResult>, SessionStoreError> {
    let row = db
        .query_one_raw(sqlite::statement(
            "SELECT * FROM session_entries WHERE session_id=? AND id=?",
            vec![
                session_id.to_owned().into(),
                format!("pl.toolTask.{task_id}").into(),
            ],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let entry = sqlite::decode_row(row)?;
    if entry.type_id != "pl.toolTask" {
        return Err(SessionStoreError::Invalid("invalid task entry type".into()));
    }
    let stored: StoredTaskRecord = sqlite::decode_builtin(entry.clone())?;
    if stored.record.snapshot.receipt.thread_id != session_id
        || stored.record.snapshot.receipt.task_id != task_id
    {
        return Err(SessionStoreError::Invalid(
            "task result ownership mismatch".into(),
        ));
    }
    match stored.result {
        Some(reference) => task_results::restore(db, &entry, &reference)
            .await
            .map(Some),
        None => Ok(None),
    }
}
