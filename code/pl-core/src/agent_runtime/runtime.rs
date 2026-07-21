use std::time::Duration;

use super::coordinator::spawn_coordinator;
use super::host::{AgentCommitObserver, AgentStateRepository};
use super::state::{AgentRuntimeError, unix_timestamp};
use super::{
    AgentActivityState, AgentCommit, AgentCommitOutcome, AgentCommittedEvent, AgentLifecycleState,
    AgentRuntimeEvent, AgentRuntimeEventKind, AgentRuntimeHandle, AgentRuntimeHost,
    AgentRuntimeResult, AgentTurnOutcome, RestoredAgentRuntime, SessionId, TurnId, TurnOutcomeKind,
};
use crate::session_event::{project_runtime_event, runtime_event_session_id};
use crate::{SessionEventHub, SessionEventHubHandle, SessionEventOptions};

/// runtime 启动时如何处理 repository 恢复出的 pending inputs。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RestoredInputPolicy {
    /// host 已完成外部资源恢复，可立即按 FIFO 执行。
    #[default]
    Start,
    /// 暂停恢复队列，等待产品调用 `start_restored_inputs` 放行。
    Hold,
}

/// agent runtime 的并发和取消参数。
#[derive(Debug, Clone, Copy)]
pub struct AgentRuntimeOptions {
    pub command_capacity: usize,
    pub cancel_grace: Duration,
    pub restored_inputs: RestoredInputPolicy,
    pub session_events: SessionEventOptions,
}

impl Default for AgentRuntimeOptions {
    fn default() -> Self {
        Self {
            command_capacity: 128,
            cancel_grace: Duration::from_millis(500),
            restored_inputs: RestoredInputPolicy::Start,
            session_events: SessionEventOptions::default(),
        }
    }
}

/// 持有泛型 host 的 agent runtime 所有者。
///
/// 产品通常保留该值，并把 cloneable `AgentRuntimeHandle` 交给 facade 与工具。
#[derive(Debug)]
pub struct AgentRuntime<H>
where
    H: AgentRuntimeHost,
{
    host: H,
    handle: AgentRuntimeHandle,
}

impl<H> AgentRuntime<H>
where
    H: AgentRuntimeHost,
{
    /// 从 repository 恢复 durable agents，收束遗留 Running turn，并启动 actors。
    pub async fn start(host: H, options: AgentRuntimeOptions) -> AgentRuntimeResult<Self> {
        let restored = host
            .repository()
            .restore_runtime()
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        let session_events = SessionEventHub::new(options.session_events);
        let session_event_handle = session_events.handle();
        for agent in &restored {
            for projection in &agent.session_projections {
                session_event_handle
                    .replace_snapshot(
                        projection.snapshot.clone(),
                        projection.durable_events.clone(),
                    )
                    .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?;
            }
        }
        let restored = recover_interrupted_turns(&host, &session_event_handle, restored).await?;
        let handle = spawn_coordinator(host.clone(), restored, options, session_events)?;
        Ok(Self { host, handle })
    }

    /// 返回不暴露 host 泛型的 runtime 命令句柄。
    pub fn handle(&self) -> AgentRuntimeHandle {
        self.handle.clone()
    }

    /// 返回 host，供产品 facade 读取自身只读能力。
    pub fn host(&self) -> &H {
        &self.host
    }

    /// 停止 runtime。
    pub async fn shutdown(&self) -> AgentRuntimeResult<()> {
        self.handle.shutdown().await
    }
}

async fn recover_interrupted_turns<H>(
    host: &H,
    session_events: &SessionEventHubHandle,
    restored: Vec<RestoredAgentRuntime>,
) -> AgentRuntimeResult<Vec<RestoredAgentRuntime>>
where
    H: AgentRuntimeHost,
{
    let mut recovered = Vec::with_capacity(restored.len());
    for mut agent in restored {
        let interrupted = agent.state.snapshot.active_turn_id.is_some()
            || agent.state.snapshot.active_session_id.is_some()
            || matches!(
                agent.state.snapshot.activity,
                AgentActivityState::Running
                    | AgentActivityState::WaitingTool
                    | AgentActivityState::WaitingInteraction
            );
        if !interrupted {
            recovered.push(agent);
            continue;
        }
        let turn_id = agent
            .state
            .snapshot
            .active_turn_id
            .clone()
            .unwrap_or_else(TurnId::generate);
        let session_id = agent
            .state
            .snapshot
            .active_session_id
            .clone()
            .or_else(|| agent.state.sessions.keys().next().cloned())
            .unwrap_or_else(SessionId::generate);
        let outcome = AgentTurnOutcome {
            turn_id,
            session_id,
            kind: TurnOutcomeKind::Cancelled,
            reason: Some("runtime_restarted".to_string()),
            usage: pl_model::TokenUsage::default(),
            finished_at: unix_timestamp(),
        };
        let expected_revision = agent.state.snapshot.revision;
        agent.state.snapshot.revision = expected_revision.saturating_add(1);
        agent.state.snapshot.event_sequence = agent.state.snapshot.event_sequence.saturating_add(1);
        agent.state.snapshot.active_turn_id = None;
        agent.state.snapshot.active_session_id = None;
        agent.state.snapshot.last_turn = Some(outcome.clone());
        agent.state.snapshot.activity = if agent.state.pending_inputs.is_empty() {
            AgentActivityState::Idle
        } else {
            AgentActivityState::Queued
        };
        if agent.state.snapshot.lifecycle != AgentLifecycleState::Active {
            agent.state.snapshot.activity = AgentActivityState::Idle;
        }
        agent.state.snapshot.updated_at = unix_timestamp();
        let event = AgentRuntimeEvent {
            agent_id: agent.state.snapshot.identity.id.clone(),
            sequence: agent.state.snapshot.event_sequence,
            created_at: agent.state.snapshot.updated_at,
            kind: AgentRuntimeEventKind::RecoveryCancelledTurn {
                outcome,
                snapshot: agent.state.snapshot.clone(),
            },
        };
        let session_id = runtime_event_session_id(&event)
            .expect("recovery cancellation always belongs to a session")
            .to_string();
        let session_key = SessionId::new(session_id.clone())
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        let sequence = agent
            .state
            .sessions
            .get(&session_key)
            .map_or(0, |session| session.session_event_sequence);
        let projected = project_runtime_event(&event, sequence);
        let durable_session_events = projected.durable_events();
        let projection = super::SessionProjectionCommit {
            snapshot: session_events
                .project_durable(&session_id, &durable_session_events)
                .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?,
            durable_events: durable_session_events,
        };
        if let Some(session) = agent.state.sessions.get_mut(&session_key) {
            session.session_event_sequence = projected.through_sequence;
        }
        let commit_outcome = host
            .repository()
            .commit(AgentCommit {
                agent_id: agent.state.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: agent.state.clone(),
                events: vec![event.clone()],
                trace_events: Vec::new(),
                session_projection: Some(projection.clone()),
                mutation: super::AgentStateMutation::SnapshotAndQueue,
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match commit_outcome {
            AgentCommitOutcome::Applied => {
                session_events
                    .publish_batch(projected.events.clone())
                    .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?;
                host.observer()
                    .publish(AgentCommittedEvent {
                        agent_id: event.agent_id.clone(),
                        session_id: Some(session_key),
                        turn_id: projected
                            .events
                            .first()
                            .and_then(|session_event| session_event.turn_id.clone())
                            .and_then(|value| TurnId::new(value).ok()),
                        runtime_events: vec![event],
                        trace_events: Vec::new(),
                        session_events: projected.events,
                    })
                    .await
            }
            AgentCommitOutcome::RevisionConflict { actual_revision } => {
                return Err(AgentRuntimeError::RevisionConflict {
                    expected: Some(expected_revision),
                    actual: actual_revision,
                });
            }
        }
        if let Some(restored_projection) = agent
            .session_projections
            .iter_mut()
            .find(|restored| restored.snapshot.session_id == session_id)
        {
            restored_projection.snapshot = projection.snapshot;
            restored_projection
                .durable_events
                .extend(projection.durable_events);
        } else {
            agent
                .session_projections
                .push(super::RestoredSessionProjection {
                    snapshot: projection.snapshot,
                    durable_events: projection.durable_events,
                });
        }
        recovered.push(agent);
    }
    Ok(recovered)
}
