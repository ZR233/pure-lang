//! Immutable task results referenced by mutable task-state entries.

use sea_orm::ConnectionTrait;

use crate::session::entry::SessionEntry;
use crate::session_runtime::ToolTaskResult;

use super::{SessionStoreError, sqlite};

pub(super) use crate::session_runtime::TaskResultIdentity as TaskResultReference;

pub(super) fn encode(
    task: &SessionEntry,
    result: ToolTaskResult,
) -> Result<(TaskResultReference, SessionEntry), SessionStoreError> {
    let task_id = task
        .id
        .strip_prefix("pl.toolTask.")
        .ok_or_else(|| SessionStoreError::Invalid("invalid result task identity".into()))?;
    let payload = serde_json::to_value(result)?;
    let id = format!("pl.toolResult.{task_id}");
    let reference = TaskResultReference {
        entry_id: id.clone(),
        content_hash: crate::canonical_json_hash(&payload),
        encoded_bytes: serde_json::to_vec(&payload)?.len() as u64,
    };
    let mut entry = task.clone();
    entry.id = id;
    entry.type_id = "pl.toolResult".into();
    entry.payload = payload;
    Ok((reference, entry))
}

pub(super) async fn restore(
    db: &impl ConnectionTrait,
    task: &SessionEntry,
    reference: &TaskResultReference,
) -> Result<ToolTaskResult, SessionStoreError> {
    let task_id = task
        .id
        .strip_prefix("pl.toolTask.")
        .ok_or_else(|| SessionStoreError::Invalid("invalid result task identity".into()))?;
    if reference.entry_id != format!("pl.toolResult.{task_id}") {
        return Err(SessionStoreError::Invalid(
            "result reference belongs to another task".into(),
        ));
    }
    let row = db
        .query_one_raw(sqlite::statement(
            "SELECT * FROM session_entries WHERE session_id=? AND id=?",
            vec![
                task.session_id.clone().into(),
                reference.entry_id.clone().into(),
            ],
        ))
        .await?
        .ok_or_else(|| SessionStoreError::Invalid("missing referenced tool result".into()))?;
    let entry = sqlite::decode_row(row)?;
    if entry.type_id != "pl.toolResult"
        || entry.turn_id != task.turn_id
        || entry.revision > task.revision
        || crate::canonical_json_hash(&entry.payload) != reference.content_hash
        || serde_json::to_vec(&entry.payload)?.len() as u64 != reference.encoded_bytes
    {
        return Err(SessionStoreError::Invalid(
            "tool result reference integrity check failed".into(),
        ));
    }
    sqlite::decode_builtin(entry)
}
