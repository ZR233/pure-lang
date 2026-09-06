use pl_core::{
    AgentSession, ThreadCommit, ThreadContextMutation, canonical_content_hash, canonical_json_hash,
};
use pl_protocol::{AgentSessionSnapshot, AgentWorkingState, ModelContextItem};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder,
};

use crate::PureError;
use crate::studio::StudioStore;
use crate::studio::entity::thread_context_segment;
use crate::studio::store::object::{decode_object, load_object, load_object_row, put_object};

use super::{i64_from_u64, store_error};

pub(super) enum SessionSnapshotAuditError {
    Fatal(PureError),
    Corrupt(PureError),
}

pub(super) async fn audit_session_snapshot(
    store: &StudioStore,
    thread_id: &str,
) -> Result<(), SessionSnapshotAuditError> {
    let working_state_row = load_object_row::<AgentWorkingState>(store.database(), thread_id)
        .await
        .map_err(|error| SessionSnapshotAuditError::Fatal(store_error(error)))?
        .ok_or_else(|| {
            SessionSnapshotAuditError::Corrupt(missing_working_state_error(thread_id))
        })?;
    decode_object::<AgentWorkingState>(working_state_row)
        .map_err(|error| SessionSnapshotAuditError::Corrupt(store_error(error)))?;
    let rows = thread_context_segment::Entity::find()
        .filter(thread_context_segment::Column::ThreadId.eq(thread_id))
        .order_by_asc(thread_context_segment::Column::Ordinal)
        .all(store.database())
        .await
        .map_err(|error| SessionSnapshotAuditError::Fatal(store_error(error)))?;
    fold_transcript(thread_id, rows).map_err(SessionSnapshotAuditError::Corrupt)?;
    Ok(())
}

pub(super) async fn restore_session_snapshot(
    store: &StudioStore,
    thread_id: &str,
) -> Result<AgentSessionSnapshot, PureError> {
    let working_state = restore_working_state(store.database(), thread_id).await?;
    let transcript = restore_transcript(store.database(), thread_id).await?;
    Ok(AgentSessionSnapshot {
        transcript,
        working_state,
    })
}

pub(super) async fn persist_session_snapshot(
    tx: &sea_orm::DatabaseTransaction,
    commit: &ThreadCommit,
) -> Result<(), PureError> {
    let snapshot = commit.next_state.session.session.snapshot();
    persist_working_state(
        tx,
        commit.facts.thread_id.as_str(),
        &snapshot.working_state,
        commit.next_state.snapshot.updated_at,
    )
    .await?;
    let Some(mutation) = commit.facts.context.as_ref() else {
        return Ok(());
    };
    persist_transcript_mutation(
        tx,
        commit.facts.thread_id.as_str(),
        commit.facts.revision,
        commit.next_state.snapshot.updated_at,
        mutation,
        &snapshot.transcript,
    )
    .await
}

pub(super) fn serialize_thread_metadata(
    metadata: &pl_core::ThreadContextMetadata,
    _session: &AgentSession,
) -> Result<String, PureError> {
    serde_json::to_string(metadata).map_err(Into::into)
}

async fn restore_working_state(
    db: &impl ConnectionTrait,
    thread_id: &str,
) -> Result<AgentWorkingState, PureError> {
    load_object::<AgentWorkingState>(db, thread_id)
        .await
        .map_err(store_error)?
        .ok_or_else(|| missing_working_state_error(thread_id))
}

fn missing_working_state_error(thread_id: &str) -> PureError {
    store_error(format!("Thread {thread_id} session state is missing"))
}

pub(in crate::studio::agent_host::repository) async fn restore_transcript(
    db: &impl ConnectionTrait,
    thread_id: &str,
) -> Result<Vec<ModelContextItem>, PureError> {
    let rows = thread_context_segment::Entity::find()
        .filter(thread_context_segment::Column::ThreadId.eq(thread_id))
        .order_by_asc(thread_context_segment::Column::Ordinal)
        .all(db)
        .await
        .map_err(store_error)?;
    fold_transcript(thread_id, rows)
}

fn fold_transcript(
    thread_id: &str,
    rows: Vec<thread_context_segment::Model>,
) -> Result<Vec<ModelContextItem>, PureError> {
    let mut transcript = Vec::new();
    let mut previous_revision = None;
    for (expected_ordinal, row) in rows.into_iter().enumerate() {
        let expected_ordinal = i64::try_from(expected_ordinal)
            .map_err(|_| store_error("context segment ordinal exceeds SQLite range"))?;
        if row.ordinal != expected_ordinal {
            return Err(store_error(format!(
                "Thread {thread_id} context segment gap: expected {expected_ordinal}, got {}",
                row.ordinal
            )));
        }
        if previous_revision.is_some_and(|revision| row.revision <= revision) {
            return Err(store_error(format!(
                "Thread {thread_id} context segment revisions are not strictly increasing"
            )));
        }
        previous_revision = Some(row.revision);
        verify_hash(
            "context segment",
            &row.id,
            &row.payload_json,
            &row.payload_hash,
        )?;
        let items = serde_json::from_str::<Vec<ModelContextItem>>(&row.payload_json)?;
        match row.kind.as_str() {
            "append" => transcript.extend(items),
            "replace" if row.ordinal == 0 => transcript = items,
            "replace" => {
                return Err(store_error(format!(
                    "Thread {thread_id} replace segment must be the baseline"
                )));
            }
            kind => {
                return Err(store_error(format!(
                    "Thread {thread_id} has unknown context segment kind {kind}"
                )));
            }
        }
        let resulting_value = serde_json::to_value(&transcript)?;
        verify_canonical_json_hash(
            "context result",
            &row.id,
            &resulting_value,
            &row.resulting_hash,
        )?;
    }
    Ok(transcript)
}

async fn persist_working_state(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
    state: &AgentWorkingState,
    updated_at: i64,
) -> Result<(), PureError> {
    put_object(tx, thread_id, state, updated_at)
        .await
        .map_err(store_error)
}

async fn persist_transcript_mutation(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
    revision: u64,
    created_at: i64,
    mutation: &ThreadContextMutation,
    resulting_transcript: &[ModelContextItem],
) -> Result<(), PureError> {
    let persisted = restore_transcript(tx, thread_id).await?;
    let (kind, payload, ordinal) = match mutation {
        ThreadContextMutation::Append { items } => {
            let expected = resulting_transcript
                .strip_prefix(persisted.as_slice())
                .ok_or_else(|| store_error("append transcript does not extend persisted state"))?;
            if expected != items {
                return Err(store_error(
                    "append transcript payload does not match the new suffix",
                ));
            }
            if items.is_empty() {
                return Ok(());
            }
            (
                "append",
                items.as_slice(),
                i64::try_from(segment_count(tx, thread_id).await?)
                    .map_err(|_| store_error("context segment ordinal exceeds SQLite range"))?,
            )
        }
        ThreadContextMutation::Replace { items } => {
            if items != resulting_transcript {
                return Err(store_error(
                    "replace transcript payload does not match the canonical snapshot",
                ));
            }
            thread_context_segment::Entity::delete_many()
                .filter(thread_context_segment::Column::ThreadId.eq(thread_id))
                .exec(tx)
                .await
                .map_err(store_error)?;
            if items.is_empty() {
                return Ok(());
            }
            ("replace", items.as_slice(), 0)
        }
    };
    let payload_json = serde_json::to_string(payload)?;
    let resulting_value = serde_json::to_value(resulting_transcript)?;
    let revision = i64_from_u64(revision)?;
    thread_context_segment::ActiveModel {
        id: Set(format!("context:{thread_id}:{revision}")),
        thread_id: Set(thread_id.to_string()),
        ordinal: Set(ordinal),
        revision: Set(revision),
        kind: Set(kind.to_string()),
        payload_hash: Set(canonical_content_hash(payload_json.as_bytes())),
        payload_json: Set(payload_json),
        resulting_hash: Set(canonical_json_hash(&resulting_value)),
        created_at: Set(created_at),
    }
    .insert(tx)
    .await
    .map_err(store_error)?;
    Ok(())
}

async fn segment_count(db: &impl ConnectionTrait, thread_id: &str) -> Result<u64, PureError> {
    thread_context_segment::Entity::find()
        .filter(thread_context_segment::Column::ThreadId.eq(thread_id))
        .count(db)
        .await
        .map_err(store_error)
}

fn verify_hash(kind: &str, id: &str, value: &str, expected: &str) -> Result<(), PureError> {
    let actual = canonical_content_hash(value.as_bytes());
    if actual == expected {
        return Ok(());
    }
    Err(store_error(format!(
        "{kind} {id} hash mismatch: expected {expected}, got {actual}"
    )))
}

fn verify_canonical_json_hash(
    kind: &str,
    id: &str,
    value: &serde_json::Value,
    expected: &str,
) -> Result<(), PureError> {
    let actual = canonical_json_hash(value);
    if actual == expected {
        return Ok(());
    }
    Err(store_error(format!(
        "{kind} {id} hash mismatch: expected {expected}, got {actual}"
    )))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pl_protocol::{Message, MessageContent, MessageRole};
    use sea_orm::{ActiveModelTrait, IntoActiveModel, TransactionTrait};

    use super::*;
    use crate::ThreadModeId;
    use crate::studio::entity::studio_object;

    #[tokio::test]
    async fn transcript_segments_append_only_suffix_and_replace_after_compaction() {
        let (store, thread_id) = store_with_thread("context-segments").await;
        let tx = store.database().begin().await.unwrap();
        persist_working_state(&tx, &thread_id, &AgentWorkingState::default(), 1)
            .await
            .unwrap();

        let mut transcript = vec![text_item("initial")];
        persist_transcript_mutation(
            &tx,
            &thread_id,
            1,
            1,
            &ThreadContextMutation::Replace {
                items: transcript.clone(),
            },
            &transcript,
        )
        .await
        .unwrap();
        for index in 0..100 {
            let item = text_item(&format!("append-{index}"));
            transcript.push(item.clone());
            persist_transcript_mutation(
                &tx,
                &thread_id,
                index + 2,
                index as i64 + 2,
                &ThreadContextMutation::Append { items: vec![item] },
                &transcript,
            )
            .await
            .unwrap();
        }
        persist_transcript_mutation(
            &tx,
            &thread_id,
            102,
            102,
            &ThreadContextMutation::Append { items: Vec::new() },
            &transcript,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(
            segment_count(store.database(), &thread_id).await.unwrap(),
            101
        );
        assert_eq!(
            restore_transcript(store.database(), &thread_id)
                .await
                .unwrap(),
            transcript
        );

        let compacted = vec![text_item("compacted baseline")];
        let tx = store.database().begin().await.unwrap();
        persist_transcript_mutation(
            &tx,
            &thread_id,
            103,
            103,
            &ThreadContextMutation::Replace {
                items: compacted.clone(),
            },
            &compacted,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(
            segment_count(store.database(), &thread_id).await.unwrap(),
            1
        );
        assert_eq!(
            restore_transcript(store.database(), &thread_id)
                .await
                .unwrap(),
            compacted
        );

        let row = thread_context_segment::Entity::find()
            .filter(thread_context_segment::Column::ThreadId.eq(&thread_id))
            .one(store.database())
            .await
            .unwrap()
            .unwrap();
        let mut active = row.into_active_model();
        active.resulting_hash = Set("sha256:corrupt".to_string());
        active.update(store.database()).await.unwrap();
        let error = restore_transcript(store.database(), &thread_id)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("hash mismatch"));
    }

    #[tokio::test]
    async fn transcript_result_hash_is_stable_across_metadata_map_order() {
        let (store, thread_id) = store_with_thread("context-canonical-hash").await;
        let mut metadata = HashMap::new();
        metadata.insert("zeta".to_string(), "2".to_string());
        metadata.insert("alpha".to_string(), "1".to_string());
        let transcript = vec![ModelContextItem::from(Message {
            presentation: Default::default(),
            role: MessageRole::User,
            content: MessageContent::text("metadata".to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata,
        })];
        let tx = store.database().begin().await.unwrap();
        persist_transcript_mutation(
            &tx,
            &thread_id,
            1,
            1,
            &ThreadContextMutation::Replace {
                items: transcript.clone(),
            },
            &transcript,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        assert_eq!(
            restore_transcript(store.database(), &thread_id)
                .await
                .unwrap(),
            transcript
        );
    }

    #[tokio::test]
    async fn working_state_revision_conflict_and_corruption_fail_closed() {
        let (store, thread_id) = store_with_thread("working-state-revision").await;
        let original = AgentWorkingState::default();
        let tx = store.database().begin().await.unwrap();
        persist_working_state(&tx, &thread_id, &original, 1)
            .await
            .unwrap();
        persist_working_state(&tx, &thread_id, &original, 2)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let mut conflicting = original.clone();
        conflicting.prompt.active_scope = "changed-without-revision".to_string();
        let tx = store.database().begin().await.unwrap();
        let conflict = persist_working_state(&tx, &thread_id, &conflicting, 3)
            .await
            .unwrap_err();
        assert!(conflict.to_string().contains("revision conflicts"));
        tx.rollback().await.unwrap();

        let mut updated = conflicting;
        updated.revision = 1;
        let tx = store.database().begin().await.unwrap();
        persist_working_state(&tx, &thread_id, &updated, 4)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            restore_working_state(store.database(), &thread_id)
                .await
                .unwrap(),
            updated
        );

        let row = studio_object::Entity::find_by_id((
            "thread".to_string(),
            thread_id.clone(),
            "agentWorkingState".to_string(),
        ))
        .one(store.database())
        .await
        .unwrap()
        .unwrap();
        let mut active = row.into_active_model();
        active.payload_hash = Set("sha256:corrupt".to_string());
        active.update(store.database()).await.unwrap();
        let error = restore_working_state(store.database(), &thread_id)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("hash mismatch"));
    }

    #[tokio::test]
    async fn session_audit_distinguishes_corrupt_content_from_fatal_storage_failure() {
        let (store, thread_id) = store_with_thread("session-audit-corrupt").await;
        let Err(SessionSnapshotAuditError::Corrupt(missing)) =
            audit_session_snapshot(&store, &thread_id).await
        else {
            panic!("missing working state must be a scoped corruption");
        };
        assert!(missing.to_string().contains("session state is missing"));

        let tx = store.database().begin().await.unwrap();
        persist_working_state(&tx, &thread_id, &AgentWorkingState::default(), 1)
            .await
            .unwrap();
        let transcript = vec![text_item("baseline")];
        persist_transcript_mutation(
            &tx,
            &thread_id,
            1,
            1,
            &ThreadContextMutation::Replace {
                items: transcript.clone(),
            },
            &transcript,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        let row = thread_context_segment::Entity::find()
            .filter(thread_context_segment::Column::ThreadId.eq(&thread_id))
            .one(store.database())
            .await
            .unwrap()
            .unwrap();
        let mut active = row.into_active_model();
        active.ordinal = Set(2);
        active.update(store.database()).await.unwrap();
        let Err(SessionSnapshotAuditError::Corrupt(gap)) =
            audit_session_snapshot(&store, &thread_id).await
        else {
            panic!("segment gaps must be scoped corruption");
        };
        assert!(gap.to_string().contains("context segment gap"));

        store
            .database()
            .execute_unprepared("DROP TABLE studio_objects")
            .await
            .unwrap();
        let Err(SessionSnapshotAuditError::Fatal(storage)) =
            audit_session_snapshot(&store, &thread_id).await
        else {
            panic!("SQLite query failure must remain application-fatal");
        };
        assert!(storage.to_string().contains("studio_objects"));
    }

    async fn store_with_thread(slug: &str) -> (StudioStore, String) {
        let store = StudioStore::open_memory().await.unwrap();
        let project = store
            .upsert_project(std::env::temp_dir().join(slug))
            .await
            .unwrap();
        let thread = store
            .create_thread(&project.id, slug, ThreadModeId::simple())
            .await
            .unwrap();
        (store, thread.id)
    }

    fn text_item(content: &str) -> ModelContextItem {
        ModelContextItem::from(Message {
            presentation: Default::default(),
            role: MessageRole::User,
            content: MessageContent::text(content.to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_result: None,
            metadata: HashMap::new(),
        })
    }
}
