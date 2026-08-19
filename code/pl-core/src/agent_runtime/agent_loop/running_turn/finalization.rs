use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

use crate::agent_runtime::state::AgentRuntimeError;
use crate::agent_runtime::*;

use super::super::{AgentLoop, AgentLoopCommand};
use super::{TurnCompletion, execute_turn, turn_outcome};

pub(in crate::agent_runtime::agent_loop) struct RunningTurn {
    pub(in crate::agent_runtime::agent_loop) turn_id: TurnId,
    pub(in crate::agent_runtime::agent_loop) thread_id: ThreadId,
    pub(in crate::agent_runtime::agent_loop) identity: std::sync::Arc<()>,
    pub(in crate::agent_runtime::agent_loop) start_revision: u64,
    pub(in crate::agent_runtime::agent_loop) cancellation: CancellationToken,
    pub(in crate::agent_runtime::agent_loop) abort_handle: AbortHandle,
    pub(in crate::agent_runtime::agent_loop) settled: oneshot::Receiver<()>,
    pub(in crate::agent_runtime::agent_loop) cancelling: bool,
    pub(in crate::agent_runtime::agent_loop) kind: ActiveKind,
    pub(in crate::agent_runtime::agent_loop) checkpoint_sequence: u64,
    pub(in crate::agent_runtime::agent_loop) steer_sender:
        mpsc::UnboundedSender<super::super::DurableMailboxEnvelope>,
    pub(in crate::agent_runtime::agent_loop) budget_refresh: super::super::TurnBudgetRefreshHandle,
}

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(in crate::agent_runtime::agent_loop) async fn begin_next_turn(&mut self) {
        if self.active.is_some()
            || !self.dispatch_enabled
            || self.state.snapshot.lifecycle != super::super::AgentLifecycleState::Active
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
                matches!(candidate.delivery_state, MailboxDeliveryState::Pending)
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
            leading.claim(input.turn_id.clone());
        }
        for leading in leading_inputs.iter().rev() {
            next.pending_inputs.push_front(leading.clone());
        }
        input.claim(input.turn_id.clone());
        next.active_input = Some(input.clone());
        next.refresh_mailbox_snapshot();
        next.snapshot.active_turn_id = Some(input.turn_id.clone());
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
            self.state.snapshot.active_turn_id = Some(input.turn_id.clone());
            self.fault(error.to_string()).await;
            return;
        }

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
            .active_turn_id
            .clone()
            .expect("started turn must have an id");
        let completion_cancellation = cancellation.clone();
        let completion_identity = identity.clone();
        tokio::spawn(async move {
            let completion = match worker.await {
                Ok(completion) => completion,
                Err(error) => TurnCompletion {
                    turn_id: completion_turn_id,
                    identity: completion_identity,
                    start_revision,
                    session: None,
                    result: Err(format!("turn task join failed: {error}")),
                    finalized_with_tool: None,
                    cancelled: completion_cancellation.is_cancelled() || error.is_cancelled(),
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
                .active_turn_id
                .clone()
                .expect("started turn must have an id"),
            thread_id,
            identity,
            start_revision,
            cancellation,
            abort_handle,
            settled,
            cancelling: false,
            kind: ActiveKind::Running,
            checkpoint_sequence: 0,
            steer_sender,
            budget_refresh,
        });
    }

    pub(in crate::agent_runtime::agent_loop) async fn interrupt_active_turn(
        &mut self,
        reason: &str,
    ) -> AgentRuntimeResult<()> {
        let Some(active) = &mut self.active else {
            return Err(AgentRuntimeError::NoActiveTurn(
                self.state.snapshot.identity.id.clone(),
            ));
        };
        if active.cancelling {
            return Ok(());
        }
        active.cancelling = true;
        active.cancellation.cancel();
        let next = self.state.clone();
        self.commit_transition(
            super::super::persist::TransitionCommit::new(next),
            |snapshot| AgentRuntimeEventKind::StateChanged {
                snapshot: Box::new(snapshot),
            },
        )
        .await?;

        let active = self
            .active
            .as_mut()
            .expect("running turn must remain while cancelling");
        let grace = self.cancel_grace.min(Duration::from_secs(1));
        if tokio::time::timeout(grace, &mut active.settled)
            .await
            .is_err()
        {
            active.abort_handle.abort();
            let _ = (&mut active.settled).await;
        }
        self.flush_pending_traces().await?;
        self.flush_pending_observations().await?;
        let active = self
            .active
            .take()
            .expect("running turn must remain until cancellation is committed");
        let (outcome, _, _) = turn_outcome(
            active.turn_id.clone(),
            active.thread_id,
            Err(reason.to_string()),
            true,
        );
        let mut next = self.state.clone();
        for input in &mut next.pending_inputs {
            if matches!(
                &input.delivery_state,
                MailboxDeliveryState::Claimed { turn_id, .. } if turn_id == &active.turn_id
            ) {
                input.delivery_state = MailboxDeliveryState::Pending;
                input.turn_id = TurnId::generate();
            }
        }
        next.active_input = None;
        next.refresh_mailbox_snapshot();
        next.snapshot.active_turn_id = None;
        next.snapshot.last_turn = Some(outcome.clone());
        self.commit_transition(
            super::super::persist::TransitionCommit::new(next),
            |snapshot| AgentRuntimeEventKind::TurnFinished {
                outcome,
                snapshot: Box::new(snapshot),
                finalized_with_tool: None,
            },
        )
        .await?;
        if self.dispatch_enabled && self.state.has_triggering_input() {
            self.begin_next_turn().await;
        }
        Ok(())
    }
}
