//! Thread item 到 store 行投影的核心类型与 durable 投递身份落库。

use pl_core::{MailboxDeliveryState, ThreadActorState, ThreadCommit, TurnId};
use pl_protocol::{PureError, ThreadItem, ThreadNotification, Turn};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::studio::StudioStore;
use crate::studio::entity::{interaction, item, turn};

use super::billing::authoritative_turn_usage;
use super::labels::{
    activity_phase, interaction_kind_label, interaction_status_label, item_kind_label,
    item_status_label, outcome_columns, turn_state_columns,
};
use super::{i64_from_u64, store_error};

struct TurnProjection<'a> {
    id: &'a str,
    thread_id: &'a str,
    status: &'a str,
    phase: Option<&'a str>,
    reason: Option<&'a str>,
    usage: Option<&'a pl_model::TokenUsage>,
    failure: Option<&'a pl_protocol::TurnFailure>,
    budget_limit: Option<&'a pl_protocol::BudgetLimitSnapshot>,
    rollover_compacted: Option<bool>,
    rollover_compaction_error: Option<&'a str>,
    metadata: Option<&'a serde_json::Value>,
    started_at: Option<i64>,
    completed_at: Option<i64>,
    updated_at: i64,
    revision: u64,
}

async fn persist_turn_projection(
    tx: &sea_orm::DatabaseTransaction,
    projection: TurnProjection<'_>,
) -> Result<(), PureError> {
    let existing = turn::Entity::find_by_id(projection.id.to_string())
        .one(tx)
        .await
        .map_err(store_error)?;
    if let Some(existing) = existing.as_ref()
        && existing.thread_id != projection.thread_id
    {
        return Err(store_error(format!(
            "Turn {} belongs to another Thread",
            projection.id
        )));
    }
    let ordinal = match existing.as_ref() {
        Some(existing) => existing.ordinal,
        None => next_turn_ordinal(tx, projection.thread_id).await?,
    };
    let usage = authoritative_turn_usage(existing.as_ref(), projection.usage)?;
    let active = turn::ActiveModel {
        id: Set(projection.id.to_string()),
        thread_id: Set(projection.thread_id.to_string()),
        ordinal: Set(ordinal),
        revision: Set(i64_from_u64(projection.revision)?),
        status: Set(projection.status.to_string()),
        phase: Set(projection.phase.map(str::to_string)),
        reason: Set(projection.reason.map(str::to_string)),
        model_json: Set(existing.as_ref().and_then(|row| row.model_json.clone())),
        usage_json: Set(serde_json::to_string(&usage)?),
        failure_json: Set(projection.failure.map(serde_json::to_string).transpose()?),
        budget_limit_json: Set(match (projection.budget_limit, existing.as_ref()) {
            (Some(limit), _) => Some(serde_json::to_string(limit)?),
            (None, Some(row)) => row.budget_limit_json.clone(),
            (None, None) => None,
        }),
        rollover_compacted: Set(projection.rollover_compacted.map_or_else(
            || existing.as_ref().map_or(0, |row| row.rollover_compacted),
            |compacted| if compacted { 1 } else { 0 },
        )),
        rollover_compaction_error: Set(
            match (projection.rollover_compaction_error, existing.as_ref()) {
                (Some(error), _) => Some(error.to_string()),
                (None, Some(row)) => row.rollover_compaction_error.clone(),
                (None, None) => None,
            },
        ),
        metadata_json: Set(match (existing.as_ref(), projection.metadata) {
            (Some(row), None) => row.metadata_json.clone(),
            (_, metadata) => metadata.map(serde_json::to_string).transpose()?,
        }),
        started_at: Set(existing
            .as_ref()
            .and_then(|row| row.started_at)
            .or(projection.started_at)),
        updated_at: Set(projection.updated_at),
        completed_at: Set(projection.completed_at),
    };
    match existing {
        Some(_) => active.update(tx).await.map_err(store_error)?,
        None => active.insert(tx).await.map_err(store_error)?,
    };
    Ok(())
}

pub(super) async fn persist_thread_notifications(
    tx: &sea_orm::DatabaseTransaction,
    commit: &ThreadCommit,
) -> Result<(), PureError> {
    for envelope in &commit.facts.notifications {
        match &envelope.notification {
            ThreadNotification::TurnStarted { turn: value }
            | ThreadNotification::TurnUpdated { turn: value }
            | ThreadNotification::TurnCompleted { turn: value } => {
                persist_turn(tx, value, envelope.revision).await?;
            }
            ThreadNotification::ItemStarted { item: value }
            | ThreadNotification::ItemCompleted { item: value } => {
                persist_item(tx, value).await?;
            }
            ThreadNotification::InteractionChanged { interaction: value } => {
                persist_interaction(tx, value).await?;
            }
            ThreadNotification::ItemDelta { .. }
            | ThreadNotification::ThreadRuntimeUpdated { .. }
            | ThreadNotification::Lagged { .. } => {}
        }
    }
    Ok(())
}

async fn persist_turn(
    tx: &sea_orm::DatabaseTransaction,
    value: &Turn,
    revision: u64,
) -> Result<(), PureError> {
    let (status, phase, reason) = turn_state_columns(&value.state);
    persist_turn_projection(
        tx,
        TurnProjection {
            id: &value.id,
            thread_id: &value.thread_id,
            status,
            phase,
            reason,
            usage: None,
            failure: value.failure.as_ref(),
            budget_limit: None,
            rollover_compacted: None,
            rollover_compaction_error: None,
            metadata: None,
            started_at: value.started_at,
            completed_at: value.completed_at,
            updated_at: value.updated_at,
            revision,
        },
    )
    .await
}

async fn persist_item(
    tx: &sea_orm::DatabaseTransaction,
    value: &ThreadItem,
) -> Result<(), PureError> {
    let existing = item::Entity::find_by_id(value.id.clone())
        .one(tx)
        .await
        .map_err(store_error)?;
    if let Some(existing) = existing.as_ref()
        && (existing.thread_id != value.thread_id || existing.turn_id != value.turn_id)
    {
        return Err(store_error(format!(
            "Item {} immutable identity changed",
            value.id
        )));
    }
    // ordinal 是内存权威事实：由 ThreadEventBus 首次应用时一次性分配，
    // DB 只原样落库（含既有行），不再派生顺序事实。
    let active = item::ActiveModel {
        id: Set(value.id.clone()),
        thread_id: Set(value.thread_id.clone()),
        turn_id: Set(value.turn_id.clone()),
        ordinal: Set(i64_from_u64(value.ordinal)?),
        revision: Set(i64_from_u64(value.revision)?),
        item_kind: Set(item_kind_label(&value.content).to_string()),
        status: Set(item_status_label(value.status).to_string()),
        payload_json: Set(serde_json::to_string(value)?),
        created_at: Set(existing
            .as_ref()
            .map_or(value.created_at, |row| row.created_at)),
        updated_at: Set(value.updated_at),
        completed_at: Set(value.completed_at),
    };
    match existing {
        Some(_) => active.update(tx).await.map_err(store_error)?,
        None => active.insert(tx).await.map_err(store_error)?,
    };
    Ok(())
}

async fn persist_interaction(
    tx: &sea_orm::DatabaseTransaction,
    value: &pl_protocol::InteractionRequest,
) -> Result<(), PureError> {
    let existing = interaction::Entity::find_by_id(value.interaction_id.clone())
        .one(tx)
        .await
        .map_err(store_error)?;
    let active = interaction::ActiveModel {
        id: Set(value.interaction_id.clone()),
        thread_id: Set(value.scope.thread_id.clone()),
        turn_id: Set(value.scope.turn_id.clone()),
        item_id: Set(value.scope.item_id.clone()),
        tool_id: Set(value.scope.tool_id.clone()),
        agent_path: Set(value.scope.agent_path.clone()),
        kind: Set(interaction_kind_label(value.kind.clone()).to_string()),
        status: Set(interaction_status_label(value.status.clone()).to_string()),
        payload_json: Set(serde_json::to_string(&value.payload)?),
        resolution_json: Set(value
            .resolution
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?),
        created_at: Set(existing
            .as_ref()
            .map_or(value.created_at, |row| row.created_at)),
        updated_at: Set(value.updated_at),
        resolved_at: Set(value.resolved_at),
    };
    match existing {
        Some(_) => active.update(tx).await.map_err(store_error)?,
        None => active.insert(tx).await.map_err(store_error)?,
    };
    Ok(())
}

pub(super) async fn latest_turn(
    store: &StudioStore,
    thread_id: &str,
    active: bool,
) -> Result<Option<turn::Model>, PureError> {
    let query = turn::Entity::find().filter(turn::Column::ThreadId.eq(thread_id));
    let query = if active {
        query.filter(turn::Column::Status.is_in(["queued", "inProgress"]))
    } else {
        query.filter(turn::Column::Status.is_in(["completed", "failed", "interrupted"]))
    };
    query
        .order_by_desc(turn::Column::Ordinal)
        .one(store.database())
        .await
        .map_err(store_error)
}

async fn next_turn_ordinal(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<i64, PureError> {
    Ok(turn::Entity::find()
        .filter(turn::Column::ThreadId.eq(thread_id))
        .order_by_desc(turn::Column::Ordinal)
        .one(tx)
        .await
        .map_err(store_error)?
        .map_or(0, |row| row.ordinal.saturating_add(1)))
}

pub(super) fn delivery_identity(state: &MailboxDeliveryState) -> Option<(TurnId, u64)> {
    match state {
        MailboxDeliveryState::Claimed {
            turn_id,
            checkpoint_seq,
        }
        | MailboxDeliveryState::Consumed {
            turn_id,
            checkpoint_seq,
        } => Some((turn_id.clone(), *checkpoint_seq)),
        MailboxDeliveryState::Pending => None,
    }
}

pub(super) async fn persist_state_turns(
    tx: &sea_orm::DatabaseTransaction,
    state: &ThreadActorState,
) -> Result<(), PureError> {
    let thread_id = state.snapshot.identity.id.as_str();
    for input in &state.pending_inputs {
        persist_turn_projection(
            tx,
            TurnProjection {
                id: input.turn_id.as_str(),
                thread_id,
                status: "queued",
                phase: None,
                reason: None,
                usage: None,
                failure: None,
                budget_limit: None,
                rollover_compacted: None,
                rollover_compaction_error: None,
                metadata: Some(&input.payload.metadata),
                started_at: None,
                completed_at: None,
                updated_at: input.queued_at,
                revision: state.session.thread_revision,
            },
        )
        .await?;
    }
    if let Some(turn_id) = state.snapshot.active_turn_id.as_ref() {
        persist_turn_projection(
            tx,
            TurnProjection {
                id: turn_id.as_str(),
                thread_id,
                status: "inProgress",
                phase: Some(activity_phase(state.snapshot.activity)),
                reason: None,
                usage: None,
                failure: None,
                budget_limit: None,
                rollover_compacted: None,
                rollover_compaction_error: None,
                metadata: None,
                started_at: Some(state.snapshot.updated_at),
                completed_at: None,
                updated_at: state.snapshot.updated_at,
                revision: state.session.thread_revision,
            },
        )
        .await?;
    }
    if let Some(outcome) = &state.snapshot.last_turn {
        let (status, reason) = outcome_columns(outcome);
        persist_turn_projection(
            tx,
            TurnProjection {
                id: outcome.turn_id.as_str(),
                thread_id,
                status,
                phase: None,
                reason,
                usage: Some(&outcome.usage),
                failure: outcome.failure.as_ref(),
                budget_limit: outcome.budget_limit.as_ref(),
                rollover_compacted: Some(outcome.rollover_compacted),
                rollover_compaction_error: outcome.rollover_compaction_error.as_deref(),
                metadata: None,
                started_at: None,
                completed_at: Some(outcome.finished_at),
                updated_at: outcome.finished_at,
                revision: state.session.thread_revision,
            },
        )
        .await?;
    }
    Ok(())
}
