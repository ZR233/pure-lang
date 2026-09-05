//! Thread pending/active 输入行与 durable mailbox 状态的落库。

use std::collections::BTreeSet;

use pl_core::{
    DurableMailboxEnvelope, MailboxCommand, MailboxDeliveryState, ThreadActorState, TurnId,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder,
};

use crate::PureError;
use crate::studio::entity::thread_input;

use super::input_metadata::serialize_input_metadata;
use super::labels::presentation_label;
use super::store_error;

pub(super) async fn persist_inputs(
    tx: &sea_orm::DatabaseTransaction,
    state: &ThreadActorState,
) -> Result<(), PureError> {
    let thread_id = state.snapshot.identity.id.to_string();
    let existing = thread_input::Entity::find()
        .filter(thread_input::Column::ThreadId.eq(thread_id.clone()))
        .all(tx)
        .await
        .map_err(store_error)?;
    let mut live = BTreeSet::new();
    for input in &state.pending_inputs {
        live.insert(input.mail_id.clone());
        upsert_input(tx, &thread_id, input).await?;
    }
    if let Some(input) = &state.active_input {
        live.insert(input.mail_id.clone());
        upsert_input(tx, &thread_id, input).await?;
    }
    for row in existing {
        if live.contains(&row.mail_id) || row.state_kind == "consumed" {
            continue;
        }
        let mut delivery_state: MailboxDeliveryState = serde_json::from_str(&row.state_json)?;
        if delivery_state.is_pending() {
            delivery_state = delivery_state
                .decide(MailboxCommand::Claim {
                    turn_id: TurnId::new(row.turn_id.clone())?,
                })
                .map_err(store_error)?
                .next_state;
        }
        let turn_id = delivery_state
            .turn_id()
            .cloned()
            .ok_or_else(|| store_error("claimed mailbox is missing its Turn identity"))?;
        let checkpoint_seq = delivery_state.checkpoint_seq().unwrap_or_default();
        let delivery_state = delivery_state
            .decide(MailboxCommand::Consume {
                turn_id,
                checkpoint_seq,
            })
            .map_err(store_error)?
            .next_state;
        let mut active = row.into_active_model();
        active.state_json = Set(serde_json::to_string(&delivery_state)?);
        active.update(tx).await.map_err(store_error)?;
    }
    Ok(())
}

async fn upsert_input(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
    input: &DurableMailboxEnvelope,
) -> Result<(), PureError> {
    let existing = thread_input::Entity::find_by_id(input.mail_id.clone())
        .one(tx)
        .await
        .map_err(store_error)?;
    if let Some(existing) = existing.as_ref()
        && existing.thread_id != thread_id
    {
        return Err(store_error(format!(
            "mail id {} belongs to another Thread",
            input.mail_id
        )));
    }
    let ordinal = match existing.as_ref() {
        Some(existing) => existing.queue_ordinal,
        None => next_input_ordinal(tx, thread_id).await?,
    };
    let active = thread_input::ActiveModel {
        id: Set(input.mail_id.clone()),
        thread_id: Set(thread_id.to_string()),
        mail_id: Set(input.mail_id.clone()),
        turn_id: Set(input.turn_id.to_string()),
        content: Set(input.payload.message.clone()),
        attachments_json: Set(serde_json::to_string(&input.payload.attachments)?),
        metadata_json: Set(serialize_input_metadata(input)?),
        presentation: Set(presentation_label(input.payload.presentation).to_string()),
        state_json: Set(serde_json::to_string(&input.delivery_state)?),
        queue_ordinal: Set(ordinal),
        queued_at: Set(existing
            .as_ref()
            .map_or(input.queued_at, |row| row.queued_at)),
        ..Default::default()
    };
    match existing {
        Some(_) => active.update(tx).await.map_err(store_error)?,
        None => active.insert(tx).await.map_err(store_error)?,
    };
    Ok(())
}

async fn next_input_ordinal(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<i64, PureError> {
    Ok(thread_input::Entity::find()
        .filter(thread_input::Column::ThreadId.eq(thread_id))
        .order_by_desc(thread_input::Column::QueueOrdinal)
        .one(tx)
        .await
        .map_err(store_error)?
        .map_or(0, |row| row.queue_ordinal.saturating_add(1)))
}
