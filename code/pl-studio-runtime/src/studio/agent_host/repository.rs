use std::collections::{BTreeMap, VecDeque};

use crate::{ModelContextItem, PureError};
use pl_core::{
    AgentActivityState, AgentCommit, AgentCommitOutcome, AgentDurableState, AgentSession,
    AgentSessionState, AgentStateRepository, PendingAgentInput, RestoredAgentRuntime, SessionId,
    TurnOutcomeKind,
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
            let sessions = self.restore_sessions(&agent_id).await?;
            let pending_inputs = self.restore_pending_inputs(&agent_id).await?;
            restored.push(RestoredAgentRuntime {
                state: AgentDurableState {
                    snapshot,
                    sessions,
                    pending_inputs,
                },
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

        replace_sessions(&tx, &agent_id, &commit.next_state).await?;
        replace_pending_inputs(&tx, &agent_id, &commit.next_state).await?;
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
        tx.commit().await.map_err(store_error)?;
        Ok(AgentCommitOutcome::Applied)
    }
}

impl StudioAgentRepository {
    async fn restore_sessions(
        &self,
        agent_id: &str,
    ) -> Result<BTreeMap<SessionId, AgentSessionState>, PureError> {
        self.store
            .database()
            .query_all(statement(
                "SELECT session_id, metadata_json, context_json, usage_json, last_context_tokens,
                        trace_sequence
                 FROM agent_runtime_sessions WHERE agent_id = ? ORDER BY session_id",
                [agent_id.to_string().into()],
            ))
            .await
            .map_err(store_error)?
            .into_iter()
            .map(|row| {
                let id = SessionId::new(text(&row, "session_id")?)?;
                let items: Vec<ModelContextItem> =
                    serde_json::from_str(&text(&row, "context_json")?)?;
                let last_context_tokens = optional_i64(&row, "last_context_tokens")?
                    .map(u64::try_from)
                    .transpose()
                    .map_err(|error| store_error(error.to_string()))?;
                Ok((
                    id.clone(),
                    AgentSessionState {
                        id,
                        metadata: serde_json::from_str(&text(&row, "metadata_json")?)?,
                        session: AgentSession::from_items(items),
                        usage: serde_json::from_str(&text(&row, "usage_json")?)?,
                        last_context_tokens,
                        trace_sequence: integer_u64(&row, "trace_sequence")?,
                    },
                ))
            })
            .collect()
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
}

async fn replace_sessions(
    tx: &sea_orm::DatabaseTransaction,
    agent_id: &str,
    state: &AgentDurableState,
) -> Result<(), PureError> {
    tx.execute(statement(
        "DELETE FROM agent_runtime_sessions WHERE agent_id = ?",
        [agent_id.to_string().into()],
    ))
    .await
    .map_err(store_error)?;
    for session in state.sessions.values() {
        tx.execute(statement(
            "INSERT INTO agent_runtime_sessions
             (agent_id, session_id, metadata_json, context_json, usage_json,
              last_context_tokens, trace_sequence, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
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
                state.snapshot.updated_at.into(),
            ],
        ))
        .await
        .map_err(store_error)?;
    }
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
    if let (Some(turn_id), Some(session_id)) = (
        state.snapshot.active_turn_id.as_ref(),
        state.snapshot.active_session_id.as_ref(),
    ) {
        upsert_turn(
            tx,
            agent_id,
            turn_id.as_str(),
            session_id.as_str(),
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
