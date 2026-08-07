use std::time::Duration;

use pl_protocol::{ThreadNotification, ThreadNotificationEnvelope};
use pl_trace::TraceEvent;
use tokio::sync::{mpsc, oneshot};

use super::host::{
    AgentCommitObserver, ThreadProjectionCommit, ThreadRepository, transcript_mutation,
};
use super::state::{AgentRuntimeError, unix_timestamp};
use super::{
    AgentActivityState, AgentCommittedEvent, AgentCurrentSessionSubmitRequest, AgentLifecycleState,
    AgentProgressCheckpoint, AgentProgressStage, AgentRuntimeEvent, AgentRuntimeEventKind,
    AgentRuntimeHandle, AgentRuntimeHost, AgentRuntimeResult, AgentSessionDigest, AgentSnapshot,
    AgentSubmitRequest, DurableCommitFacts, ThreadActorState, ThreadCommit, ThreadCommitOutcome,
    ThreadId, TurnId,
};
use crate::thread_event::{
    ObservedTurnEvent, TurnObservation, project_observation, project_runtime_event,
    project_trace_events, runtime_event_thread_id,
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
    ReconfigureIdleRole {
        role: crate::AgentRoleId,
        reply: oneshot::Sender<AgentRuntimeResult<AgentSnapshot>>,
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
        checkpoint: Box<super::AgentTurnCheckpoint>,
        reply: oneshot::Sender<AgentRuntimeResult<()>>,
    },
    RecordThreadFacts {
        thread_id: ThreadId,
        facts: Vec<crate::ThreadNotificationFact>,
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
    state: ThreadActorState,
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
    state: ThreadActorState,
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
                        AgentLoopCommand::ReconfigureIdleRole { role, reply } => {
                            let result = self.reconfigure_idle_role(role).await;
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
                            let result = self.checkpoint(*checkpoint).await;
                            if let Err(error) = &result {
                                self.fault_in_memory(error.to_string());
                            }
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::RecordThreadFacts {
                            thread_id,
                            facts,
                            reply,
                        } => {
                            let result = self.record_thread_facts(thread_id, facts).await;
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
            thread_id: active.thread_id.to_string(),
            observation,
        })
        .await
    }

    async fn persist_observation(&mut self, observed: ObservedTurnEvent) -> AgentRuntimeResult<()> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        if active.turn_id.as_str() != observed.turn_id
            || active.thread_id.as_str() != observed.thread_id
            || active.cancelling
        {
            return Ok(());
        }
        let thread_id = active.thread_id.clone();
        let turn_id = active.turn_id.clone();
        let expected_revision = self.state.snapshot.revision;
        let current = self
            .runtime
            .thread_events
            .snapshot(thread_id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
        let projected = project_observation(
            thread_id.as_str(),
            turn_id.as_str(),
            current.revision,
            &current,
            observed.observation,
        );
        let projection = ThreadProjectionCommit {
            snapshot: self
                .runtime
                .thread_events
                .project(thread_id.as_str(), &projected.notifications)
                .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?,
            notifications: projected.notifications.clone(),
        };
        let mut next = self.state.clone();
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        next.session.thread_revision = projected.through_revision;
        let outcome = self
            .host
            .repository()
            .commit(ThreadCommit {
                agent_id: next.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: next.clone(),
                facts: DurableCommitFacts::from_state(
                    &next,
                    Vec::new(),
                    Vec::new(),
                    Some(projection),
                    None,
                ),
                mutation: super::ThreadMutation::AppendThreadNotifications {
                    thread_id: thread_id.clone(),
                },
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match outcome {
            ThreadCommitOutcome::Applied => {
                self.state = next;
                self.runtime
                    .directory
                    .store_snapshot(self.state.snapshot.clone());
                self.runtime
                    .thread_events
                    .publish_batch(projected.notifications.clone())
                    .await
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
                self.host
                    .observer()
                    .publish(AgentCommittedEvent {
                        agent_id: self.state.snapshot.identity.id.clone(),
                        thread_id: Some(thread_id),
                        turn_id: Some(turn_id),
                        runtime_events: Vec::new(),
                        trace_events: Vec::new(),
                        thread_notifications: projected.notifications,
                    })
                    .await;
                Ok(())
            }
            ThreadCommitOutcome::RevisionConflict { actual_revision } => {
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
        let thread_id = active.thread_id.clone();
        let turn_id = active.turn_id.clone();
        if trace_events
            .iter()
            .any(|trace| trace.session_id != thread_id.as_str())
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
        let thread_revision = self
            .runtime
            .thread_events
            .snapshot(thread_id.as_str())
            .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?
            .revision;
        let projected = project_trace_events(thread_id.as_str(), thread_revision, &trace_events);
        let session_projection = if projected.notifications.is_empty() {
            None
        } else {
            Some(ThreadProjectionCommit {
                snapshot: self
                    .runtime
                    .thread_events
                    .project(thread_id.as_str(), &projected.notifications)
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?,
                notifications: projected.notifications.clone(),
            })
        };
        let mut next = self.state.clone();
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        next.session.trace_sequence = next_trace_sequence;
        next.session.thread_revision = projected.through_revision;
        let committed_trace_events = trace_events.clone();
        let committed_thread_events = projected.notifications.clone();
        let result = self
            .host
            .repository()
            .commit(ThreadCommit {
                agent_id: next.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: next.clone(),
                facts: DurableCommitFacts::from_state(
                    &next,
                    Vec::new(),
                    trace_events,
                    session_projection,
                    None,
                ),
                mutation: super::ThreadMutation::AppendTrace,
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match result {
            ThreadCommitOutcome::Applied => {
                let agent_id = next.snapshot.identity.id.clone();
                self.state = next;
                self.runtime
                    .directory
                    .store_snapshot(self.state.snapshot.clone());
                self.runtime
                    .thread_events
                    .publish_batch(committed_thread_events.clone())
                    .await
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
                self.host
                    .observer()
                    .publish(AgentCommittedEvent {
                        agent_id,
                        thread_id: Some(thread_id),
                        turn_id: Some(turn_id),
                        runtime_events: Vec::new(),
                        trace_events: committed_trace_events,
                        thread_notifications: committed_thread_events,
                    })
                    .await;
                Ok(())
            }
            ThreadCommitOutcome::RevisionConflict { actual_revision } => {
                Err(AgentRuntimeError::RevisionConflict {
                    expected: Some(expected_revision),
                    actual: actual_revision,
                })
            }
        }
    }

    async fn commit_transition<F>(
        &mut self,
        mut next: ThreadActorState,
        trace_events: Vec<TraceEvent>,
        event_kind: F,
    ) -> AgentRuntimeResult<()>
    where
        F: FnOnce(AgentSnapshot) -> AgentRuntimeEventKind,
    {
        let expected_revision = self.state.snapshot.revision;
        let context = transcript_mutation(
            self.state.session.session.items(),
            next.session.session.items(),
        );
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.event_sequence = self.state.snapshot.event_sequence.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        let event = AgentRuntimeEvent {
            agent_id: next.snapshot.identity.id.clone(),
            sequence: next.snapshot.event_sequence,
            created_at: next.snapshot.updated_at,
            kind: event_kind(next.snapshot.clone()),
        };
        let thread_id = runtime_event_thread_id(&event).map(str::to_string);
        let current_thread_revision = match thread_id.as_deref() {
            Some(thread_id) => {
                self.runtime
                    .thread_events
                    .snapshot(thread_id)
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?
                    .revision
            }
            None => 0,
        };
        let projected = project_runtime_event(&event, current_thread_revision);
        let thread_projection = match thread_id.as_deref() {
            Some(thread_id) if !projected.notifications.is_empty() => {
                Some(ThreadProjectionCommit {
                    snapshot: self
                        .runtime
                        .thread_events
                        .project(thread_id, &projected.notifications)
                        .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?,
                    notifications: projected.notifications.clone(),
                })
            }
            Some(_) | None => None,
        };
        if let Some(thread_id) = thread_id.as_ref() {
            let thread_id = ThreadId::new(thread_id.clone())
                .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
            if next.snapshot.identity.id != thread_id {
                return Err(AgentRuntimeError::ThreadMismatch {
                    agent_id: next.snapshot.identity.id.clone(),
                    expected: next.snapshot.identity.id.clone(),
                    actual: thread_id,
                });
            }
            next.session.thread_revision = projected.through_revision;
        }
        let committed_trace_events = trace_events.clone();
        let committed_thread_events = projected.notifications.clone();
        let result = self
            .host
            .repository()
            .commit(ThreadCommit {
                agent_id: next.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: next.clone(),
                facts: DurableCommitFacts::from_state(
                    &next,
                    vec![event.clone()],
                    trace_events,
                    thread_projection,
                    context,
                ),
                mutation: super::ThreadMutation::SnapshotAndQueue,
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match result {
            ThreadCommitOutcome::Applied => {
                self.state = next;
                self.runtime.directory.publish_runtime_event(&event);
                self.runtime
                    .thread_events
                    .publish_batch(committed_thread_events.clone())
                    .await
                    .map_err(|error| AgentRuntimeError::ThreadEvents(error.to_string()))?;
                self.host
                    .observer()
                    .publish(AgentCommittedEvent {
                        agent_id: event.agent_id.clone(),
                        thread_id: thread_id.and_then(|value| ThreadId::new(value).ok()),
                        turn_id: projected
                            .notifications
                            .first()
                            .and_then(notification_turn_id)
                            .and_then(|value| TurnId::new(value.to_string()).ok()),
                        runtime_events: vec![event],
                        trace_events: committed_trace_events,
                        thread_notifications: committed_thread_events,
                    })
                    .await;
                Ok(())
            }
            ThreadCommitOutcome::RevisionConflict { actual_revision } => {
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
        let thread_id = self
            .active
            .as_ref()
            .map(|active| active.thread_id.clone())
            .or_else(|| {
                turn_id
                    .as_ref()
                    .map(|_| self.state.snapshot.identity.id.clone())
            });
        let fault_outcome = turn_id
            .clone()
            .zip(thread_id.clone())
            .map(|(turn_id, thread_id)| {
                turn_outcome(turn_id, thread_id, Err(reason.clone()), false).0
            });
        tracing::error!(
            agent_id = %self.state.snapshot.identity.id,
            turn_id = turn_id.as_ref().map(TurnId::as_str),
            thread_id = thread_id.as_ref().map(ThreadId::as_str),
            reason_bytes = reason.len(),
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

fn notification_turn_id(notification: &ThreadNotificationEnvelope) -> Option<&str> {
    match &notification.notification {
        ThreadNotification::TurnStarted { turn }
        | ThreadNotification::TurnUpdated { turn }
        | ThreadNotification::TurnCompleted { turn } => Some(turn.id.as_str()),
        ThreadNotification::ItemStarted { item } | ThreadNotification::ItemCompleted { item } => {
            Some(item.turn_id.as_str())
        }
        ThreadNotification::InteractionChanged { interaction } => {
            Some(interaction.scope.turn_id.as_str())
        }
        ThreadNotification::ItemDelta { .. }
        | ThreadNotification::ThreadRuntimeUpdated { .. }
        | ThreadNotification::Lagged { .. } => None,
    }
}
