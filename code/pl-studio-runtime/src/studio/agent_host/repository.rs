use std::collections::VecDeque;

use crate::{ModelContextItem, PureError};
use pl_core::{
    AgentActivityState, AgentCommitOutcome, AgentDurableState, AgentSession, AgentSessionState,
    AgentStateRepository, PendingAgentInput, RestoredAgentRuntime, RestoredSessionProjection,
    SessionHistoryCommit, SessionId, TurnOutcomeKind,
};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter,
    QueryOrder, QuerySelect, TransactionTrait,
};

use super::persistence::AgentPersistenceWriter;
use crate::studio::StudioStore;
use crate::studio::entity::{
    agent_active_input, agent_pending_input, agent_runtime_session, agent_runtime_state,
    agent_turn, session_history_item, session_view_snapshot,
};

/// Studio SQLite 对 PL canonical runtime state 的 CAS repository。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentRepository {
    store: StudioStore,
    writer: AgentPersistenceWriter,
}

impl StudioAgentRepository {
    pub(super) fn new(store: StudioStore) -> Self {
        let writer = AgentPersistenceWriter::spawn(store.clone());
        Self { store, writer }
    }
}

impl AgentStateRepository for StudioAgentRepository {
    type Error = PureError;

    async fn restore_runtime(&self) -> Result<Vec<RestoredAgentRuntime>, Self::Error> {
        let states = agent_runtime_state::Entity::find()
            .order_by_asc(agent_runtime_state::Column::AgentId)
            .all(self.store.database())
            .await
            .map_err(store_error)?;
        let mut restored = Vec::with_capacity(states.len());
        for state in states {
            let agent_id = state.agent_id;
            let snapshot: pl_core::AgentSnapshot = serde_json::from_str(&state.snapshot_json)?;
            if snapshot.revision != u64_from_i64(state.revision)? {
                return Err(store_error(format!(
                    "agent {agent_id} snapshot revision does not match its CAS revision"
                )));
            }
            self.writer
                .seed_revision(&agent_id, snapshot.revision)
                .await;
            let session = self.restore_session(&agent_id).await?;
            let pending_inputs = self.restore_pending_inputs(&agent_id).await?;
            let active_input = self.restore_active_input(&agent_id).await?;
            let session_projection = self.restore_session_projection(&agent_id).await?;
            restored.push(RestoredAgentRuntime {
                state: AgentDurableState {
                    snapshot,
                    session,
                    pending_inputs,
                    active_input,
                },
                session_projection,
            });
        }
        Ok(restored)
    }

    async fn commit(
        &self,
        commit: SessionHistoryCommit,
    ) -> Result<AgentCommitOutcome, Self::Error> {
        self.writer.submit(commit).await
    }

    async fn barrier(&self) -> Result<(), Self::Error> {
        self.writer.barrier().await
    }
}

pub(super) async fn persist_state_commit(
    store: &StudioStore,
    commit: &SessionHistoryCommit,
) -> Result<AgentCommitOutcome, PureError> {
    let agent_id = commit.agent_id.to_string();
    let tx = store.database().begin().await.map_err(store_error)?;
    let existing_state = agent_runtime_state::Entity::find_by_id(agent_id.clone())
        .one(&tx)
        .await
        .map_err(store_error)?;
    let actual_revision = existing_state
        .as_ref()
        .map(|state| u64_from_i64(state.revision))
        .transpose()?;
    if actual_revision != commit.expected_revision {
        tx.rollback().await.map_err(store_error)?;
        return Ok(AgentCommitOutcome::RevisionConflict { actual_revision });
    }

    let next_state = agent_runtime_state::ActiveModel {
        agent_id: Set(agent_id.clone()),
        revision: Set(i64_from_u64(commit.next_state.snapshot.revision)?),
        snapshot_json: Set(serde_json::to_string(&commit.next_state.snapshot)?),
        updated_at: Set(commit.next_state.snapshot.updated_at),
    };
    match existing_state {
        Some(_) => next_state.update(&tx).await.map_err(store_error)?,
        None => next_state.insert(&tx).await.map_err(store_error)?,
    };
    upsert_session(&tx, &agent_id, &commit.next_state).await?;
    replace_pending_inputs(&tx, &agent_id, &commit.next_state).await?;
    replace_active_input(&tx, &agent_id, &commit.next_state).await?;
    upsert_turns(&tx, &agent_id, &commit.next_state).await?;
    if let Some(snapshot) = &commit.facts.projection_snapshot {
        persist_session_projection(&tx, snapshot, &commit.facts.items).await?;
    }
    tx.commit().await.map_err(store_error)?;
    Ok(AgentCommitOutcome::Applied)
}

impl StudioAgentRepository {
    async fn restore_session(&self, agent_id: &str) -> Result<AgentSessionState, PureError> {
        agent_runtime_session::Entity::find_by_id(agent_id.to_string())
            .one(self.store.database())
            .await
            .map_err(store_error)?
            .ok_or_else(|| store_error(format!("agent {agent_id} has no canonical session")))
            .and_then(|session| {
                let id = SessionId::new(session.session_id)?;
                let items: Vec<ModelContextItem> = serde_json::from_str(&session.context_json)?;
                let last_context_tokens = session
                    .last_context_tokens
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|error| store_error(error.to_string()))?;
                Ok(AgentSessionState {
                    id,
                    metadata: serde_json::from_str(&session.metadata_json)?,
                    session: AgentSession::from_items(items),
                    usage: serde_json::from_str(&session.usage_json)?,
                    last_context_tokens,
                    trace_sequence: u64_from_i64(session.trace_sequence)?,
                    session_event_sequence: u64_from_i64(session.session_event_sequence)?,
                })
            })
    }

    async fn restore_session_projection(
        &self,
        agent_id: &str,
    ) -> Result<Option<RestoredSessionProjection>, PureError> {
        let runtime_session = agent_runtime_session::Entity::find_by_id(agent_id.to_string())
            .one(self.store.database())
            .await
            .map_err(store_error)?;
        let Some(runtime_session) = runtime_session else {
            return Ok(None);
        };
        let session_id = runtime_session.session_id;
        let projection = session_view_snapshot::Entity::find_by_id(session_id.clone())
            .one(self.store.database())
            .await
            .map_err(store_error)?;
        let mut snapshot = projection
            .map(|projection| serde_json::from_str(&projection.snapshot_json))
            .transpose()?
            .unwrap_or_else(|| pl_protocol::SessionViewSnapshot::empty(session_id.clone()));
        let suffix = session_history_item::Entity::find()
            .filter(session_history_item::Column::SessionId.eq(session_id.clone()))
            .filter(
                session_history_item::Column::Sequence.gt(i64_from_u64(snapshot.through_sequence)?),
            )
            .order_by_asc(session_history_item::Column::Sequence)
            .all(self.store.history_database())
            .await
            .map_err(store_error)?
            .into_iter()
            .map(|item| serde_json::from_str(&item.payload_json).map_err(Into::into))
            .collect::<Result<Vec<_>, PureError>>()?;
        snapshot =
            pl_core::replay_session_history_suffix(snapshot, &suffix).map_err(store_error)?;
        let mut retained_durable_events = session_history_item::Entity::find()
            .filter(session_history_item::Column::SessionId.eq(session_id))
            .order_by_desc(session_history_item::Column::Sequence)
            .limit(4096)
            .all(self.store.history_database())
            .await
            .map_err(store_error)?
            .into_iter()
            .map(|item| serde_json::from_str(&item.payload_json).map_err(Into::into))
            .collect::<Result<Vec<_>, PureError>>()?;
        retained_durable_events.reverse();
        Ok(Some(RestoredSessionProjection {
            snapshot,
            retained_durable_events,
        }))
    }

    async fn restore_pending_inputs(
        &self,
        agent_id: &str,
    ) -> Result<VecDeque<PendingAgentInput>, PureError> {
        agent_pending_input::Entity::find()
            .filter(agent_pending_input::Column::AgentId.eq(agent_id))
            .order_by_asc(agent_pending_input::Column::QueuePosition)
            .all(self.store.database())
            .await
            .map_err(store_error)?
            .into_iter()
            .map(|input| serde_json::from_str(&input.input_json).map_err(Into::into))
            .collect()
    }

    async fn restore_active_input(
        &self,
        agent_id: &str,
    ) -> Result<Option<PendingAgentInput>, PureError> {
        agent_active_input::Entity::find_by_id(agent_id.to_string())
            .one(self.store.database())
            .await
            .map_err(store_error)?
            .map(|input| serde_json::from_str(&input.input_json).map_err(Into::into))
            .transpose()
    }
}

async fn persist_session_projection(
    tx: &sea_orm::DatabaseTransaction,
    snapshot: &pl_protocol::SessionViewSnapshot,
    items: &[pl_protocol::SessionEventEnvelope],
) -> Result<(), PureError> {
    let session_id = snapshot.session_id.clone();
    let active = session_view_snapshot::ActiveModel {
        session_id: Set(session_id.clone()),
        through_sequence: Set(i64_from_u64(snapshot.through_sequence)?),
        snapshot_json: Set(serde_json::to_string(snapshot)?),
        updated_at: Set(items.last().map_or(0, |event| event.emitted_at)),
    };
    if session_view_snapshot::Entity::find_by_id(session_id)
        .one(tx)
        .await
        .map_err(store_error)?
        .is_some()
    {
        active.update(tx).await.map_err(store_error)?;
    } else {
        active.insert(tx).await.map_err(store_error)?;
    }
    Ok(())
}

async fn upsert_session(
    tx: &sea_orm::DatabaseTransaction,
    agent_id: &str,
    state: &AgentDurableState,
) -> Result<(), PureError> {
    let session = &state.session;
    let active = agent_runtime_session::ActiveModel {
        agent_id: Set(agent_id.to_string()),
        session_id: Set(session.id.to_string()),
        metadata_json: Set(serde_json::to_string(&session.metadata)?),
        context_json: Set(serde_json::to_string(session.session.items())?),
        usage_json: Set(serde_json::to_string(&session.usage)?),
        last_context_tokens: Set(session.last_context_tokens.map(i64_from_u64).transpose()?),
        trace_sequence: Set(i64_from_u64(session.trace_sequence)?),
        session_event_sequence: Set(i64_from_u64(session.session_event_sequence)?),
        updated_at: Set(state.snapshot.updated_at),
    };
    if agent_runtime_session::Entity::find_by_id(agent_id.to_string())
        .one(tx)
        .await
        .map_err(store_error)?
        .is_some()
    {
        active.update(tx).await.map_err(store_error)?;
    } else {
        active.insert(tx).await.map_err(store_error)?;
    }
    Ok(())
}

async fn replace_pending_inputs(
    tx: &sea_orm::DatabaseTransaction,
    agent_id: &str,
    state: &AgentDurableState,
) -> Result<(), PureError> {
    agent_pending_input::Entity::delete_many()
        .filter(agent_pending_input::Column::AgentId.eq(agent_id))
        .exec(tx)
        .await
        .map_err(store_error)?;
    if !state.pending_inputs.is_empty() {
        let inputs = state
            .pending_inputs
            .iter()
            .enumerate()
            .map(|(position, input)| {
                Ok(agent_pending_input::ActiveModel {
                    agent_id: Set(agent_id.to_string()),
                    queue_position: Set(i64::try_from(position).map_err(store_error)?),
                    input_json: Set(serde_json::to_string(input)?),
                })
            })
            .collect::<Result<Vec<_>, PureError>>()?;
        agent_pending_input::Entity::insert_many(inputs)
            .exec(tx)
            .await
            .map_err(store_error)?;
    }
    Ok(())
}

async fn replace_active_input(
    tx: &sea_orm::DatabaseTransaction,
    agent_id: &str,
    state: &AgentDurableState,
) -> Result<(), PureError> {
    match &state.active_input {
        Some(input) => {
            let active = agent_active_input::ActiveModel {
                agent_id: Set(agent_id.to_string()),
                input_json: Set(serde_json::to_string(input)?),
                updated_at: Set(state.snapshot.updated_at),
            };
            if agent_active_input::Entity::find_by_id(agent_id.to_string())
                .one(tx)
                .await
                .map_err(store_error)?
                .is_some()
            {
                active.update(tx).await.map_err(store_error)?;
            } else {
                active.insert(tx).await.map_err(store_error)?;
            }
        }
        None => {
            agent_active_input::Entity::delete_by_id(agent_id.to_string())
                .exec(tx)
                .await
                .map_err(store_error)?;
        }
    }
    Ok(())
}

async fn upsert_turns(
    tx: &sea_orm::DatabaseTransaction,
    agent_id: &str,
    state: &AgentDurableState,
) -> Result<(), PureError> {
    for input in &state.pending_inputs {
        upsert_turn(
            tx,
            TurnProjection {
                agent_id,
                turn_id: input.turn_id.as_str(),
                session_id: input.session_id.as_str(),
                status: "queued",
                reason: None,
                usage: &pl_model::TokenUsage::default(),
                metadata: Some(&input.metadata),
                started_at: None,
                finished_at: None,
            },
        )
        .await?;
    }
    if let Some(turn_id) = state.snapshot.active_turn_id.as_ref() {
        upsert_turn(
            tx,
            TurnProjection {
                agent_id,
                turn_id: turn_id.as_str(),
                session_id: state.session.id.as_str(),
                status: activity_label(state.snapshot.activity),
                reason: None,
                usage: &pl_model::TokenUsage::default(),
                metadata: None,
                started_at: Some(state.snapshot.updated_at),
                finished_at: None,
            },
        )
        .await?;
    }
    if let Some(outcome) = &state.snapshot.last_turn {
        upsert_turn(
            tx,
            TurnProjection {
                agent_id,
                turn_id: outcome.turn_id.as_str(),
                session_id: outcome.session_id.as_str(),
                status: outcome_label(outcome.kind),
                reason: outcome.reason.as_deref(),
                usage: &outcome.usage,
                metadata: None,
                started_at: None,
                finished_at: Some(outcome.finished_at),
            },
        )
        .await?;
    }
    Ok(())
}

struct TurnProjection<'a> {
    agent_id: &'a str,
    turn_id: &'a str,
    session_id: &'a str,
    status: &'a str,
    reason: Option<&'a str>,
    usage: &'a pl_model::TokenUsage,
    metadata: Option<&'a serde_json::Value>,
    started_at: Option<i64>,
    finished_at: Option<i64>,
}

async fn upsert_turn(
    tx: &sea_orm::DatabaseTransaction,
    turn: TurnProjection<'_>,
) -> Result<(), PureError> {
    let existing =
        agent_turn::Entity::find_by_id((turn.agent_id.to_string(), turn.turn_id.to_string()))
            .one(tx)
            .await
            .map_err(store_error)?;
    let metadata_json = turn.metadata.map(serde_json::to_string).transpose()?;
    let usage_json = serde_json::to_string(turn.usage)?;
    if let Some(existing) = existing {
        let preserved_metadata = existing.metadata_json.clone().or(metadata_json);
        let preserved_started_at = existing.started_at.or(turn.started_at);
        let mut active = existing.into_active_model();
        active.status = Set(turn.status.to_string());
        active.reason = Set(turn.reason.map(str::to_string));
        active.usage_json = Set(usage_json);
        active.metadata_json = Set(preserved_metadata);
        active.started_at = Set(preserved_started_at);
        active.finished_at = Set(turn.finished_at);
        active.update(tx).await.map_err(store_error)?;
    } else {
        agent_turn::ActiveModel {
            agent_id: Set(turn.agent_id.to_string()),
            turn_id: Set(turn.turn_id.to_string()),
            session_id: Set(turn.session_id.to_string()),
            status: Set(turn.status.to_string()),
            reason: Set(turn.reason.map(str::to_string)),
            usage_json: Set(usage_json),
            metadata_json: Set(metadata_json),
            started_at: Set(turn.started_at),
            finished_at: Set(turn.finished_at),
        }
        .insert(tx)
        .await
        .map_err(store_error)?;
    }
    Ok(())
}

fn activity_label(activity: AgentActivityState) -> &'static str {
    match activity {
        AgentActivityState::Idle => "idle",
        AgentActivityState::Queued => "queued",
        AgentActivityState::Running => "running",
        AgentActivityState::WaitingTool => "waiting_tool",
        AgentActivityState::WaitingInteraction => "waiting_interaction",
        AgentActivityState::Cancelling => "cancelling",
    }
}

fn outcome_label(outcome: TurnOutcomeKind) -> &'static str {
    match outcome {
        TurnOutcomeKind::Completed => "completed",
        TurnOutcomeKind::Cancelled => "cancelled",
        TurnOutcomeKind::Failed => "failed",
        TurnOutcomeKind::BudgetLimited => "budget_limited",
    }
}

fn u64_from_i64(value: i64) -> Result<u64, PureError> {
    u64::try_from(value).map_err(|error| store_error(error.to_string()))
}

fn i64_from_u64(value: u64) -> Result<i64, PureError> {
    i64::try_from(value).map_err(|error| store_error(error.to_string()))
}

fn store_error(error: impl std::fmt::Display) -> PureError {
    PureError::MemoryError(error.to_string())
}

#[cfg(test)]
mod tests {
    use pl_protocol::{
        SessionEventEnvelope, SessionEventKind, SessionEventPosition, SessionMessage,
        SessionMessageRole, SessionMessageStatus,
    };
    use pretty_assertions::assert_eq;

    use super::*;

    #[tokio::test]
    async fn active_mailbox_input_is_restored_and_removed_with_runtime_state() {
        let store = StudioStore::open_memory().await.unwrap();
        let repository = StudioAgentRepository::new(store);
        let agent_id = pl_core::AgentId::new("agent-active-mailbox").unwrap();
        let turn_id = pl_core::TurnId::new("turn-active-mailbox").unwrap();
        let session_id = pl_core::SessionId::new("session-active-mailbox").unwrap();
        let active_input = pl_core::DurableMailboxEnvelope {
            mail_id: "mail-active-1".to_string(),
            turn_id: turn_id.clone(),
            session_id: session_id.clone(),
            message: "durable active input".to_string(),
            metadata: serde_json::json!({"source": "test"}),
            presentation: pl_core::MailboxPresentation::User,
            delivery_state: pl_core::MailboxDeliveryState::Consumed {
                turn_id,
                checkpoint_seq: 3,
            },
            queued_at: 10,
        };
        let state = AgentDurableState {
            snapshot: pl_core::AgentSnapshot {
                identity: pl_core::AgentIdentity {
                    id: agent_id.clone(),
                    parent_id: None,
                    role: pl_core::AgentRoleId::new("executor").unwrap(),
                    depth: 0,
                },
                lifecycle: pl_core::AgentLifecycleState::Active,
                activity: AgentActivityState::Running,
                active_turn_id: Some(active_input.turn_id.clone()),
                pending_inputs: 0,
                progress: None,
                last_turn: None,
                revision: 1,
                event_sequence: 0,
                updated_at: 10,
            },
            session: AgentSessionState::empty(session_id),
            pending_inputs: VecDeque::new(),
            active_input: Some(active_input.clone()),
        };

        assert_eq!(
            repository
                .commit(SessionHistoryCommit {
                    agent_id: agent_id.clone(),
                    expected_revision: None,
                    next_state: state.clone(),
                    facts: pl_core::DurableCommitFacts::from_state(
                        &state,
                        Vec::new(),
                        Vec::new(),
                        None,
                        None,
                    ),
                    mutation: pl_core::AgentStateMutation::SnapshotAndQueue,
                })
                .await
                .unwrap(),
            AgentCommitOutcome::Applied
        );
        let restored = repository.restore_runtime().await.unwrap();
        assert_eq!(restored[0].state.active_input, Some(active_input));

        let mut cleared = restored[0].state.clone();
        cleared.snapshot.revision = 2;
        cleared.active_input = None;
        assert_eq!(
            repository
                .commit(SessionHistoryCommit {
                    agent_id: agent_id.clone(),
                    expected_revision: Some(1),
                    next_state: cleared.clone(),
                    facts: pl_core::DurableCommitFacts::from_state(
                        &cleared,
                        Vec::new(),
                        Vec::new(),
                        None,
                        None,
                    ),
                    mutation: pl_core::AgentStateMutation::SnapshotAndQueue,
                })
                .await
                .unwrap(),
            AgentCommitOutcome::Applied
        );
        assert_eq!(
            repository.restore_runtime().await.unwrap()[0]
                .state
                .active_input,
            None
        );
    }

    #[tokio::test]
    async fn restore_replays_history_suffix_when_projection_is_missing() {
        let store = StudioStore::open_memory().await.unwrap();
        let repository = StudioAgentRepository::new(store.clone());
        let agent_id = pl_core::AgentId::new("agent-history-recovery").unwrap();
        let session_id = pl_core::SessionId::new("session-history-recovery").unwrap();
        let turn_id = pl_core::TurnId::new("turn-history-recovery").unwrap();
        let mut state = AgentDurableState {
            snapshot: pl_core::AgentSnapshot {
                identity: pl_core::AgentIdentity {
                    id: agent_id.clone(),
                    parent_id: None,
                    role: pl_core::AgentRoleId::new("planner").unwrap(),
                    depth: 0,
                },
                lifecycle: pl_core::AgentLifecycleState::Active,
                activity: AgentActivityState::Idle,
                active_turn_id: None,
                pending_inputs: 0,
                progress: None,
                last_turn: None,
                revision: 1,
                event_sequence: 0,
                updated_at: 20,
            },
            session: AgentSessionState::empty(session_id.clone()),
            pending_inputs: VecDeque::new(),
            active_input: None,
        };
        state.session.session_event_sequence = 1;
        let event = SessionEventEnvelope {
            event_id: "history-event-1".to_string(),
            session_id: session_id.to_string(),
            source_agent_id: Some(agent_id.to_string()),
            turn_id: Some(turn_id.to_string()),
            emitted_at: 20,
            position: SessionEventPosition::Durable { sequence: 1 },
            kind: SessionEventKind::MessageChanged {
                message: Box::new(SessionMessage {
                    message_id: "history-message-1".to_string(),
                    session_id: session_id.to_string(),
                    turn_id: turn_id.to_string(),
                    role: SessionMessageRole::Assistant,
                    status: SessionMessageStatus::Completed,
                    created_at: 10,
                    updated_at: 20,
                    completed_at: Some(20),
                    error: None,
                    metadata: serde_json::json!({}),
                }),
            },
        };
        assert_eq!(
            repository
                .commit(SessionHistoryCommit {
                    agent_id: agent_id.clone(),
                    expected_revision: None,
                    next_state: state.clone(),
                    facts: pl_core::DurableCommitFacts {
                        session_id: session_id.clone(),
                        turn_id: Some(turn_id),
                        through_sequence: 1,
                        revision: 1,
                        items: vec![event.clone()],
                        turn_transition: None,
                        context: None,
                        projection_snapshot: None,
                        runtime_events: Vec::new(),
                        trace_events: Vec::new(),
                    },
                    mutation: pl_core::AgentStateMutation::SnapshotAndQueue,
                })
                .await
                .unwrap(),
            AgentCommitOutcome::Applied
        );
        repository.barrier().await.unwrap();

        let restored = StudioAgentRepository::new(store)
            .restore_runtime()
            .await
            .unwrap();
        let projection = restored[0]
            .session_projection
            .as_ref()
            .expect("history suffix should rebuild a missing projection");
        assert_eq!(projection.snapshot.through_sequence, 1);
        assert_eq!(
            projection.snapshot.messages[0].message_id,
            "history-message-1"
        );
        assert_eq!(projection.retained_durable_events, vec![event]);
    }
}
