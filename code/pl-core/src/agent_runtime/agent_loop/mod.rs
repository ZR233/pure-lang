use std::time::Duration;

use pl_trace::TraceEvent;
use tokio::sync::{mpsc, oneshot};

use super::host::{AgentCommitObserver, AgentStateRepository, SessionProjectionCommit};
use super::state::{AgentRuntimeError, unix_timestamp};
use super::{
    AgentActivityState, AgentCommit, AgentCommitOutcome, AgentCommittedEvent,
    AgentCurrentSessionSubmitRequest, AgentDurableState, AgentLifecycleState,
    AgentProgressCheckpoint, AgentProgressStage, AgentRuntimeEvent, AgentRuntimeEventKind,
    AgentRuntimeHandle, AgentRuntimeHost, AgentRuntimeResult, AgentSessionDigest, AgentSnapshot,
    AgentSubmitRequest, SessionId, TurnId,
};
use crate::session_event::{
    ObservedTurnEvent, TurnObservation, project_observation, project_runtime_event,
    project_trace_events, runtime_event_session_id,
};
use running_turn::{RunningTurn, TurnCompletion, turn_outcome};

mod checkpoint;
mod completion;
mod input;
mod lifecycle;
mod running_turn;
mod session_digest;

pub(crate) enum AgentLoopCommand {
    Submit {
        request: AgentSubmitRequest,
        reply: oneshot::Sender<AgentRuntimeResult<TurnId>>,
    },
    SubmitCurrentSession {
        root_agent_id: super::AgentId,
        request: AgentCurrentSessionSubmitRequest,
        reply: oneshot::Sender<AgentRuntimeResult<TurnId>>,
    },
    CancelTurn {
        turn_id: TurnId,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    SetActivity {
        turn_id: TurnId,
        activity: AgentActivityState,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    Checkpoint {
        checkpoint: super::AgentTurnCheckpoint,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    RecordSessionFacts {
        session_id: SessionId,
        facts: Vec<crate::SessionEventFact>,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    Snapshot {
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    ReportProgress {
        stage: AgentProgressStage,
        summary: String,
        next_step: String,
        reply: oneshot::Sender<AgentRuntimeResult<AgentProgressCheckpoint>>,
    },
    ReadSession {
        reply: oneshot::Sender<AgentRuntimeResult<AgentSessionDigest>>,
    },
    StartPendingInputs {
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    Close {
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
    },
    TurnFinished(Box<TurnCompletion>),
    Shutdown {
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
}

#[derive(Clone)]
pub(crate) struct AgentLoopHandle {
    sender: mpsc::Sender<AgentLoopCommand>,
}

impl AgentLoopHandle {
    pub(crate) async fn send(&self, command: AgentLoopCommand) -> AgentRuntimeResult<()> {
        self.sender
            .send(command)
            .await
            .map_err(|_| AgentRuntimeError::ChannelClosed)
    }
}

struct AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    host: H,
    state: AgentDurableState,
    runtime: AgentRuntimeHandle,
    sender: mpsc::Sender<AgentLoopCommand>,
    receiver: mpsc::Receiver<AgentLoopCommand>,
    trace_sender: mpsc::UnboundedSender<TraceEvent>,
    trace_receiver: mpsc::UnboundedReceiver<TraceEvent>,
    observation_sender: mpsc::UnboundedSender<ObservedTurnEvent>,
    observation_receiver: mpsc::UnboundedReceiver<ObservedTurnEvent>,
    active: Option<RunningTurn>,
    dispatch_enabled: bool,
    cancel_grace: Duration,
}

pub(crate) fn spawn_agent_loop<H>(
    host: H,
    state: AgentDurableState,
    runtime: AgentRuntimeHandle,
    cancel_grace: Duration,
    start_pending_inputs: bool,
    command_capacity: usize,
) -> AgentLoopHandle
where
    H: AgentRuntimeHost,
{
    let (sender, receiver) = mpsc::channel(command_capacity.max(1));
    let (trace_sender, trace_receiver) = mpsc::unbounded_channel();
    let (observation_sender, observation_receiver) = mpsc::unbounded_channel();
    let handle = AgentLoopHandle {
        sender: sender.clone(),
    };
    let dispatch_enabled = start_pending_inputs;
    tokio::spawn(
        AgentLoop {
            host,
            state,
            runtime,
            sender,
            receiver,
            trace_sender,
            trace_receiver,
            observation_sender,
            observation_receiver,
            active: None,
            dispatch_enabled,
            cancel_grace,
        }
        .run(),
    );
    handle
}

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    async fn run(mut self) {
        if self.dispatch_enabled
            && self.state.snapshot.lifecycle == AgentLifecycleState::Active
            && self.state.has_triggering_input()
        {
            self.begin_next_turn().await;
        }
        loop {
            tokio::select! {
                command = self.receiver.recv() => {
                    let Some(command) = command else {
                        break;
                    };
                    match command {
                        AgentLoopCommand::Submit { request, reply } => {
                            let result = self.submit(request).await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::SubmitCurrentSession {
                            root_agent_id,
                            request,
                            reply,
                        } => {
                            let result =
                                self.submit_current_session(root_agent_id, request).await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::CancelTurn { turn_id, reply } => {
                            let result = self.cancel_turn(turn_id).await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::SetActivity {
                            turn_id,
                            activity,
                            reply,
                        } => {
                            let result = self.set_activity(turn_id, activity).await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::Checkpoint { checkpoint, reply } => {
                            let result = self.checkpoint(checkpoint).await;
                            if let Err(error) = &result {
                                self.fault_in_memory(error.to_string());
                            }
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::RecordSessionFacts {
                            session_id,
                            facts,
                            reply,
                        } => {
                            let result = self.record_session_facts(session_id, facts).await;
                            if let Err(error) = &result {
                                self.fault_in_memory(error.to_string());
                            }
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::Snapshot { reply } => {
                            let _ = reply.send(Ok(self.state.snapshot.clone()));
                        }
                        AgentLoopCommand::ReportProgress {
                            stage,
                            summary,
                            next_step,
                            reply,
                        } => {
                            let result = self.report_progress(stage, summary, next_step).await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::ReadSession { reply } => {
                            let _ = reply.send(self.read_session());
                        }
                        AgentLoopCommand::StartPendingInputs { reply } => {
                            self.dispatch_enabled = true;
                            self.begin_next_turn().await;
                            let _ = reply.send(Ok(()));
                        }
                        AgentLoopCommand::Close { reply } => {
                            let result = self.close().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::TurnFinished(completion) => {
                            self.finish_turn(*completion).await;
                        }
                        AgentLoopCommand::Shutdown { reply } => {
                            let result = self.shutdown().await;
                            let _ = reply.send(result);
                            break;
                        }
                    }
                }
                trace = self.trace_receiver.recv() => {
                    let Some(trace) = trace else {
                        continue;
                    };
                    if let Err(error) = self.persist_trace_batch(vec![trace]).await {
                        self.fault_in_memory(error.to_string());
                    }
                }
                observation = self.observation_receiver.recv() => {
                    let Some(observation) = observation else {
                        continue;
                    };
                    if let Err(error) = self.persist_observation(observation).await {
                        self.fault_in_memory(error.to_string());
                    }
                }
            }
        }
        self.stop_active_turn();
    }

    async fn cancel_turn(&mut self, turn_id: TurnId) -> AgentRuntimeResult<()> {
        let Some(active) = &self.active else {
            return Err(AgentRuntimeError::NoActiveTurn(
                self.state.snapshot.identity.id.clone(),
            ));
        };
        if active.turn_id != turn_id {
            return Err(AgentRuntimeError::TurnMismatch {
                expected: active.turn_id.clone(),
                actual: turn_id,
            });
        }
        self.interrupt_active_turn("interrupted").await
    }

    async fn flush_pending_traces(&mut self) -> AgentRuntimeResult<()> {
        let mut trace_events = Vec::new();
        while let Ok(trace) = self.trace_receiver.try_recv() {
            trace_events.push(trace);
        }
        if trace_events.is_empty() {
            return Ok(());
        }
        self.persist_trace_batch(trace_events).await
    }

    pub(super) async fn flush_pending_observations(&mut self) -> AgentRuntimeResult<()> {
        let mut observations = Vec::new();
        while let Ok(observation) = self.observation_receiver.try_recv() {
            observations.push(observation);
        }
        for observation in observations {
            self.persist_observation(observation).await?;
        }
        Ok(())
    }

    pub(super) async fn persist_turn_observation(
        &mut self,
        observation: TurnObservation,
    ) -> AgentRuntimeResult<()> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        self.persist_observation(ObservedTurnEvent {
            turn_id: active.turn_id.to_string(),
            session_id: active.session_id.to_string(),
            observation,
        })
        .await
    }

    async fn persist_observation(&mut self, observed: ObservedTurnEvent) -> AgentRuntimeResult<()> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        if active.turn_id.as_str() != observed.turn_id
            || active.session_id.as_str() != observed.session_id
            || active.cancelling
        {
            return Ok(());
        }
        let session_id = active.session_id.clone();
        let turn_id = active.turn_id.clone();
        let expected_revision = self.state.snapshot.revision;
        let current = self
            .runtime
            .session_events
            .snapshot(session_id.as_str())
            .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?;
        let projected = project_observation(
            self.state.snapshot.identity.id.as_str(),
            session_id.as_str(),
            turn_id.as_str(),
            current.through_sequence,
            &current,
            observed.observation,
        );
        let durable_events = projected.durable_events();
        let projection = SessionProjectionCommit {
            snapshot: self
                .runtime
                .session_events
                .project_durable(session_id.as_str(), &durable_events)
                .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?,
            durable_events,
        };
        let mut next = self.state.clone();
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        next.session.session_event_sequence = projected.through_sequence;
        let outcome = self
            .host
            .repository()
            .commit(AgentCommit {
                agent_id: next.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: next.clone(),
                events: Vec::new(),
                trace_events: Vec::new(),
                session_projection: Some(projection),
                mutation: super::AgentStateMutation::AppendSessionEvents {
                    session_id: session_id.clone(),
                },
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match outcome {
            AgentCommitOutcome::Applied => {
                self.state = next;
                self.runtime
                    .directory
                    .store_snapshot(self.state.snapshot.clone());
                self.runtime
                    .session_events
                    .publish_batch(projected.events.clone())
                    .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?;
                self.host
                    .observer()
                    .publish(AgentCommittedEvent {
                        agent_id: self.state.snapshot.identity.id.clone(),
                        session_id: Some(session_id),
                        turn_id: Some(turn_id),
                        runtime_events: Vec::new(),
                        trace_events: Vec::new(),
                        session_events: projected.events,
                    })
                    .await;
                Ok(())
            }
            AgentCommitOutcome::RevisionConflict { actual_revision } => {
                Err(AgentRuntimeError::RevisionConflict {
                    expected: Some(expected_revision),
                    actual: actual_revision,
                })
            }
        }
    }

    async fn persist_trace_batch(
        &mut self,
        mut trace_events: Vec<TraceEvent>,
    ) -> AgentRuntimeResult<()> {
        while let Ok(trace) = self.trace_receiver.try_recv() {
            trace_events.push(trace);
        }
        let Some(active) = &self.active else {
            return Ok(());
        };
        let session_id = active.session_id.clone();
        let turn_id = active.turn_id.clone();
        if trace_events
            .iter()
            .any(|trace| trace.session_id != session_id.as_str())
        {
            return Err(AgentRuntimeError::Repository(format!(
                "trace session mismatch for agent {}",
                self.state.snapshot.identity.id
            )));
        }
        trace_events.sort_by_key(|trace| trace.sequence);
        let current_sequence = self.state.session.trace_sequence;
        trace_events.retain(|trace| trace.sequence >= current_sequence);
        if trace_events.is_empty() {
            return Ok(());
        }
        let next_trace_sequence = trace_events
            .last()
            .map(|trace| trace.sequence.saturating_add(1))
            .unwrap_or(current_sequence);
        let expected_revision = self.state.snapshot.revision;
        let session_event_sequence = self
            .runtime
            .session_events
            .snapshot(session_id.as_str())
            .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?
            .through_sequence;
        let projected = project_trace_events(
            self.state.snapshot.identity.id.as_str(),
            session_id.as_str(),
            session_event_sequence,
            &trace_events,
        );
        let durable_session_events = projected.durable_events();
        let session_projection = if durable_session_events.is_empty() {
            None
        } else {
            Some(SessionProjectionCommit {
                snapshot: self
                    .runtime
                    .session_events
                    .project_durable(session_id.as_str(), &durable_session_events)
                    .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?,
                durable_events: durable_session_events,
            })
        };
        let mut next = self.state.clone();
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        next.session.trace_sequence = next_trace_sequence;
        next.session.session_event_sequence = projected.through_sequence;
        let committed_trace_events = trace_events.clone();
        let committed_session_events = projected.events.clone();
        let result = self
            .host
            .repository()
            .commit(AgentCommit {
                agent_id: next.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: next.clone(),
                events: Vec::new(),
                trace_events,
                session_projection,
                mutation: super::AgentStateMutation::AppendTrace,
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match result {
            AgentCommitOutcome::Applied => {
                let agent_id = next.snapshot.identity.id.clone();
                self.state = next;
                self.runtime
                    .directory
                    .store_snapshot(self.state.snapshot.clone());
                self.runtime
                    .session_events
                    .publish_batch(committed_session_events.clone())
                    .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?;
                self.host
                    .observer()
                    .publish(AgentCommittedEvent {
                        agent_id,
                        session_id: Some(session_id),
                        turn_id: Some(turn_id),
                        runtime_events: Vec::new(),
                        trace_events: committed_trace_events,
                        session_events: committed_session_events,
                    })
                    .await;
                Ok(())
            }
            AgentCommitOutcome::RevisionConflict { actual_revision } => {
                Err(AgentRuntimeError::RevisionConflict {
                    expected: Some(expected_revision),
                    actual: actual_revision,
                })
            }
        }
    }

    async fn commit_transition<F>(
        &mut self,
        mut next: AgentDurableState,
        trace_events: Vec<TraceEvent>,
        event_kind: F,
    ) -> AgentRuntimeResult<()>
    where
        F: FnOnce(AgentSnapshot) -> AgentRuntimeEventKind,
    {
        let expected_revision = self.state.snapshot.revision;
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.event_sequence = self.state.snapshot.event_sequence.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        let event = AgentRuntimeEvent {
            agent_id: next.snapshot.identity.id.clone(),
            sequence: next.snapshot.event_sequence,
            created_at: next.snapshot.updated_at,
            kind: event_kind(next.snapshot.clone()),
        };
        let session_id = runtime_event_session_id(&event).map(str::to_string);
        let current_session_event_sequence = match session_id.as_deref() {
            Some(session_id) => {
                self.runtime
                    .session_events
                    .snapshot(session_id)
                    .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?
                    .through_sequence
            }
            None => 0,
        };
        let projected = project_runtime_event(&event, current_session_event_sequence);
        let durable_session_events = projected.durable_events();
        let session_projection = match session_id.as_deref() {
            Some(session_id) if !durable_session_events.is_empty() => {
                Some(SessionProjectionCommit {
                    snapshot: self
                        .runtime
                        .session_events
                        .project_durable(session_id, &durable_session_events)
                        .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?,
                    durable_events: durable_session_events,
                })
            }
            Some(_) | None => None,
        };
        if let Some(session_id) = session_id.as_ref() {
            let session_id = SessionId::new(session_id.clone())
                .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
            if next.session.id != session_id {
                return Err(AgentRuntimeError::SessionMismatch {
                    agent_id: next.snapshot.identity.id.clone(),
                    expected: next.session.id.clone(),
                    actual: session_id,
                });
            }
            next.session.session_event_sequence = projected.through_sequence;
        }
        let committed_trace_events = trace_events.clone();
        let committed_session_events = projected.events.clone();
        let result = self
            .host
            .repository()
            .commit(AgentCommit {
                agent_id: next.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: next.clone(),
                events: vec![event.clone()],
                trace_events,
                session_projection,
                mutation: super::AgentStateMutation::SnapshotAndQueue,
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match result {
            AgentCommitOutcome::Applied => {
                self.state = next;
                self.runtime.directory.publish_runtime_event(&event);
                self.runtime
                    .session_events
                    .publish_batch(committed_session_events.clone())
                    .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?;
                self.host
                    .observer()
                    .publish(AgentCommittedEvent {
                        agent_id: event.agent_id.clone(),
                        session_id: session_id.and_then(|value| SessionId::new(value).ok()),
                        turn_id: projected
                            .events
                            .first()
                            .and_then(|event| event.turn_id.clone())
                            .and_then(|value| TurnId::new(value).ok()),
                        runtime_events: vec![event],
                        trace_events: committed_trace_events,
                        session_events: committed_session_events,
                    })
                    .await;
                Ok(())
            }
            AgentCommitOutcome::RevisionConflict { actual_revision } => {
                Err(AgentRuntimeError::RevisionConflict {
                    expected: Some(expected_revision),
                    actual: actual_revision,
                })
            }
        }
    }

    fn stop_active_turn(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancellation.cancel();
            active.abort_handle.abort();
        }
    }

    fn fault_in_memory(&mut self, reason: String) {
        let turn_id = self
            .active
            .as_ref()
            .map(|active| active.turn_id.clone())
            .or_else(|| self.state.snapshot.active_turn_id.clone());
        let session_id = self
            .active
            .as_ref()
            .map(|active| active.session_id.clone())
            .or_else(|| turn_id.as_ref().map(|_| self.state.session.id.clone()));
        let fault_outcome = turn_id
            .clone()
            .zip(session_id.clone())
            .map(|(turn_id, session_id)| {
                turn_outcome(turn_id, session_id, Err(reason.clone()), false).0
            });
        tracing::error!(
            agent_id = %self.state.snapshot.identity.id,
            turn_id = turn_id.as_ref().map(TurnId::as_str),
            session_id = session_id.as_ref().map(SessionId::as_str),
            reason,
            "agent runtime entered an in-memory faulted state"
        );
        self.stop_active_turn();
        self.state.snapshot.lifecycle = AgentLifecycleState::Faulted;
        self.state.snapshot.activity = AgentActivityState::Idle;
        self.state.snapshot.active_turn_id = None;
        self.state.active_input = None;
        if fault_outcome.is_some() {
            self.state.snapshot.last_turn = fault_outcome;
        }
        self.runtime
            .directory
            .store_snapshot(self.state.snapshot.clone());
    }
}
