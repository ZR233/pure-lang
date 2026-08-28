//! Thread item 到 store 行投影的核心类型与 durable 投递身份落库。

use pl_core::{AgentState, ThreadActorState, ThreadCommit, TurnId};
use pl_protocol::{
    BudgetLimitedTurnState, CancelledTurnState, CompletedTurnState, FailedTurnState, PureError,
    QueuedTurnState, RunningTurnState, ThreadItem, ThreadNotification, Turn, TurnOutcome,
    TurnPhase, TurnState,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::studio::StudioStore;
use crate::studio::entity::{interaction, item, turn};

use super::billing::authoritative_turn_usage;
use super::{i64_from_u64, store_error};

struct TurnProjection<'a> {
    id: &'a str,
    thread_id: &'a str,
    state: TurnState,
    usage: Option<&'a pl_model::TokenUsage>,
    metadata: Option<&'a serde_json::Value>,
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
    let state = preserve_running_started_at(existing.as_ref(), projection.state)?;
    let active = turn::ActiveModel {
        id: Set(projection.id.to_string()),
        thread_id: Set(projection.thread_id.to_string()),
        ordinal: Set(ordinal),
        revision: Set(i64_from_u64(projection.revision)?),
        state_json: Set(serde_json::to_string(&state)?),
        model_json: Set(existing.as_ref().and_then(|row| row.model_json.clone())),
        usage_json: Set(serde_json::to_string(&usage)?),
        metadata_json: Set(match (existing.as_ref(), projection.metadata) {
            (Some(row), None) => row.metadata_json.clone(),
            (_, metadata) => metadata.map(serde_json::to_string).transpose()?,
        }),
        updated_at: Set(projection.updated_at),
        ..Default::default()
    };
    match existing {
        Some(_) => active.update(tx).await.map_err(store_error)?,
        None => active.insert(tx).await.map_err(store_error)?,
    };
    Ok(())
}

fn preserve_running_started_at(
    existing: Option<&turn::Model>,
    state: TurnState,
) -> Result<TurnState, PureError> {
    let TurnState::Running(next) = state else {
        return Ok(state);
    };
    let started_at = existing
        .map(|row| serde_json::from_str::<TurnState>(&row.state_json))
        .transpose()?
        .and_then(|state| state.started_at())
        .unwrap_or_else(|| next.started_at());
    Ok(TurnState::Running(RunningTurnState::new(
        started_at,
        next.phase(),
    )))
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
    persist_turn_projection(
        tx,
        TurnProjection {
            id: &value.id,
            thread_id: &value.thread_id,
            state: value.state.clone(),
            usage: None,
            metadata: None,
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
        state_json: Set(serde_json::to_string(value.state())?),
        created_at: Set(existing
            .as_ref()
            .map_or(value.created_at, |row| row.created_at)),
        updated_at: Set(value.updated_at),
        ..Default::default()
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
        revision: Set(i64_from_u64(value.revision)?),
        state_json: Set(serde_json::to_string(&value.content)?),
        created_at: Set(existing
            .as_ref()
            .map_or(value.created_at, |row| row.created_at)),
        updated_at: Set(value.updated_at),
        ..Default::default()
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
        query.filter(turn::Column::StateKind.is_in(["queued", "running"]))
    } else {
        query.filter(turn::Column::StateKind.is_in([
            "completed",
            "cancelled",
            "failed",
            "budgetLimited",
        ]))
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
                state: TurnState::Queued(QueuedTurnState::new(input.queued_at)),
                usage: None,
                metadata: Some(&input.payload.metadata),
                updated_at: input.queued_at,
                revision: state.session.thread_revision,
            },
        )
        .await?;
    }
    if let Some(turn_id) = state.snapshot.active_turn_id()
        && let Some(turn_state) = active_turn_projection(state, turn_id)?
    {
        // 只有缺失 Turn 行时，才用粗粒度 AgentState fallback 补建并写入
        // 保守 phase；既有行由 canonical Thread notification 拥有精确 phase，
        // 宿主的 active_turn_projection 不得反向覆盖它。
        if turn::Entity::find_by_id(turn_id.to_string())
            .one(tx)
            .await
            .map_err(store_error)?
            .is_none()
        {
            persist_turn_projection(
                tx,
                TurnProjection {
                    id: turn_id.as_str(),
                    thread_id,
                    state: turn_state,
                    usage: None,
                    metadata: None,
                    updated_at: state.snapshot.updated_at,
                    revision: state.session.thread_revision,
                },
            )
            .await?;
        }
    }
    if let Some(outcome) = &state.snapshot.last_turn {
        let turn_state = match &outcome.outcome {
            TurnOutcome::Completed(value) => TurnState::Completed(CompletedTurnState::new(
                outcome.started_at,
                outcome.finished_at,
                value.completion(),
            )),
            TurnOutcome::Cancelled(value) => TurnState::Cancelled(CancelledTurnState::new(
                outcome.started_at,
                outcome.finished_at,
                outcome.finished_at,
                value.cause().clone(),
            )),
            TurnOutcome::Failed(value) => TurnState::Failed(FailedTurnState::new(
                outcome.started_at,
                outcome.finished_at,
                value.failure().clone(),
            )),
            TurnOutcome::BudgetLimited(value) => {
                TurnState::BudgetLimited(BudgetLimitedTurnState::new(
                    outcome.started_at,
                    outcome.finished_at,
                    *value.limit(),
                    value.rollover().clone(),
                ))
            }
        };
        persist_turn_projection(
            tx,
            TurnProjection {
                id: outcome.turn_id.as_str(),
                thread_id,
                state: turn_state,
                usage: Some(&outcome.usage),
                metadata: None,
                updated_at: outcome.finished_at,
                revision: state.session.thread_revision,
            },
        )
        .await?;
    }
    Ok(())
}

fn active_turn_projection(
    state: &ThreadActorState,
    turn_id: &TurnId,
) -> Result<Option<TurnState>, PureError> {
    let turn_state = match &state.snapshot.state {
        AgentState::Queued(_) => TurnState::Queued(QueuedTurnState::new(state.snapshot.updated_at)),
        AgentState::Running(_) => TurnState::Running(RunningTurnState::new(
            state.snapshot.updated_at,
            TurnPhase::Responding,
        )),
        AgentState::WaitingTool(_) => TurnState::Running(RunningTurnState::new(
            state.snapshot.updated_at,
            TurnPhase::RunningTool,
        )),
        AgentState::WaitingInteraction(_) => TurnState::Running(RunningTurnState::new(
            state.snapshot.updated_at,
            TurnPhase::Responding,
        )),
        AgentState::Cancelling(_) => TurnState::Running(RunningTurnState::new(
            state.snapshot.updated_at,
            TurnPhase::Persisting,
        )),
        AgentState::Faulted(_) => {
            let outcome = state.snapshot.last_turn.as_ref().ok_or_else(|| {
                store_error("Faulted Agent diagnostic Turn is missing its failed outcome")
            })?;
            if &outcome.turn_id != turn_id {
                return Err(store_error(
                    "Faulted Agent diagnostic Turn does not match the last Turn outcome",
                ));
            }
            if !matches!(outcome.outcome, TurnOutcome::Failed(_)) {
                return Err(store_error(
                    "Faulted Agent diagnostic Turn must have a failed outcome",
                ));
            }
            return Ok(None);
        }
        AgentState::Idle(_) | AgentState::Closing(_) | AgentState::Closed(_) => {
            return Err(store_error("Agent state exposes an invalid active Turn"));
        }
    };
    Ok(Some(turn_state))
}
