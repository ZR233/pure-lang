use std::collections::{BTreeMap, VecDeque};

use crate::{ModelContextItem, PureError};
use pl_core::{
    AcceptedAgentWake, AgentActivityState, AgentCommit, AgentCommitOutcome, AgentDurableState,
    AgentSession, AgentSessionState, AgentStateRepository, AgentWakeId, PendingAgentInput,
    RestoredAgentRuntime, RestoredSessionProjection, SessionId, SessionProjectionCommit,
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
            let accepted_wakes = self.restore_accepted_wakes(&agent_id).await?;
            let session_projections = self.restore_session_projections(&agent_id).await?;
            restored.push(RestoredAgentRuntime {
                state: AgentDurableState {
                    snapshot,
                    sessions,
                    pending_inputs,
                    accepted_wakes,
                },
                session_projections,
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
        replace_accepted_wakes(&tx, &agent_id, &commit.next_state).await?;
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
    async fn restore_sessions(
        &self,
        agent_id: &str,
    ) -> Result<BTreeMap<SessionId, AgentSessionState>, PureError> {
        self.store
            .database()
            .query_all(statement(
                "SELECT session_id, metadata_json, context_json, usage_json, last_context_tokens,
                        trace_sequence, session_event_sequence
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
                        session_event_sequence: integer_u64(&row, "session_event_sequence")?,
                    },
                ))
            })
            .collect()
    }

    async fn restore_session_projections(
        &self,
        agent_id: &str,
    ) -> Result<Vec<RestoredSessionProjection>, PureError> {
        let rows = self
            .store
            .database()
            .query_all(statement(
                "SELECT projection.session_id, projection.snapshot_json
                 FROM session_view_snapshots projection
                 INNER JOIN agent_runtime_sessions runtime_session
                   ON runtime_session.session_id = projection.session_id
                 WHERE runtime_session.agent_id = ?
                 ORDER BY projection.session_id",
                [agent_id.to_string().into()],
            ))
            .await
            .map_err(store_error)?;
        let mut projections = Vec::with_capacity(rows.len());
        for row in rows {
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
            projections.push(RestoredSessionProjection {
                snapshot,
                durable_events,
            });
        }
        Ok(projections)
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

    async fn restore_accepted_wakes(
        &self,
        agent_id: &str,
    ) -> Result<BTreeMap<AgentWakeId, AcceptedAgentWake>, PureError> {
        self.store
            .database()
            .query_all(statement(
                "SELECT receipt_json FROM agent_wake_receipts
                 WHERE agent_id = ? ORDER BY accepted_at, wake_id",
                [agent_id.to_string().into()],
            ))
            .await
            .map_err(store_error)?
            .into_iter()
            .map(|row| {
                let receipt: AcceptedAgentWake =
                    serde_json::from_str(&text(&row, "receipt_json")?)?;
                Ok((receipt.wake_id.clone(), receipt))
            })
            .collect()
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
              last_context_tokens, trace_sequence, session_event_sequence, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
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

async fn replace_accepted_wakes(
    tx: &sea_orm::DatabaseTransaction,
    agent_id: &str,
    state: &AgentDurableState,
) -> Result<(), PureError> {
    for receipt in state.accepted_wakes.values() {
        tx.execute(statement(
            "INSERT OR IGNORE INTO agent_wake_receipts
             (agent_id, wake_id, receipt_json, accepted_at) VALUES (?, ?, ?, ?)",
            [
                agent_id.to_string().into(),
                receipt.wake_id.to_string().into(),
                serde_json::to_string(receipt)?.into(),
                receipt.accepted_at.into(),
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
        AgentActivityState::WaitingAgents => "waiting_agents",
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
    async fn accepted_wake_receipt_is_restored_with_runtime_state() {
        let store = StudioStore::open_memory().await.unwrap();
        let repository = StudioAgentRepository::new(store);
        let agent_id = pl_core::AgentId::new("agent-wake-receipt").unwrap();
        let wake_id = AgentWakeId::new("wake-delivery-1").unwrap();
        let turn_id = pl_core::TurnId::new("turn-delivery-1").unwrap();
        let receipt = AcceptedAgentWake {
            wake_id: wake_id.clone(),
            turn_id: turn_id.clone(),
            signal_ids: vec!["delivery:outcome-1".to_string()],
            accepted_at: 10,
        };
        let state = AgentDurableState {
            snapshot: pl_core::AgentSnapshot {
                identity: pl_core::AgentIdentity {
                    id: agent_id.clone(),
                    parent_id: None,
                    role: pl_core::AgentRoleId::new("planner").unwrap(),
                    depth: 0,
                },
                wake_policy: pl_core::AgentWakePolicy::RuntimeTerminal,
                lifecycle: pl_core::AgentLifecycleState::Active,
                activity: AgentActivityState::Idle,
                active_turn_id: None,
                active_session_id: None,
                pending_inputs: 0,
                last_turn: None,
                revision: 1,
                event_sequence: 1,
                updated_at: 10,
            },
            sessions: BTreeMap::new(),
            pending_inputs: VecDeque::new(),
            accepted_wakes: BTreeMap::from([(wake_id.clone(), receipt.clone())]),
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

        assert_eq!(restored.len(), 1);
        assert_eq!(
            restored[0].state.accepted_wakes.get(&wake_id),
            Some(&receipt)
        );
        assert_eq!(restored[0].state.accepted_wakes[&wake_id].turn_id, turn_id);
    }

    #[tokio::test]
    async fn session_note_is_restored_and_removed_with_its_session() {
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
        let mut state = AgentDurableState {
            snapshot: pl_core::AgentSnapshot {
                identity: pl_core::AgentIdentity {
                    id: agent_id.clone(),
                    parent_id: None,
                    role: pl_core::AgentRoleId::new("executor").unwrap(),
                    depth: 0,
                },
                wake_policy: pl_core::AgentWakePolicy::RuntimeTerminal,
                lifecycle: pl_core::AgentLifecycleState::Active,
                activity: AgentActivityState::Idle,
                active_turn_id: None,
                active_session_id: None,
                pending_inputs: 0,
                last_turn: None,
                revision: 1,
                event_sequence: 0,
                updated_at: 1,
            },
            sessions: BTreeMap::from([(session_id.clone(), session_state)]),
            pending_inputs: VecDeque::new(),
            accepted_wakes: BTreeMap::new(),
        };

        let tx = store.database().begin().await.unwrap();
        replace_sessions(&tx, agent_id.as_str(), &state)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let restored = repository
            .restore_sessions(agent_id.as_str())
            .await
            .unwrap();
        assert_eq!(
            restored[&session_id]
                .session
                .session_note()
                .unwrap()
                .content,
            "durable note"
        );

        state.sessions.clear();
        let tx = store.database().begin().await.unwrap();
        replace_sessions(&tx, agent_id.as_str(), &state)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        assert!(
            repository
                .restore_sessions(agent_id.as_str())
                .await
                .unwrap()
                .is_empty()
        );
    }
}
