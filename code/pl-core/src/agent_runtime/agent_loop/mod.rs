use std::time::Duration;

use futures::FutureExt;
use pl_protocol::{ThreadNotification, ThreadNotificationEnvelope};
use pl_trace::TraceEvent;
use tokio::sync::mpsc;

use super::state::{AgentRuntimeError, derive_activity};
use super::*;
use crate::thread_event::ObservedTurnEvent;
pub(crate) use command::{AgentLoopCommand, AgentLoopHandle};
use running_turn::{RunningTurn, turn_outcome};

mod checkpoint;
mod command;
mod commit;
mod completion;
mod input;
mod lifecycle;
mod persist;
mod recovery;
mod running_turn;
mod session_digest;
mod submissions;

struct LoopChannels {
    command_sender: mpsc::Sender<AgentLoopCommand>,
    command_receiver: mpsc::Receiver<AgentLoopCommand>,
    trace_sender: mpsc::UnboundedSender<TraceEvent>,
    trace_receiver: mpsc::UnboundedReceiver<TraceEvent>,
    observation_sender: mpsc::UnboundedSender<ObservedTurnEvent>,
    observation_receiver: mpsc::UnboundedReceiver<ObservedTurnEvent>,
}

struct AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    host: H,
    state: ThreadActorState,
    runtime: AgentRuntimeHandle,
    channels: LoopChannels,
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
    let handle = AgentLoopHandle::new(sender.clone());
    let dispatch_enabled = start_pending_inputs;
    tokio::spawn(
        AgentLoop {
            host,
            state,
            runtime,
            channels: LoopChannels {
                command_sender: sender,
                command_receiver: receiver,
                trace_sender,
                trace_receiver,
                observation_sender,
                observation_receiver,
            },
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
            self.begin_next_turn().boxed().await;
        }
        loop {
            tokio::select! {
                command = self.channels.command_receiver.recv() => {
                    let Some(command) = command else {
                        break;
                    };
                    match command {
                        AgentLoopCommand::Submit { request, reply } => {
                            // `.boxed()`：把命令处理状态机放堆上，避免 debug 构建下
                            // 全部命令分支内联进 run 的 select! 状态机导致超大栈帧。
                            let result = self.submit(request).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::SubmitCurrentSession {
                            root_agent_id,
                            request,
                            reply,
                        } => {
                            let result =
                                self.submit_current_session(root_agent_id, request).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::SubmitInteractionContinuation {
                            root_agent_id,
                            request,
                            reply,
                        } => {
                            let result = self
                                .submit_interaction_continuation(root_agent_id, *request)
                                .boxed()
                                .await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::ReconfigureIdleRole { role, reply } => {
                            let result = self.reconfigure_idle_role(role).await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::PreviewConversationRecovery { target, reply } => {
                            let _ = reply.send(self.preview_conversation_recovery(target));
                        }
                        AgentLoopCommand::RecoverConversation { request, reply } => {
                            let result = self.recover_conversation(request).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::CancelTurn { turn_id, reply } => {
                            let result = self.cancel_turn(turn_id).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::SetActivity {
                            turn_id,
                            kind,
                            reply,
                        } => {
                            let result = self.set_activity(turn_id, kind).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::Checkpoint { checkpoint, reply } => {
                            let result = self.checkpoint(*checkpoint).boxed().await;
                            if let Err(error) = &result {
                                self.fault(error.to_string()).boxed().await;
                            }
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::RecordThreadFacts {
                            thread_id,
                            facts,
                            reply,
                        } => {
                            let result = self.record_thread_facts(thread_id, facts).boxed().await;
                            if let Err(error) = &result {
                                self.fault(error.to_string()).boxed().await;
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
                            detail,
                            reply,
                        } => {
                            let result =
                                self.report_progress(stage, summary, next_step, detail).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::ReadSession { reply } => {
                            let _ = reply.send(self.read_session());
                        }
                        AgentLoopCommand::ReadSubmissions {
                            offset,
                            limit,
                            reply,
                        } => {
                            let result = self.read_submissions(offset, limit).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::StartPendingInputs { reply } => {
                            self.dispatch_enabled = true;
                            self.begin_next_turn().boxed().await;
                            let _ = reply.send(Ok(()));
                        }
                        AgentLoopCommand::Close { reply } => {
                            let result = self.close().boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::TurnFinished(completion) => {
                            self.finish_turn(*completion).boxed().await;
                        }
                        AgentLoopCommand::Shutdown { reply } => {
                            let result = self.shutdown().boxed().await;
                            let _ = reply.send(result);
                            break;
                        }
                    }
                }
                trace = self.channels.trace_receiver.recv() => {
                    let Some(trace) = trace else {
                        continue;
                    };
                    if let Err(error) = self.persist_trace_batch(vec![trace]).boxed().await {
                        self.fault(error.to_string()).boxed().await;
                    }
                }
                observation = self.channels.observation_receiver.recv() => {
                    let Some(observation) = observation else {
                        continue;
                    };
                    if let Err(error) = self.persist_observation(observation).boxed().await {
                        self.fault(error.to_string()).boxed().await;
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
        while let Ok(trace) = self.channels.trace_receiver.try_recv() {
            trace_events.push(trace);
        }
        if trace_events.is_empty() {
            return Ok(());
        }
        self.persist_trace_batch(trace_events).await
    }

    pub(super) async fn flush_pending_observations(&mut self) -> AgentRuntimeResult<()> {
        let mut observations = Vec::new();
        while let Ok(observation) = self.channels.observation_receiver.try_recv() {
            observations.push(observation);
        }
        for observation in observations {
            self.persist_observation(observation).await?;
        }
        Ok(())
    }

    fn stop_active_turn(&mut self) {
        if let Some(active) = self.active.take() {
            active.cancellation.cancel();
            active.abort_handle.abort();
        }
    }

    async fn fault(&mut self, reason: String) {
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
            "agent runtime is committing a faulted state"
        );
        self.stop_active_turn();
        let mut next = self.state.clone();
        next.snapshot.lifecycle = AgentLifecycleState::Faulted;
        next.snapshot.active_turn_id = None;
        next.active_input = None;
        if fault_outcome.is_some() {
            next.snapshot.last_turn = fault_outcome.clone();
        }
        let event_reason = reason.clone();
        if self
            .commit_transition(persist::TransitionCommit::new(next), move |snapshot| {
                AgentRuntimeEventKind::Faulted {
                    reason: event_reason,
                    snapshot: Box::new(snapshot),
                }
            })
            .await
            .is_ok()
        {
            return;
        }
        tracing::error!(
            agent_id = %self.state.snapshot.identity.id,
            reason_bytes = reason.len(),
            "failed to persist the faulted runtime event; using in-memory fallback"
        );
        self.state.snapshot.lifecycle = AgentLifecycleState::Faulted;
        self.state.snapshot.activity = derive_activity(AgentLifecycleState::Faulted, None, false);
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
