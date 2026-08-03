use std::collections::VecDeque;

use crate::{ModelContextItem, PureError};
use pl_core::{
    AgentActivityState, AgentCommit, AgentCommitOutcome, AgentDurableState, AgentSession,
    AgentSessionState, AgentStateRepository, PendingAgentInput, RestoredAgentRuntime,
    RestoredSessionProjection, SessionId, SessionProjectionCommit, TurnOutcomeKind,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, QueryResult, Statement, TransactionTrait};

use crate::studio::StudioStore;

/// Studio SQLite 对 PL canonical runtime state 的 CAS repository。
#[derive(Clone)]
pub(in crate::studio) struct StudioAgentRepository {
    store: StudioStore,
}

impl StudioAgentRepository {
    pub(super) fn new(store: StudioStore) -> Self {
        Self { store }
    }
}

impl AgentStateRepository for StudioAgentRepository {
    type Error = PureError;

    async fn restore_runtime(&self) -> Result<Vec<RestoredAgentRuntime>, Self::Error> {
        let states = self
            .store
            .database()
            .query_all(statement(
                "SELECT agent_id, snapshot_json FROM agent_runtime_states ORDER BY agent_id",
                [],
            ))
            .await
            .map_err(store_error)?;
        let mut restored = Vec::with_capacity(states.len());
        for row in states {
            let agent_id = text(&row, "agent_id")?;
            let snapshot = serde_json::from_str(&text(&row, "snapshot_json")?)?;
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

    async fn commit(&self, commit: AgentCommit) -> Result<AgentCommitOutcome, Self::Error> {
        let agent_id = commit.agent_id.to_string();
        let tx = self.store.database().begin().await.map_err(store_error)?;
        let actual_revision = tx
            .query_one(statement(
                "SELECT revision FROM agent_runtime_states WHERE agent_id = ?",
                [agent_id.clone().into()],
            ))
            .await
            .map_err(store_error)?
            .map(|row| integer_u64(&row, "revision"))
            .transpose()?;
        if actual_revision != commit.expected_revision {
            tx.rollback().await.map_err(store_error)?;
            return Ok(AgentCommitOutcome::RevisionConflict { actual_revision });
        }

        let snapshot_json = serde_json::to_string(&commit.next_state.snapshot)?;
        tx.execute(statement(
            "INSERT INTO agent_runtime_states (agent_id, revision, snapshot_json, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(agent_id) DO UPDATE SET
               revision = excluded.revision,
               snapshot_json = excluded.snapshot_json,
               updated_at = excluded.updated_at",
            [
                agent_id.clone().into(),
                i64_from_u64(commit.next_state.snapshot.revision)?.into(),
                snapshot_json.into(),
                commit.next_state.snapshot.updated_at.into(),
            ],
        ))
        .await
        .map_err(store_error)?;

        upsert_session(&tx, &agent_id, &commit.next_state).await?;
        replace_pending_inputs(&tx, &agent_id, &commit.next_state).await?;
        replace_active_input(&tx, &agent_id, &commit.next_state).await?;
        upsert_turns(&tx, &agent_id, &commit.next_state).await?;
        for event in &commit.events {
            tx.execute(statement(
                "INSERT OR REPLACE INTO agent_framework_events
                 (agent_id, sequence, payload_json, created_at) VALUES (?, ?, ?, ?)",
                [
                    agent_id.clone().into(),
                    i64_from_u64(event.sequence)?.into(),
                    serde_json::to_string(event)?.into(),
                    event.created_at.into(),
                ],
            ))
            .await
            .map_err(store_error)?;
        }
        for trace in &commit.trace_events {
            tx.execute(statement(
                "INSERT OR REPLACE INTO agent_runtime_traces
                 (agent_id, session_id, sequence, payload_json, created_at)
                 VALUES (?, ?, ?, ?, ?)",
                [
                    agent_id.clone().into(),
                    trace.session_id.clone().into(),
                    i64_from_u64(trace.sequence)?.into(),
                    serde_json::to_string(trace)?.into(),
                    trace.timestamp.into(),
                ],
            ))
            .await
            .map_err(store_error)?;
        }
        if let Some(projection) = &commit.session_projection {
            persist_session_projection(&tx, projection).await?;
        }
        tx.commit().await.map_err(store_error)?;
        Ok(AgentCommitOutcome::Applied)
    }
}

impl StudioAgentRepository {
    async fn restore_session(&self, agent_id: &str) -> Result<AgentSessionState, PureError> {
        self.store
            .database()
            .query_one(statement(
                "SELECT session_id, metadata_json, context_json, usage_json, last_context_tokens,
                        trace_sequence, session_event_sequence
                 FROM agent_runtime_sessions WHERE agent_id = ?",
                [agent_id.to_string().into()],
            ))
            .await
            .map_err(store_error)?
            .ok_or_else(|| store_error(format!("agent {agent_id} has no canonical session")))
            .and_then(|row| {
                let id = SessionId::new(text(&row, "session_id")?)?;
                let items: Vec<ModelContextItem> =
                    serde_json::from_str(&text(&row, "context_json")?)?;
                let last_context_tokens = optional_i64(&row, "last_context_tokens")?
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|error| store_error(error.to_string()))?;
                Ok(AgentSessionState {
                    id,
                    metadata: serde_json::from_str(&text(&row, "metadata_json")?)?,
                    session: AgentSession::from_items(items),
                    usage: serde_json::from_str(&text(&row, "usage_json")?)?,
                    last_context_tokens,
                    trace_sequence: integer_u64(&row, "trace_sequence")?,
                    session_event_sequence: integer_u64(&row, "session_event_sequence")?,
                })
            })
    }

    async fn restore_session_projection(
        &self,
        agent_id: &str,
    ) -> Result<Option<RestoredSessionProjection>, PureError> {
        let row = self
            .store
            .database()
            .query_one(statement(
                "SELECT projection.session_id, projection.snapshot_json
                 FROM session_view_snapshots projection
                 INNER JOIN agent_runtime_sessions runtime_session
                   ON runtime_session.session_id = projection.session_id
                 WHERE runtime_session.agent_id = ?",
                [agent_id.to_string().into()],
            ))
            .await
            .map_err(store_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let session_id = text(&row, "session_id")?;
        let snapshot = serde_json::from_str(&text(&row, "snapshot_json")?)?;
        let durable_events = self
            .store
            .database()
            .query_all(statement(
                "SELECT event_json FROM session_event_journal
                 WHERE session_id = ? ORDER BY sequence",
                [session_id.into()],
            ))
            .await
            .map_err(store_error)?
            .into_iter()
            .map(|event| serde_json::from_str(&text(&event, "event_json")?).map_err(Into::into))
            .collect::<Result<_, PureError>>()?;
        Ok(Some(RestoredSessionProjection {
            snapshot,
            durable_events,
        }))
    }

    async fn restore_pending_inputs(
        &self,
        agent_id: &str,
    ) -> Result<VecDeque<PendingAgentInput>, PureError> {
        self.store
            .database()
            .query_all(statement(
                "SELECT input_json FROM agent_pending_inputs
                 WHERE agent_id = ? ORDER BY queue_position",
                [agent_id.to_string().into()],
            ))
            .await
            .map_err(store_error)?
            .into_iter()
            .map(|row| serde_json::from_str(&text(&row, "input_json")?).map_err(Into::into))
            .collect()
    }

    async fn restore_active_input(
        &self,
        agent_id: &str,
    ) -> Result<Option<PendingAgentInput>, PureError> {
        self.store
            .database()
            .query_one(statement(
                "SELECT input_json FROM agent_active_inputs WHERE agent_id = ?",
                [agent_id.to_string().into()],
            ))
            .await
            .map_err(store_error)?
            .map(|row| serde_json::from_str(&text(&row, "input_json")?).map_err(Into::into))
            .transpose()
    }
}

async fn persist_session_projection(
    tx: &sea_orm::DatabaseTransaction,
    projection: &SessionProjectionCommit,
) -> Result<(), PureError> {
    let session_id = projection.snapshot.session_id.clone();
    tx.execute(statement(
        "INSERT INTO session_view_snapshots
         (session_id, through_sequence, snapshot_json, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(session_id) DO UPDATE SET
           through_sequence = excluded.through_sequence,
           snapshot_json = excluded.snapshot_json,
           updated_at = excluded.updated_at",
        [
            session_id.clone().into(),
            i64_from_u64(projection.snapshot.through_sequence)?.into(),
            serde_json::to_string(&projection.snapshot)?.into(),
            projection
                .durable_events
                .last()
                .map_or(0, |event| event.emitted_at)
                .into(),
        ],
    ))
    .await
    .map_err(store_error)?;
    for event in &projection.durable_events {
        let sequence = event.position.durable_sequence().ok_or_else(|| {
            store_error("session projection commit contains transient event".to_string())
        })?;
        tx.execute(statement(
            "INSERT OR REPLACE INTO session_event_journal
             (session_id, sequence, event_json, emitted_at) VALUES (?, ?, ?, ?)",
            [
                session_id.clone().into(),
                i64_from_u64(sequence)?.into(),
                serde_json::to_string(event)?.into(),
                event.emitted_at.into(),
            ],
        ))
        .await
        .map_err(store_error)?;
    }
    let retain_after = projection.snapshot.through_sequence.saturating_sub(4096);
    tx.execute(statement(
        "DELETE FROM session_event_journal WHERE session_id = ? AND sequence <= ?",
        [session_id.into(), i64_from_u64(retain_after)?.into()],
    ))
    .await
    .map_err(store_error)?;
    Ok(())
}

async fn upsert_session(
    tx: &sea_orm::DatabaseTransaction,
    agent_id: &str,
    state: &AgentDurableState,
) -> Result<(), PureError> {
    let session = &state.session;
    tx.execute(statement(
        "INSERT INTO agent_runtime_sessions
         (agent_id, session_id, metadata_json, context_json, usage_json,
          last_context_tokens, trace_sequence, session_event_sequence, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(agent_id) DO UPDATE SET
           session_id = excluded.session_id,
           metadata_json = excluded.metadata_json,
           context_json = excluded.context_json,
           usage_json = excluded.usage_json,
           last_context_tokens = excluded.last_context_tokens,
           trace_sequence = excluded.trace_sequence,
           session_event_sequence = excluded.session_event_sequence,
           updated_at = excluded.updated_at",
        [
            agent_id.to_string().into(),
            session.id.to_string().into(),
            serde_json::to_string(&session.metadata)?.into(),
            serde_json::to_string(session.session.items())?.into(),
            serde_json::to_string(&session.usage)?.into(),
            session
                .last_context_tokens
                .map(i64_from_u64)
                .transpose()?
                .into(),
            i64_from_u64(session.trace_sequence)?.into(),
            i64_from_u64(session.session_event_sequence)?.into(),
            state.snapshot.updated_at.into(),
        ],
    ))
    .await
    .map_err(store_error)?;
    Ok(())
}

async fn replace_pending_inputs(
    tx: &sea_orm::DatabaseTransaction,
    agent_id: &str,
    state: &AgentDurableState,
) -> Result<(), PureError> {
    tx.execute(statement(
        "DELETE FROM agent_pending_inputs WHERE agent_id = ?",
        [agent_id.to_string().into()],
    ))
    .await
    .map_err(store_error)?;
    for (position, input) in state.pending_inputs.iter().enumerate() {
        tx.execute(statement(
            "INSERT INTO agent_pending_inputs (agent_id, queue_position, input_json)
             VALUES (?, ?, ?)",
            [
                agent_id.to_string().into(),
                i64::try_from(position)
                    .map_err(|error| store_error(error.to_string()))?
                    .into(),
                serde_json::to_string(input)?.into(),
            ],
        ))
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
            tx.execute(statement(
                "INSERT INTO agent_active_inputs (agent_id, input_json, updated_at)
                 VALUES (?, ?, ?)
                 ON CONFLICT(agent_id) DO UPDATE SET
                   input_json = excluded.input_json,
                   updated_at = excluded.updated_at",
                [
                    agent_id.to_string().into(),
                    serde_json::to_string(input)?.into(),
                    state.snapshot.updated_at.into(),
                ],
            ))
            .await
            .map_err(store_error)?;
        }
        None => {
            tx.execute(statement(
                "DELETE FROM agent_active_inputs WHERE agent_id = ?",
                [agent_id.to_string().into()],
            ))
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
            agent_id,
            input.turn_id.as_str(),
            input.session_id.as_str(),
            "queued",
            None,
            &pl_model::TokenUsage::default(),
            Some(&input.metadata),
            None,
            None,
        )
        .await?;
    }
    if let Some(turn_id) = state.snapshot.active_turn_id.as_ref() {
        upsert_turn(
            tx,
            agent_id,
            turn_id.as_str(),
            state.session.id.as_str(),
            activity_label(state.snapshot.activity),
            None,
            &pl_model::TokenUsage::default(),
            None,
            Some(state.snapshot.updated_at),
            None,
        )
        .await?;
    }
    if let Some(outcome) = &state.snapshot.last_turn {
        upsert_turn(
            tx,
            agent_id,
            outcome.turn_id.as_str(),
            outcome.session_id.as_str(),
            outcome_label(outcome.kind),
            outcome.reason.as_deref(),
            &outcome.usage,
            None,
            None,
            Some(outcome.finished_at),
        )
        .await?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn upsert_turn(
    tx: &sea_orm::DatabaseTransaction,
    agent_id: &str,
    turn_id: &str,
    session_id: &str,
    status: &str,
    reason: Option<&str>,
    usage: &pl_model::TokenUsage,
    metadata: Option<&serde_json::Value>,
    started_at: Option<i64>,
    finished_at: Option<i64>,
) -> Result<(), PureError> {
    tx.execute(statement(
        "INSERT INTO agent_turns
         (agent_id, turn_id, session_id, status, reason, usage_json, metadata_json,
          started_at, finished_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(agent_id, turn_id) DO UPDATE SET
           status = excluded.status,
           reason = excluded.reason,
           usage_json = excluded.usage_json,
           metadata_json = COALESCE(agent_turns.metadata_json, excluded.metadata_json),
           started_at = COALESCE(agent_turns.started_at, excluded.started_at),
           finished_at = excluded.finished_at",
        [
            agent_id.to_string().into(),
            turn_id.to_string().into(),
            session_id.to_string().into(),
            status.to_string().into(),
            reason.map(str::to_string).into(),
            serde_json::to_string(usage)?.into(),
            metadata.map(serde_json::to_string).transpose()?.into(),
            started_at.into(),
            finished_at.into(),
        ],
    ))
    .await
    .map_err(store_error)?;
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

fn statement<const N: usize>(sql: &str, values: [sea_orm::Value; N]) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values)
}

fn text(row: &QueryResult, column: &str) -> Result<String, PureError> {
    row.try_get("", column).map_err(store_error)
}

fn integer_u64(row: &QueryResult, column: &str) -> Result<u64, PureError> {
    let value: i64 = row.try_get("", column).map_err(store_error)?;
    u64::try_from(value).map_err(|error| store_error(error.to_string()))
}

fn optional_i64(row: &QueryResult, column: &str) -> Result<Option<i64>, PureError> {
    row.try_get("", column).map_err(store_error)
}

fn i64_from_u64(value: u64) -> Result<i64, PureError> {
    i64::try_from(value).map_err(|error| store_error(error.to_string()))
}

fn store_error(error: impl std::fmt::Display) -> PureError {
    PureError::MemoryError(error.to_string())
}

#[cfg(test)]
mod tests {
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
                .commit(AgentCommit {
                    agent_id: agent_id.clone(),
                    expected_revision: None,
                    next_state: state,
                    events: Vec::new(),
                    trace_events: Vec::new(),
                    session_projection: None,
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
                .commit(AgentCommit {
                    agent_id: agent_id.clone(),
                    expected_revision: Some(1),
                    next_state: cleared,
                    events: Vec::new(),
                    trace_events: Vec::new(),
                    session_projection: None,
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
    async fn canonical_session_note_is_restored() {
        let store = StudioStore::open_memory().await.unwrap();
        let repository = StudioAgentRepository::new(store.clone());
        let agent_id = pl_core::AgentId::new("agent-note").unwrap();
        let session_id = pl_core::SessionId::new("session-note").unwrap();
        let mut session = AgentSession::new();
        session.replace_session_note(pl_protocol::SessionNote {
            revision: 3,
            content: "durable note".to_string(),
            content_hash: pl_core::canonical_content_hash(b"durable note"),
            updated_at: 1,
        });
        let session_state = AgentSessionState {
            id: session_id.clone(),
            metadata: serde_json::Value::Null,
            session,
            usage: pl_model::TokenUsage::default(),
            last_context_tokens: None,
            trace_sequence: 0,
            session_event_sequence: 0,
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
                activity: AgentActivityState::Idle,
                active_turn_id: None,
                pending_inputs: 0,
                progress: None,
                last_turn: None,
                revision: 1,
                event_sequence: 0,
                updated_at: 1,
            },
            session: session_state,
            pending_inputs: VecDeque::new(),
            active_input: None,
        };

        let tx = store.database().begin().await.unwrap();
        upsert_session(&tx, agent_id.as_str(), &state)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let restored = repository.restore_session(agent_id.as_str()).await.unwrap();
        assert_eq!(
            restored.session.session_note().unwrap().content,
            "durable note"
        );
        assert_eq!(restored.id, session_id);
    }
}
