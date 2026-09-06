use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

use crate::agent_runtime::state::AgentRuntimeError;
use crate::agent_runtime::*;

use super::super::{AgentLoop, AgentLoopCommand};
use super::{
    TurnCompletion, TurnExecutionTerminal, TurnSessionDisposition, TurnWorkerOutcome, execute_turn,
    turn_outcome,
};

pub(in crate::agent_runtime::agent_loop) enum RunningTurnCancellation {
    Open,
    Requested {
        cause: pl_protocol::TurnCancellationCause,
    },
}

pub(in crate::agent_runtime::agent_loop) struct RunningTurn {
    pub(in crate::agent_runtime::agent_loop) turn_id: TurnId,
    pub(in crate::agent_runtime::agent_loop) thread_id: ThreadId,
    pub(in crate::agent_runtime::agent_loop) identity: std::sync::Arc<()>,
    pub(in crate::agent_runtime::agent_loop) start_revision: u64,
    pub(in crate::agent_runtime::agent_loop) started_at: i64,
    pub(in crate::agent_runtime::agent_loop) cancellation: CancellationToken,
    pub(in crate::agent_runtime::agent_loop) abort_handle: AbortHandle,
    pub(in crate::agent_runtime::agent_loop) settled: oneshot::Receiver<()>,
    pub(in crate::agent_runtime::agent_loop) cancellation_state: RunningTurnCancellation,
    pub(in crate::agent_runtime::agent_loop) activity: AgentActivityUpdate,
    pub(in crate::agent_runtime::agent_loop) checkpoint_sequence: u64,
    pub(in crate::agent_runtime::agent_loop) steer_sender:
        mpsc::UnboundedSender<super::super::DurableMailboxEnvelope>,
    pub(in crate::agent_runtime::agent_loop) budget_refresh: super::super::TurnBudgetRefreshHandle,
    pub(in crate::agent_runtime::agent_loop) projection_failure: Option<String>,
}

impl RunningTurn {
    pub(in crate::agent_runtime::agent_loop) const fn is_cancelling(&self) -> bool {
        matches!(
            self.cancellation_state,
            RunningTurnCancellation::Requested { .. }
        )
    }

    pub(in crate::agent_runtime::agent_loop) fn terminal(
        &self,
        outcome: TurnWorkerOutcome,
    ) -> TurnExecutionTerminal {
        if let Some(error) = &self.projection_failure {
            return TurnExecutionTerminal::ProtocolFailed {
                error: error.clone(),
            };
        }
        match (&self.cancellation_state, outcome) {
            (RunningTurnCancellation::Open, outcome) => outcome.into(),
            (RunningTurnCancellation::Requested { cause }, TurnWorkerOutcome::Returned(result)) => {
                TurnExecutionTerminal::CancelledAfterReturn {
                    cause: cause.clone(),
                    result: *result,
                }
            }
            (RunningTurnCancellation::Requested { cause }, TurnWorkerOutcome::Failed { .. }) => {
                TurnExecutionTerminal::CancelledBeforeReturn {
                    cause: cause.clone(),
                }
            }
        }
    }

    fn request_cancellation(&mut self, cause: pl_protocol::TurnCancellationCause) {
        self.cancellation_state = RunningTurnCancellation::Requested { cause };
    }
}

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(in crate::agent_runtime::agent_loop) async fn begin_next_turn(&mut self) {
        if self.active.is_some()
            || !self.dispatch_enabled
            || !self.state.snapshot.state.is_queued()
            || !self.state.has_triggering_input()
        {
            return;
        }
        if self
            .state
            .pending_inputs
            .front()
            .is_some_and(|input| input.thread_id != self.state.snapshot.identity.id)
        {
            self.fault(
                AgentRuntimeError::ThreadMismatch {
                    agent_id: self.state.snapshot.identity.id.clone(),
                    expected: self.state.snapshot.identity.id.clone(),
                    actual: self.state.pending_inputs[0].thread_id.clone(),
                }
                .to_string(),
            )
            .await;
            return;
        }
        let mut next = self.state.clone();
        let Some(mut input) = next.pending_inputs.pop_front() else {
            return;
        };
        let mut leading_inputs = Vec::new();
        if let Some(key) = input.queue_coalescing_key.clone() {
            while next.pending_inputs.front().is_some_and(|candidate| {
                candidate.delivery_state.is_pending()
                    && candidate.queue_coalescing_key.as_deref() == Some(key.as_str())
            }) {
                leading_inputs.push(input);
                input = next
                    .pending_inputs
                    .pop_front()
                    .expect("matched queued input must remain available");
            }
        }
        for leading in &mut leading_inputs {
            if let Err(error) = leading.claim(input.turn_id.clone()) {
                self.fault(error.to_string()).await;
                return;
            }
        }
        for leading in leading_inputs.iter().rev() {
            next.pending_inputs.push_front(leading.clone());
        }
        if let Err(error) = input.claim(input.turn_id.clone()) {
            self.fault(error.to_string()).await;
            return;
        }
        next.active_input = Some(input.clone());
        next.refresh_mailbox_snapshot();
        if let Err(error) = next.snapshot.transition(AgentCommand::Start {
            turn_id: input.turn_id.clone(),
        }) {
            self.fault(error.to_string()).await;
            return;
        }
        let committed = self
            .commit_transition(
                super::super::persist::TransitionCommit::new(next),
                |snapshot| AgentRuntimeEventKind::TurnStarted {
                    turn_id: input.turn_id.clone(),
                    thread_id: input.thread_id.clone(),
                    input: input.clone(),
                    claimed_inputs: leading_inputs.clone(),
                    snapshot: Box::new(snapshot),
                },
            )
            .await;
        if let Err(error) = committed {
            // The durable state still owns the queued input, but the in-memory
            // fault should identify the turn whose start transition failed.
            self.fault(error.to_string()).await;
            return;
        }

        // A new turn never shares a producer channel with its predecessor.
        let (trace_sender, trace_receiver) = mpsc::unbounded_channel();
        let (observation_sender, observation_receiver) = mpsc::unbounded_channel();
        self.channels.trace_sender = trace_sender;
        self.channels.trace_receiver = trace_receiver;
        self.channels.observation_sender = observation_sender;
        self.channels.observation_receiver = observation_receiver;
        let cancellation = CancellationToken::new();
        let (steer_sender, steer_receiver) = mpsc::unbounded_channel();
        let mailbox = AgentTurnMailboxHandle::new(
            steer_receiver,
            leading_inputs
                .iter()
                .map(|input| input.mail_id.clone())
                .collect(),
        );
        let (budget_refresh, budget_refresh_receiver) = super::super::turn_budget_refresh_channel();
        let thread_id = self.state.snapshot.identity.id.clone();
        let context = AgentTurnPreparationContext {
            snapshot: self.state.snapshot.clone(),
            turn_id: input.turn_id.clone(),
            thread_id: input.thread_id.clone(),
            input,
            leading_inputs,
            session: self.state.session.session.clone(),
            trace_sequence: self.state.session.trace_sequence,
            runtime: self.runtime.clone(),
            cancellation_token: cancellation.clone(),
            mailbox,
            budget_refresh: budget_refresh_receiver,
        };
        let start_revision = self.state.snapshot.revision;
        let identity = Arc::new(());
        let initial_trace_sequence = context.trace_sequence;
        let worker_host = self.host.clone();
        let worker_cancellation = cancellation.clone();
        let worker_identity = identity.clone();
        let durable_trace_tx = self.channels.trace_sender.clone();
        let observation_tx = self.channels.observation_sender.clone();
        let worker = tokio::spawn(async move {
            execute_turn(
                worker_host,
                context,
                worker_identity,
                worker_cancellation,
                durable_trace_tx,
                observation_tx,
            )
            .await
        });
        let abort_handle = worker.abort_handle();
        let (settled_sender, settled) = oneshot::channel();
        let completion_sender = self.channels.command_sender.clone();
        let completion_turn_id = self
            .state
            .snapshot
            .active_turn_id()
            .cloned()
            .expect("started turn must have an id");
        let completion_identity = identity.clone();
        tokio::spawn(async move {
            let completion = match worker.await {
                Ok(completion) => completion,
                Err(error) => TurnCompletion {
                    turn_id: completion_turn_id,
                    identity: completion_identity,
                    start_revision,
                    session: TurnSessionDisposition::Preserve,
                    worker_outcome: TurnWorkerOutcome::Failed {
                        error: format!("turn task join failed: {error}"),
                    },
                    next_trace_sequence: initial_trace_sequence,
                },
            };
            let _ = settled_sender.send(());
            let _ = completion_sender
                .send(AgentLoopCommand::TurnFinished(Box::new(completion)))
                .await;
        });
        self.active = Some(RunningTurn {
            turn_id: self
                .state
                .snapshot
                .active_turn_id()
                .cloned()
                .expect("started turn must have an id"),
            thread_id,
            identity,
            start_revision,
            started_at: self.state.snapshot.updated_at,
            cancellation,
            abort_handle,
            settled,
            cancellation_state: RunningTurnCancellation::Open,
            activity: AgentActivityUpdate::Running,
            checkpoint_sequence: 0,
            steer_sender,
            budget_refresh,
            projection_failure: None,
        });
    }

    pub(in crate::agent_runtime::agent_loop) async fn interrupt_active_turn(
        &mut self,
        cause: pl_protocol::TurnCancellationCause,
    ) -> AgentRuntimeResult<()> {
        let Some(active) = &mut self.active else {
            return Err(AgentRuntimeError::NoActiveTurn(
                self.state.snapshot.identity.id.clone(),
            ));
        };
        if active.is_cancelling() {
            return Ok(());
        }
        active.request_cancellation(cause.clone());
        active.cancellation.cancel();
        let mut next = self.state.clone();
        next.snapshot
            .transition(AgentCommand::Cancel {
                turn_id: active.turn_id.clone(),
            })
            .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?;
        self.commit_transition(
            super::super::persist::TransitionCommit::new(next),
            |snapshot| AgentRuntimeEventKind::StateChanged {
                snapshot: Box::new(snapshot),
            },
        )
        .await?;

        let grace = self.cancel_grace.min(Duration::from_secs(1));
        let deadline = tokio::time::sleep(grace);
        tokio::pin!(deadline);
        loop {
            let active = self
                .active
                .as_mut()
                .expect("running turn remains while cancelling");
            tokio::select! {
                _ = &mut active.settled => break,
                _ = &mut deadline => {
                    active.abort_handle.abort();
                    let _ = (&mut active.settled).await;
                    break;
                }
                command = self.channels.command_receiver.recv() => match command {
                    Some(AgentLoopCommand::Checkpoint { checkpoint, reply }) => {
                        // The cancelled worker awaits durable accounting before it can settle.
                        let result = self.checkpoint(*checkpoint).await;
                        let _ = reply.send(result);
                    }
                    Some(AgentLoopCommand::SetActivity { reply, .. }) => {
                        let _ = reply.send(Ok(()));
                    }
                    Some(command) => self.channels.deferred_commands.push_back(command),
                    None => break,
                }
            }
        }
        if let Err(error) = self.flush_pending_traces().await {
            self.mark_projection_failure(&error);
        }
        if let Err(error) = self.flush_pending_observations().await {
            self.mark_projection_failure(&error);
        }
        let active = self
            .active
            .take()
            .expect("running turn must remain until cancellation is committed");
        let mut outcome = turn_outcome(
            active.turn_id.clone(),
            active.thread_id,
            match active.projection_failure {
                Some(error) => TurnExecutionTerminal::ProtocolFailed { error },
                None => TurnExecutionTerminal::CancelledBeforeReturn { cause },
            },
            Some(active.started_at),
        )
        .outcome;
        if let Some(billing) = self
            .state
            .session
            .billing_by_turn
            .get(outcome.turn_id.as_str())
        {
            outcome.usage = billing.aggregate_usage();
        }
        let mut next = self.state.clone();
        for input in &mut next.pending_inputs {
            if matches!(
                &input.delivery_state,
                MailboxDeliveryState::Claimed(state) if state.turn_id() == &active.turn_id
            ) && let Err(error) = input.requeue(TurnId::generate())
            {
                return Err(AgentRuntimeError::InvalidInput(error.to_string()));
            }
        }
        next.active_input = None;
        next.refresh_mailbox_snapshot();
        let next_turn_id = next.triggering_turn_id();
        next.snapshot
            .transition(AgentCommand::Settle { next_turn_id })
            .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()))?;
        next.snapshot.last_turn = Some(outcome.clone());
        self.commit_transition(
            super::super::persist::TransitionCommit::new(next),
            |snapshot| AgentRuntimeEventKind::TurnFinished {
                outcome,
                snapshot: Box::new(snapshot),
            },
        )
        .await?;
        if self.dispatch_enabled && self.state.has_triggering_input() {
            self.begin_next_turn().await;
        }
        Ok(())
    }
}
