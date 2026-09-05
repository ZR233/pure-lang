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
    usage: Option<&'a pl_protocol::InferenceTokenUsage>,
    metadata: Option<&'a pl_core::MailboxMetadata>,
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
        purpose_json: Set(serde_json::to_string(&value.scope.purpose)?),
        continuation_json: Set(serde_json::to_string(&value.continuation)?),
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
        let existing = turn::Entity::find_by_id(turn_id.to_string())
            .one(tx)
            .await
            .map_err(store_error)?;
        match existing {
            Some(existing) => {
                if existing.thread_id != thread_id {
                    return Err(store_error(format!(
                        "Turn {} belongs to another Thread",
                        turn_id.as_str()
                    )));
                }
                // 同 Thread 的既有行由 canonical Thread notification 拥有精确 phase，
                // 宿主的 active_turn_projection 不得反向覆盖它。
            }
            None => {
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

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use pl_core::{
        AgentIdentity, AgentRoleId, AgentSnapshot, AgentState, DurableCommitFacts,
        PersistenceClass, RunningAgentState, ThreadActorState, ThreadContextState, ThreadId,
        ThreadMutation, TurnId,
    };
    use pl_protocol::{
        RunningTurnState, ThreadNotification, ThreadNotificationEnvelope, Turn, TurnPhase,
        TurnState,
    };
    use sea_orm::{ActiveModelTrait, ActiveValue::Set, EntityTrait, TransactionTrait};

    use crate::ThreadModeId;

    use super::*;

    fn running_actor_state(thread_id: &str, turn_id: &str, updated_at: i64) -> ThreadActorState {
        let thread_id = ThreadId::new(thread_id).expect("thread id");
        let turn_id = TurnId::new(turn_id).expect("turn id");
        ThreadActorState {
            snapshot: AgentSnapshot {
                identity: AgentIdentity {
                    id: thread_id.clone(),
                    parent_id: None,
                    role: AgentRoleId::new("executor").expect("role"),
                    depth: 0,
                },
                state: AgentState::Running(RunningAgentState::new(turn_id)),
                pending_inputs: 0,
                progress: None,
                last_turn: None,
                revision: 1,
                event_sequence: 1,
                updated_at,
            },
            session: ThreadContextState::empty(),
            pending_inputs: VecDeque::new(),
            active_input: None,
        }
    }

    fn turn_update_commit(
        state: ThreadActorState,
        turn_id: &str,
        phase: TurnPhase,
        emitted_at: i64,
    ) -> ThreadCommit {
        let thread_id = state.snapshot.identity.id.clone();
        let turn_id_ref = TurnId::new(turn_id).expect("turn id");
        ThreadCommit {
            agent_id: thread_id.clone(),
            persistence: PersistenceClass::Standard,
            expected_revision: None,
            next_state: state,
            facts: DurableCommitFacts {
                thread_id: thread_id.clone(),
                turn_id: Some(turn_id_ref),
                through_revision: 0,
                revision: 1,
                notifications: vec![ThreadNotificationEnvelope {
                    thread_id: thread_id.to_string(),
                    revision: 1,
                    emitted_at,
                    notification: ThreadNotification::TurnUpdated {
                        turn: Turn {
                            id: turn_id.to_string(),
                            thread_id: thread_id.to_string(),
                            revision: 1,
                            state: TurnState::Running(RunningTurnState::new(emitted_at, phase)),
                            updated_at: emitted_at,
                        },
                    },
                }],
                turn_transition: None,
                context: None,
                projection_snapshot: None,
                runtime_events: Vec::new(),
                trace_events: Vec::new(),
                inference: None,
                submission: None,
            },
            mutation: ThreadMutation::ReplaceThread { thread_id },
        }
    }

    async fn seed_running_turn(
        store: &StudioStore,
        thread_id: &str,
        turn_id: &str,
        ordinal: i64,
        phase: TurnPhase,
    ) {
        turn::ActiveModel {
            id: Set(turn_id.to_string()),
            thread_id: Set(thread_id.to_string()),
            ordinal: Set(ordinal),
            revision: Set(1),
            state_json: Set(
                serde_json::to_string(&TurnState::Running(RunningTurnState::new(1, phase)))
                    .expect("turn state JSON"),
            ),
            model_json: Set(None),
            usage_json: Set(
                serde_json::to_string(&pl_protocol::InferenceTokenUsage::default()).unwrap(),
            ),
            metadata_json: Set(None),
            updated_at: Set(1),
            ..Default::default()
        }
        .insert(store.database())
        .await
        .expect("seed running turn");
    }

    async fn assert_phase(
        store: &StudioStore,
        thread_id: &str,
        turn_id: &str,
        expected: TurnPhase,
    ) {
        let row = turn::Entity::find_by_id(turn_id)
            .one(store.database())
            .await
            .expect("read turn")
            .expect("turn row exists");
        assert_eq!(row.thread_id, thread_id, "turn {turn_id} belongs to thread");
        let state = serde_json::from_str::<TurnState>(&row.state_json).expect("parse turn state");
        assert_eq!(
            state.phase(),
            Some(expected),
            "turn {turn_id} must keep phase {expected:?}"
        );
    }

    #[tokio::test]
    async fn active_turn_fallback_preserves_canonical_phase() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let project = store
            .upsert_project(std::env::temp_dir().join("active-turn-fallback"))
            .await
            .expect("project");
        let thread = store
            .create_thread(&project.id, "active-turn-fallback", ThreadModeId::simple())
            .await
            .expect("thread");
        let thread_id = thread.id;

        // 既有的 canonical Thinking phase 不能被粗粒度 AgentState fallback 覆盖。
        seed_running_turn(&store, &thread_id, "turn-thinking", 0, TurnPhase::Thinking).await;
        {
            let tx = store.database().begin().await.expect("begin");
            let state = running_actor_state(&thread_id, "turn-thinking", 9);
            persist_state_turns(&tx, &state)
                .await
                .expect("write-behind");
            tx.commit().await.expect("commit");
        }
        assert_phase(&store, &thread_id, "turn-thinking", TurnPhase::Thinking).await;

        // 既有的 canonical RunningTool phase 同样被保留。
        seed_running_turn(&store, &thread_id, "turn-tool", 1, TurnPhase::RunningTool).await;
        {
            let tx = store.database().begin().await.expect("begin");
            let state = running_actor_state(&thread_id, "turn-tool", 10);
            persist_state_turns(&tx, &state)
                .await
                .expect("write-behind");
            tx.commit().await.expect("commit");
        }
        assert_phase(&store, &thread_id, "turn-tool", TurnPhase::RunningTool).await;

        // 缺失 Turn 行仍建立粗粒度 fallback。
        {
            let tx = store.database().begin().await.expect("begin");
            let state = running_actor_state(&thread_id, "turn-missing", 11);
            persist_state_turns(&tx, &state)
                .await
                .expect("write-behind");
            tx.commit().await.expect("commit");
        }
        assert_phase(&store, &thread_id, "turn-missing", TurnPhase::Responding).await;

        // 后续 canonical ThreadNotification 把 fallback 覆盖为精确 phase。
        {
            let tx = store.database().begin().await.expect("begin");
            let state = running_actor_state(&thread_id, "turn-missing", 12);
            let commit = turn_update_commit(state, "turn-missing", TurnPhase::Thinking, 12);
            persist_thread_notifications(&tx, &commit)
                .await
                .expect("notification");
            tx.commit().await.expect("commit");
        }
        assert_phase(&store, &thread_id, "turn-missing", TurnPhase::Thinking).await;

        // 同一 Turn id 已属于另一 Thread 时，持久化必须失败且不得修改该行。
        let foreign = store
            .create_thread(&project.id, "foreign", ThreadModeId::simple())
            .await
            .expect("foreign thread");
        seed_running_turn(&store, &foreign.id, "turn-foreign", 0, TurnPhase::Thinking).await;
        let before = turn::Entity::find_by_id("turn-foreign")
            .one(store.database())
            .await
            .expect("read foreign turn")
            .expect("foreign turn exists");
        {
            let tx = store.database().begin().await.expect("begin");
            let state = running_actor_state(&thread_id, "turn-foreign", 13);
            let error = persist_state_turns(&tx, &state)
                .await
                .expect_err("must reject a Turn owned by another Thread");
            assert!(
                error.to_string().contains("belongs to another Thread"),
                "unexpected error: {error}"
            );
            tx.rollback().await.ok();
        }
        let after = turn::Entity::find_by_id("turn-foreign")
            .one(store.database())
            .await
            .expect("read foreign turn")
            .expect("foreign turn exists");
        assert_eq!(after, before, "foreign Turn row must be unchanged");
    }
}
