use std::collections::BTreeSet;
use std::time::Duration;

use pl_trace::TraceEvent;
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

use super::execution::{TurnCompletion, execute_turn, turn_outcome};
use super::host::{AgentCommitObserver, AgentStateRepository, SessionProjectionCommit};
use super::state::{AgentRuntimeError, ResolvedAgentSessionTarget, unix_timestamp};
use super::{
    AgentActivityState, AgentCommit, AgentCommitOutcome, AgentCommittedEvent,
    AgentCurrentSessionSubmitRequest, AgentDurableState, AgentLifecycleState, AgentRuntimeEvent,
    AgentRuntimeEventKind, AgentRuntimeHandle, AgentRuntimeHost, AgentRuntimeResult, AgentSnapshot,
    AgentSubmitRequest, AgentTurnPreparationContext, AgentWaitResult, InputDelivery,
    PendingAgentInput, SessionId, TurnId,
};
use crate::session_event::{
    ObservedTurnEvent, TurnObservation, project_observation, project_runtime_event,
    project_trace_events, runtime_event_session_id,
};

mod completion;
mod lifecycle;

pub(crate) enum ActorCommand {
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
    OpenSession {
        session: super::AgentSessionState,
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
    Wait {
        reply: oneshot::Sender<AgentRuntimeResult<AgentWaitResult>>,
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
pub(crate) struct AgentActorHandle {
    sender: mpsc::Sender<ActorCommand>,
    #[cfg(test)]
    trace_sender: mpsc::UnboundedSender<TraceEvent>,
}

impl AgentActorHandle {
    pub(crate) async fn send(&self, command: ActorCommand) -> AgentRuntimeResult<()> {
        self.sender
            .send(command)
            .await
            .map_err(|_| AgentRuntimeError::ChannelClosed)
    }

    #[cfg(test)]
    pub(crate) fn record_trace(&self, event: TraceEvent) -> AgentRuntimeResult<()> {
        self.trace_sender
            .send(event)
            .map_err(|_| AgentRuntimeError::ChannelClosed)
    }
}

struct ActiveTurn {
    turn_id: TurnId,
    session_id: SessionId,
    start_revision: u64,
    cancellation: CancellationToken,
    abort_handle: AbortHandle,
    settled: oneshot::Receiver<()>,
    cancellation_requested: bool,
    checkpoint_sequence: u64,
}

struct AgentActor<H>
where
    H: AgentRuntimeHost,
{
    host: H,
    state: AgentDurableState,
    runtime: AgentRuntimeHandle,
    sender: mpsc::Sender<ActorCommand>,
    receiver: mpsc::Receiver<ActorCommand>,
    trace_sender: mpsc::UnboundedSender<TraceEvent>,
    trace_receiver: mpsc::UnboundedReceiver<TraceEvent>,
    observation_sender: mpsc::UnboundedSender<ObservedTurnEvent>,
    observation_receiver: mpsc::UnboundedReceiver<ObservedTurnEvent>,
    active: Option<ActiveTurn>,
    run_queue: bool,
    waiters: Vec<oneshot::Sender<AgentRuntimeResult<AgentWaitResult>>>,
    cancel_grace: Duration,
}

pub(crate) fn spawn_agent_actor<H>(
    host: H,
    state: AgentDurableState,
    runtime: AgentRuntimeHandle,
    cancel_grace: Duration,
    start_pending_inputs: bool,
    command_capacity: usize,
) -> AgentActorHandle
where
    H: AgentRuntimeHost,
{
    let (sender, receiver) = mpsc::channel(command_capacity.max(1));
    let (trace_sender, trace_receiver) = mpsc::unbounded_channel();
    let (observation_sender, observation_receiver) = mpsc::unbounded_channel();
    let handle = AgentActorHandle {
        sender: sender.clone(),
        #[cfg(test)]
        trace_sender: trace_sender.clone(),
    };
    let run_queue = start_pending_inputs && !state.pending_inputs.is_empty();
    tokio::spawn(
        AgentActor {
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
            run_queue,
            waiters: Vec::new(),
            cancel_grace,
        }
        .run(),
    );
    handle
}

impl<H> AgentActor<H>
where
    H: AgentRuntimeHost,
{
    async fn run(mut self) {
        if self.run_queue && self.state.snapshot.lifecycle == AgentLifecycleState::Active {
            self.begin_next_turn().await;
        }
        loop {
            tokio::select! {
                command = self.receiver.recv() => {
                    let Some(command) = command else {
                        break;
                    };
                    match command {
                        ActorCommand::Submit { request, reply } => {
                            let result = self.submit(request).await;
                            let _ = reply.send(result);
                        }
                        ActorCommand::SubmitCurrentSession {
                            root_agent_id,
                            request,
                            reply,
                        } => {
                            let result =
                                self.submit_current_session(root_agent_id, request).await;
                            let _ = reply.send(result);
                        }
                        ActorCommand::CancelTurn { turn_id, reply } => {
                            let result = self.cancel_turn(turn_id);
                            let _ = reply.send(result);
                        }
                        ActorCommand::SetActivity {
                            turn_id,
                            activity,
                            reply,
                        } => {
                            let result = self.set_activity(turn_id, activity).await;
                            let _ = reply.send(result);
                        }
                        ActorCommand::Checkpoint { checkpoint, reply } => {
                            let result = self.checkpoint(checkpoint).await;
                            if let Err(error) = &result {
                                self.fault_in_memory(error.to_string());
                            }
                            let _ = reply.send(result);
                        }
                        ActorCommand::OpenSession { session, reply } => {
                            let result = self.open_session(session).await;
                            let _ = reply.send(result);
                        }
                        ActorCommand::RecordSessionFacts {
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
                        ActorCommand::Snapshot { reply } => {
                            let _ = reply.send(Ok(self.state.snapshot.clone()));
                        }
                        ActorCommand::Wait { reply } => self.wait(reply),
                        ActorCommand::StartPendingInputs { reply } => {
                            self.run_queue = true;
                            self.begin_next_turn().await;
                            let _ = reply.send(Ok(()));
                        }
                        ActorCommand::Close { reply } => {
                            let result = self.close().await;
                            let _ = reply.send(result);
                        }
                        ActorCommand::TurnFinished(completion) => {
                            self.finish_turn(*completion).await;
                        }
                        ActorCommand::Shutdown { reply } => {
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
        for waiter in self.waiters.drain(..) {
            let _ = waiter.send(Err(AgentRuntimeError::ChannelClosed));
        }
    }

    async fn submit(&mut self, request: AgentSubmitRequest) -> AgentRuntimeResult<TurnId> {
        if self.state.snapshot.lifecycle != AgentLifecycleState::Active {
            return Err(AgentRuntimeError::NotActive(
                self.state.snapshot.identity.id.clone(),
                self.state.snapshot.lifecycle,
            ));
        }
        if !self.state.sessions.contains_key(&request.session_id) {
            return Err(AgentRuntimeError::SessionNotOwned {
                agent_id: self.state.snapshot.identity.id.clone(),
                session_id: request.session_id,
            });
        }
        let input = PendingAgentInput {
            turn_id: TurnId::generate(),
            session_id: request.session_id,
            message: request.message,
            metadata: request.metadata,
            queued_at: unix_timestamp(),
        };
        let mut next = self.state.clone();
        match request.delivery {
            InputDelivery::QueueOnly | InputDelivery::Start => {
                next.pending_inputs.push_back(input.clone());
            }
            InputDelivery::InterruptThenStart => {
                next.pending_inputs.push_front(input.clone());
                if let Some(active) = &mut self.active {
                    request_cancellation(active, self.cancel_grace);
                }
            }
        }
        next.snapshot.pending_inputs = next.pending_inputs.len();
        if self.active.is_none() {
            next.snapshot.activity = AgentActivityState::Queued;
        }
        self.commit_transition(next, Vec::new(), |snapshot| {
            AgentRuntimeEventKind::TurnQueued {
                input: input.clone(),
                snapshot,
            }
        })
        .await?;
        if request.delivery != InputDelivery::QueueOnly {
            self.run_queue = true;
        }
        let turn_id = input.turn_id.clone();
        if self.active.is_none() && self.run_queue {
            self.begin_next_turn().await;
        }
        Ok(turn_id)
    }

    async fn submit_current_session(
        &mut self,
        root_agent_id: super::AgentId,
        request: AgentCurrentSessionSubmitRequest,
    ) -> AgentRuntimeResult<TurnId> {
        let target = self.resolve_current_session_target(root_agent_id)?;
        debug_assert!(
            target.root_agent_id == target.agent_id || self.state.snapshot.identity.depth > 0
        );
        debug_assert_eq!(target.agent_id, self.state.snapshot.identity.id);
        debug_assert_eq!(target.owner_revision, self.state.snapshot.revision);
        self.submit(AgentSubmitRequest {
            session_id: target.session_id,
            message: request.message,
            metadata: request.metadata,
            delivery: request.delivery,
        })
        .await
    }

    fn resolve_current_session_target(
        &self,
        root_agent_id: super::AgentId,
    ) -> AgentRuntimeResult<ResolvedAgentSessionTarget> {
        let agent_id = self.state.snapshot.identity.id.clone();
        if self.state.snapshot.lifecycle != AgentLifecycleState::Active {
            return Err(AgentRuntimeError::NotActive(
                agent_id,
                self.state.snapshot.lifecycle,
            ));
        }

        let mut current = BTreeSet::new();
        if let Some(session_id) = &self.state.snapshot.active_session_id {
            current.insert(session_id.clone());
        }
        current.extend(
            self.state
                .pending_inputs
                .iter()
                .map(|input| input.session_id.clone()),
        );
        let session_id = match current.len() {
            1 => current.into_iter().next().ok_or_else(|| {
                AgentRuntimeError::CurrentSessionUnavailable {
                    agent_id: agent_id.clone(),
                    session_count: self.state.sessions.len(),
                }
            })?,
            0 if self.state.sessions.len() == 1 => self
                .state
                .sessions
                .first_key_value()
                .map(|(session_id, _)| session_id.clone())
                .ok_or_else(|| AgentRuntimeError::CurrentSessionUnavailable {
                    agent_id: agent_id.clone(),
                    session_count: self.state.sessions.len(),
                })?,
            _ => {
                return Err(AgentRuntimeError::CurrentSessionUnavailable {
                    agent_id,
                    session_count: self.state.sessions.len(),
                });
            }
        };
        if !self.state.sessions.contains_key(&session_id) {
            return Err(AgentRuntimeError::SessionNotOwned {
                agent_id,
                session_id,
            });
        }
        Ok(ResolvedAgentSessionTarget {
            root_agent_id,
            agent_id,
            session_id,
            owner_revision: self.state.snapshot.revision,
        })
    }

    fn cancel_turn(&mut self, turn_id: TurnId) -> AgentRuntimeResult<()> {
        let Some(active) = &mut self.active else {
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
        request_cancellation(active, self.cancel_grace);
        Ok(())
    }

    async fn set_activity(
        &mut self,
        turn_id: TurnId,
        activity: AgentActivityState,
    ) -> AgentRuntimeResult<()> {
        let Some(active) = &self.active else {
            return Ok(());
        };
        if active.turn_id != turn_id
            || self.state.snapshot.lifecycle != AgentLifecycleState::Active
            || self.state.snapshot.activity == activity
        {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.snapshot.activity = activity;
        self.commit_transition(next, Vec::new(), |snapshot| {
            AgentRuntimeEventKind::StateChanged { snapshot }
        })
        .await
    }

    async fn checkpoint(
        &mut self,
        checkpoint: super::AgentTurnCheckpoint,
    ) -> AgentRuntimeResult<()> {
        self.flush_pending_traces().await?;
        let Some(active) = &self.active else {
            return Ok(());
        };
        if active.turn_id != checkpoint.turn_id
            || active.session_id != checkpoint.session_id
            || active.cancellation_requested
            || active.cancellation.is_cancelled()
            || checkpoint.sequence <= active.checkpoint_sequence
        {
            return Ok(());
        }
        let expected_revision = self.state.snapshot.revision;
        let mut next = self.state.clone();
        next.snapshot.revision = expected_revision.saturating_add(1);
        next.snapshot.updated_at = unix_timestamp();
        let Some(session) = next.sessions.get_mut(&checkpoint.session_id) else {
            return Err(AgentRuntimeError::Repository(format!(
                "active session {} is missing for checkpoint",
                checkpoint.session_id
            )));
        };
        session.session = checkpoint.session;
        let result = self
            .host
            .repository()
            .commit(AgentCommit {
                agent_id: next.snapshot.identity.id.clone(),
                expected_revision: Some(expected_revision),
                next_state: next.clone(),
                events: Vec::new(),
                trace_events: Vec::new(),
                session_projection: None,
                mutation: super::AgentStateMutation::ReplaceSession {
                    session_id: checkpoint.session_id.clone(),
                },
            })
            .await
            .map_err(|error| AgentRuntimeError::Repository(error.to_string()))?;
        match result {
            AgentCommitOutcome::Applied => {
                tracing::debug!(
                    agent_id = %next.snapshot.identity.id,
                    turn_id = %checkpoint.turn_id,
                    sequence = checkpoint.sequence,
                    reason = ?checkpoint.reason,
                    "agent turn checkpoint committed"
                );
                self.state = next;
                if let Some(active) = &mut self.active {
                    active.checkpoint_sequence = checkpoint.sequence;
                }
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

    async fn open_session(&mut self, session: super::AgentSessionState) -> AgentRuntimeResult<()> {
        if self.state.snapshot.lifecycle != AgentLifecycleState::Active {
            return Err(AgentRuntimeError::NotActive(
                self.state.snapshot.identity.id.clone(),
                self.state.snapshot.lifecycle,
            ));
        }
        if self.state.sessions.contains_key(&session.id) {
            return Ok(());
        }
        let session_id = session.id.clone();
        let mut next = self.state.clone();
        next.sessions.insert(session_id.clone(), session);
        self.commit_transition(next, Vec::new(), |snapshot| {
            AgentRuntimeEventKind::SessionOpened {
                session_id,
                snapshot,
            }
        })
        .await
    }

    async fn record_session_facts(
        &mut self,
        session_id: SessionId,
        mut facts: Vec<crate::SessionEventFact>,
    ) -> AgentRuntimeResult<()> {
        if facts.is_empty() {
            return Ok(());
        }
        if !self.state.sessions.contains_key(&session_id) {
            return Err(AgentRuntimeError::SessionNotOwned {
                agent_id: self.state.snapshot.identity.id.clone(),
                session_id,
            });
        }
        let owner_agent_id = self.state.snapshot.identity.id.to_string();
        for fact in &mut facts {
            match fact.source_agent_id.as_deref() {
                Some(source_agent_id) if source_agent_id != owner_agent_id => {
                    return Err(AgentRuntimeError::Repository(format!(
                        "session {session_id} belongs to agent {owner_agent_id}, \
                         but fact source is {source_agent_id}"
                    )));
                }
                Some(_) => {}
                None => fact.source_agent_id = Some(owner_agent_id.clone()),
            }
        }
        let current = self
            .runtime
            .session_events
            .snapshot(session_id.as_str())
            .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?;
        let projected = crate::session_event::project_session_facts(
            session_id.as_str(),
            current.through_sequence,
            facts,
        );
        let durable_events = projected.durable_events();
        if durable_events.is_empty() {
            self.runtime
                .session_events
                .publish_batch(projected.events)
                .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?;
            return Ok(());
        }

        let expected_revision = self.state.snapshot.revision;
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
        next.sessions
            .get_mut(&session_id)
            .expect("validated session must be present")
            .session_event_sequence = projected.through_sequence;
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
                let agent_id = next.snapshot.identity.id.clone();
                self.state = next;
                self.runtime
                    .session_events
                    .publish_batch(projected.events.clone())
                    .map_err(|error| AgentRuntimeError::SessionEvents(error.to_string()))?;
                self.host
                    .observer()
                    .publish(AgentCommittedEvent {
                        agent_id,
                        session_id: Some(session_id),
                        turn_id: projected
                            .events
                            .first()
                            .and_then(|event| event.turn_id.clone())
                            .and_then(|value| TurnId::new(value).ok()),
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

    fn wait(&mut self, reply: oneshot::Sender<AgentRuntimeResult<AgentWaitResult>>) {
        if self.is_idle_and_drained() {
            let _ = reply.send(Ok(self.wait_result()));
        } else {
            self.waiters.push(reply);
        }
    }

    async fn begin_next_turn(&mut self) {
        if self.active.is_some()
            || !self.run_queue
            || self.state.snapshot.lifecycle != AgentLifecycleState::Active
        {
            return;
        }
        let Some(input) = self.state.pending_inputs.front().cloned() else {
            self.run_queue = false;
            return;
        };
        let mut next = self.state.clone();
        next.pending_inputs.pop_front();
        next.snapshot.pending_inputs = next.pending_inputs.len();
        next.snapshot.activity = AgentActivityState::Running;
        next.snapshot.active_turn_id = Some(input.turn_id.clone());
        next.snapshot.active_session_id = Some(input.session_id.clone());
        let committed = self
            .commit_transition(next, Vec::new(), |snapshot| {
                AgentRuntimeEventKind::TurnStarted {
                    turn_id: input.turn_id.clone(),
                    session_id: input.session_id.clone(),
                    snapshot,
                }
            })
            .await;
        if committed.is_err() {
            self.run_queue = false;
            return;
        }

        let cancellation = CancellationToken::new();
        let context = AgentTurnPreparationContext {
            snapshot: self.state.snapshot.clone(),
            turn_id: input.turn_id.clone(),
            session_id: input.session_id.clone(),
            input,
            session: self.state.sessions[&self.state.snapshot.active_session_id.clone().unwrap()]
                .session
                .clone(),
            trace_sequence: self.state.sessions
                [&self.state.snapshot.active_session_id.clone().unwrap()]
                .trace_sequence,
            runtime: self.runtime.clone(),
            cancellation_token: cancellation.clone(),
        };
        let start_revision = self.state.snapshot.revision;
        let initial_trace_sequence = context.trace_sequence;
        let worker_host = self.host.clone();
        let worker_cancellation = cancellation.clone();
        let durable_trace_tx = self.trace_sender.clone();
        let observation_tx = self.observation_sender.clone();
        let worker = tokio::spawn(async move {
            execute_turn(
                worker_host,
                context,
                worker_cancellation,
                durable_trace_tx,
                observation_tx,
            )
            .await
        });
        let abort_handle = worker.abort_handle();
        let (settled_sender, settled) = oneshot::channel();
        let completion_sender = self.sender.clone();
        let completion_turn_id = self.state.snapshot.active_turn_id.clone().unwrap();
        let completion_cancellation = cancellation.clone();
        tokio::spawn(async move {
            let completion = match worker.await {
                Ok(completion) => completion,
                Err(error) => TurnCompletion {
                    turn_id: completion_turn_id,
                    start_revision,
                    session: None,
                    result: Err(format!("turn task join failed: {error}")),
                    cancelled: completion_cancellation.is_cancelled() || error.is_cancelled(),
                    next_trace_sequence: initial_trace_sequence,
                },
            };
            let _ = settled_sender.send(());
            let _ = completion_sender
                .send(ActorCommand::TurnFinished(Box::new(completion)))
                .await;
        });
        self.active = Some(ActiveTurn {
            turn_id: self.state.snapshot.active_turn_id.clone().unwrap(),
            session_id: self.state.snapshot.active_session_id.clone().unwrap(),
            start_revision,
            cancellation,
            abort_handle,
            settled,
            cancellation_requested: false,
            checkpoint_sequence: 0,
        });
    }

    async fn abort_and_settle_active_turn(&mut self) {
        let Some(active) = &mut self.active else {
            return;
        };
        active.cancellation.cancel();
        active.abort_handle.abort();
        let _ = (&mut active.settled).await;
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
            || active.cancellation_requested
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
        next.sessions
            .get_mut(&session_id)
            .expect("active session must be present")
            .session_event_sequence = projected.through_sequence;
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
        let current_sequence = self.state.sessions[&session_id].trace_sequence;
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
        let next_session = next
            .sessions
            .get_mut(&session_id)
            .expect("active session must be present");
        next_session.trace_sequence = next_trace_sequence;
        next_session.session_event_sequence = projected.through_sequence;
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
            if let Some(session) = next.sessions.get_mut(&session_id) {
                session.session_event_sequence = projected.through_sequence;
            }
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
            .or_else(|| self.state.snapshot.active_session_id.clone());
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
        self.state.snapshot.active_session_id = None;
        if fault_outcome.is_some() {
            self.state.snapshot.last_turn = fault_outcome;
        }
        let result = self.wait_result();
        for waiter in self.waiters.drain(..) {
            let _ = waiter.send(Ok(result.clone()));
        }
    }

    fn is_idle_and_drained(&self) -> bool {
        self.state.snapshot.lifecycle != AgentLifecycleState::Active
            || (self.state.snapshot.activity == AgentActivityState::Idle
                && self.state.pending_inputs.is_empty())
    }

    fn wait_result(&self) -> AgentWaitResult {
        AgentWaitResult {
            snapshot: self.state.snapshot.clone(),
            last_turn: self.state.snapshot.last_turn.clone(),
        }
    }

    fn notify_waiters_if_ready(&mut self) {
        if !self.is_idle_and_drained() {
            return;
        }
        let result = self.wait_result();
        for waiter in self.waiters.drain(..) {
            let _ = waiter.send(Ok(result.clone()));
        }
    }
}

fn request_cancellation(active: &mut ActiveTurn, grace: Duration) {
    if active.cancellation_requested {
        return;
    }
    active.cancellation_requested = true;
    active.cancellation.cancel();
    let abort_handle = active.abort_handle.clone();
    tokio::spawn(async move {
        tokio::time::sleep(grace).await;
        abort_handle.abort();
    });
}
