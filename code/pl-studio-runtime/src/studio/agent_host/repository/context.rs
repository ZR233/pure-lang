use pl_core::{AgentSession, ThreadCommit, ThreadContextMutation};
use pl_protocol::{ThreadItem, ThreadItemContent, ThreadItemStatus, ThreadPromptMetadata};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::StudioStore;
use crate::studio::entity::item;
use crate::{ModelContextItem, PureError};

use super::{persist_item, store_error};

pub(super) async fn restore_context_items(
    store: &StudioStore,
    thread_id: &str,
) -> Result<Option<Vec<ModelContextItem>>, PureError> {
    item::Entity::find()
        .filter(item::Column::ThreadId.eq(thread_id))
        .filter(item::Column::ItemKind.is_in(["contextPatch", "contextCompaction"]))
        .filter(item::Column::ProviderPrivatePayload.is_not_null())
        .order_by_desc(item::Column::Ordinal)
        .one(store.database())
        .await
        .map_err(store_error)?
        .and_then(|row| row.provider_private_payload)
        .map(|payload| serde_json::from_slice::<Vec<ModelContextItem>>(&payload))
        .transpose()
        .map_err(Into::into)
}

pub(super) async fn persist_context_baseline(
    tx: &sea_orm::DatabaseTransaction,
    commit: &ThreadCommit,
) -> Result<(), PureError> {
    let Some(mutation) = commit.facts.context.as_ref() else {
        return Ok(());
    };
    let context = match mutation {
        ThreadContextMutation::Append { items } | ThreadContextMutation::Replace { items } => items,
    };
    let Some(turn_id) = commit.facts.turn_id.as_ref() else {
        return Err(store_error("context mutation requires a Turn id"));
    };
    let thread_id = commit.agent_id.to_string();
    let now = commit.next_state.snapshot.updated_at;
    let mut baseline = context
        .iter()
        .rev()
        .find_map(ModelContextItem::as_context_patch)
        .map_or_else(
            || ThreadItem {
                id: format!("context:{thread_id}:{}", commit.facts.through_revision),
                thread_id: thread_id.clone(),
                turn_id: turn_id.to_string(),
                ordinal: 0,
                revision: 1,
                status: ThreadItemStatus::Completed,
                created_at: now,
                updated_at: now,
                completed_at: Some(now),
                error: None,
                content: ThreadItemContent::ContextCompaction {
                    before_tokens: 0,
                    after_tokens: commit.next_state.session.last_context_tokens.unwrap_or(0),
                    compacted_at: now,
                },
                usage: None,
            },
            |patch| ThreadItem {
                id: patch.id.clone(),
                thread_id: thread_id.clone(),
                turn_id: turn_id.to_string(),
                ordinal: 0,
                revision: 1,
                status: ThreadItemStatus::Completed,
                created_at: patch.prompt.updated_at,
                updated_at: now,
                completed_at: Some(now),
                error: None,
                content: ThreadItemContent::ContextPatch {
                    generation: patch.prompt.generation,
                    fixed_prefix_hash: patch.prompt.fixed_prefix_hash.clone(),
                    tool_schema_hash: patch.prompt.tool_schema_hash.clone(),
                    context_hash: patch.prompt.context_hash.clone(),
                    changed_section_ids: patch.changed_section_ids.clone(),
                    prefix_changed_reason: patch.prompt.prefix_changed_reason,
                },
                usage: None,
            },
        );
    if let Some(existing) = item::Entity::find_by_id(baseline.id.clone())
        .one(tx)
        .await
        .map_err(store_error)?
    {
        baseline.turn_id = existing.turn_id;
    }
    persist_item(tx, &baseline, Some(serde_json::to_vec(context)?)).await
}

pub(super) fn metadata_with_prompt_snapshot(
    metadata: &serde_json::Value,
    session: &AgentSession,
) -> Result<String, PureError> {
    let Some(patch) = session.latest_context_patch() else {
        return serde_json::to_string(metadata).map_err(Into::into);
    };
    let mut object = match metadata {
        serde_json::Value::Object(object) => object.clone(),
        serde_json::Value::Null => serde_json::Map::new(),
        _ => {
            return Err(store_error(
                "Thread metadata must be an object before storing prompt metadata",
            ));
        }
    };
    object.insert(
        "threadPromptSnapshot".to_string(),
        serde_json::to_value(ThreadPromptMetadata {
            active_scope: patch.prompt.scope.clone(),
            slots: patch.prompt_snapshots.clone(),
        })?,
    );
    serde_json::to_string(&serde_json::Value::Object(object)).map_err(Into::into)
}
