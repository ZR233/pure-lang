use std::time::Duration;

use futures::FutureExt;
use pl_protocol::{ThreadNotification, ThreadNotificationEnvelope};
use pl_trace::TraceEvent;
use tokio::sync::mpsc;

use super::state::AgentRuntimeError;
use super::*;
use crate::thread_event::ObservedTurnEvent;
pub(crate) use command::{AgentLoopCommand, AgentLoopHandle};
use running_turn::{RunningTurn, TurnExecutionTerminal, turn_outcome};

/// 合并短时间内的事件，减少内存投影与异步保存事实的重复快照。
const TRACE_BATCH_MAX_DELAY: Duration = Duration::from_millis(100);
const TRACE_BATCH_MAX_EVENTS: usize = 256;

mod checkpoint;
mod closing;
mod command;
mod commit;
mod completion;
mod input;
mod lifecycle;
mod persist;
mod recovery;
mod running_turn;
mod session_messages;
mod session_sources;
mod session_tasks;
mod submissions;
mod task_delivery;

struct LoopChannels {
    deferred_commands: std::collections::VecDeque<AgentLoopCommand>,
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
    session_waiter: Option<session_messages::SessionWaitReply>,
    task_resources: crate::session_runtime::SessionTaskResources,
    session_runtime: crate::session_runtime::SessionRuntime,
    closing: Option<closing::SessionClose<<H::Lifecycle as AgentLifecycleAdapter>::CloseLease>>,
    dispatch_enabled: bool,
    cancel_grace: Duration,
}

pub(crate) async fn prepare_agent_loop<H>(
    host: H,
    state: ThreadActorState,
    runtime: AgentRuntimeHandle,
    cancel_grace: Duration,
    start_pending_inputs: bool,
    command_capacity: usize,
) -> AgentRuntimeResult<PreparedAgentLoop<H>>
where
    H: AgentRuntimeHost,
{
    let (sender, receiver) = mpsc::channel(command_capacity.max(1));
    let (trace_sender, trace_receiver) = mpsc::unbounded_channel();
    let (observation_sender, observation_receiver) = mpsc::unbounded_channel();
    let handle = AgentLoopHandle::new(sender.clone());
    let dispatch_enabled = start_pending_inputs;
    let initialization = handle.cancellation().drop_guard();
    let context = crate::session_runtime::SessionBuildContext {
        identity: state.snapshot.identity.clone(),
        session: state.session.session.clone(),
        runtime: runtime.for_session(handle.clone()),
        actor: handle.clone(),
        cancellation: handle.cancellation(),
        tool_session: crate::ToolSessionRuntime::default(),
        source: "host".into(),
    };
    context
        .tool_session
        .begin_turn(&context.session)
        .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?;
    let builder = if state.snapshot.state.is_operational() {
        std::panic::AssertUnwindSafe(async {
            host.turn_factory().prepare_session(context.clone()).await
        })
        .catch_unwind()
        .await
        .map_err(|_| {
            AgentRuntimeError::Lifecycle("session factory panicked during preparation".into())
        })?
        .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))?
    } else {
        crate::session_runtime::SessionRuntimeBuilder::default()
    };
    let built = std::panic::AssertUnwindSafe(builder.build(context))
        .catch_unwind()
        .await
        .map_err(|_| AgentRuntimeError::Lifecycle("session tool assembly panicked".into()))?;
    let mut session_runtime = built.map_err(|error| {
        AgentRuntimeError::Lifecycle(format!(
            "{error}: {}",
            std::error::Error::source(&error).map_or(String::new(), ToString::to_string)
        ))
    })?;
    let mut proposed_inbox = state.session.inbox.clone();
    let source_ids: Vec<_> = session_runtime.source_ids().map(str::to_owned).collect();
    let obsolete: Vec<_> = proposed_inbox
        .reservations()
        .filter(|id| {
            id.strip_prefix("source:")
                .is_some_and(|source| !source_ids.iter().any(|id| id == source))
        })
        .map(str::to_owned)
        .collect();
    for id in obsolete {
        proposed_inbox.release_reservation(&id);
    }
    for source in &source_ids {
        if let Err(error) = proposed_inbox.reserve(&format!("source:{source}")) {
            session_runtime.rollback().await.map_err(|cleanup| {
                AgentRuntimeError::Lifecycle(format!("{error}; rollback: {cleanup}"))
            })?;
            return Err(AgentRuntimeError::Lifecycle(error.to_string()));
        }
    }
    initialization.disarm();
    Ok(PreparedAgentLoop {
        handle,
        owner: AgentLoop {
            host,
            state,
            runtime,
            channels: LoopChannels {
                deferred_commands: std::collections::VecDeque::new(),
                command_sender: sender,
                command_receiver: receiver,
                trace_sender,
                trace_receiver,
                observation_sender,
                observation_receiver,
            },
            active: None,
            session_waiter: None,
            task_resources: Default::default(),
            session_runtime,
            closing: None,
            dispatch_enabled,
            cancel_grace,
        },
    })
}

pub(crate) struct PreparedAgentLoop<H: AgentRuntimeHost> {
    handle: AgentLoopHandle,
    owner: AgentLoop<H>,
}

impl<H: AgentRuntimeHost> PreparedAgentLoop<H> {
    pub(crate) fn handle(&self) -> AgentLoopHandle {
        self.handle.clone()
    }
    pub(crate) fn start(self) -> AgentLoopHandle {
        self.handle.mark_ready();
        tokio::spawn(self.owner.run());
        self.handle
    }
    pub(crate) async fn rollback(mut self) -> AgentRuntimeResult<()> {
        self.owner
            .session_runtime
            .rollback()
            .await
            .map_err(|error| AgentRuntimeError::Lifecycle(error.to_string()))
    }
}

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    async fn run(mut self) {
        if let Err(error) = self.initialize_session_sources().await {
            self.dispatch_enabled = false;
            self.fault(format!("session source initialization failed: {error}"))
                .await;
        }
        if let Err(error) = self.recover_session_tasks().await {
            self.dispatch_enabled = false;
            self.fault(format!("session task recovery failed: {error}"))
                .await;
        }
        if self.dispatch_enabled
            && self.state.snapshot.state.is_queued()
            && self.state.has_triggering_input()
        {
            self.begin_next_turn().boxed().await;
        }
        if let AgentState::Closing(state) = &self.state.snapshot.state
            && state.error().is_none()
        {
            let disposition = state.workspace_disposition();
            if let Err(error) = self.close(disposition).await {
                let _ = self.record_close_error(error).await;
            }
        }
        loop {
            if self.host.repository().is_durable(
                &self.state.snapshot.identity.id,
                self.state.snapshot.revision,
            ) {
                self.state.session.tasks.release_saved_results();
            }
            self.advance_session_close();
            tokio::select! {
                event = closing::next_close_event(&mut self.closing), if self.closing.is_some() => {
                    if let Err(error) = self.handle_close_event(event).await {
                        tracing::error!(%error, "background session close failed");
                        let _ = self.record_close_error(error).await;
                    }
                }
                source = self.session_runtime.next_source(), if self.session_runtime.has_running_sources() => {
                    if let Some((id, result)) = source
                        && let Err(error) = self.finish_session_source(&id, result).await {
                        self.fault(format!("session source settlement failed: {error}")).await;
                    }
                }
                completion = self.task_resources.next_completion(), if self.task_resources.has_completions_or_workers() => {
                    if let Some(completion) = completion {
                        if let Err(error) = self.finish_session_task(completion).boxed().await {
                            self.dispatch_enabled = false;
                            self.task_resources.block_settlement();
                            tracing::error!(%error, "session task completion retained after commit failure");
                            self.fault(format!("session task settlement failed: {error}")).await;
                            continue;
                        }
                        if let Err(error) = self.start_session_tasks().boxed().await {
                            self.fault(format!("session task scheduling failed: {error}")).await;
                        }
                    }
                }
                command = async {
                    match self.channels.deferred_commands.pop_front() {
                        Some(command) => Some(command),
                        None => self.channels.command_receiver.recv().await,
                    }
                } => {
                    let Some(command) = command else {
                        break;
                    };
                    match command {
                        AgentLoopCommand::AdmitToolTasks { turn_id, tasks, reply } => {
                            let result = self.admit_tool_tasks(&turn_id, tasks).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::SelectToolTaskResults { turn_id, ids, deadline, reply } => {
                            let result = self.select_tool_task_results(&turn_id, &ids, deadline).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::ToolTaskOutput { id, delta } => {
                            if let Err(error) = self.append_session_task_output(&id, &delta).boxed().await {
                                tracing::warn!(%error, task_id = %id, "task output preview projection failed");
                            }
                        }
                        AgentLoopCommand::ListToolTasks { status, cursor, reply } => {
                            let _ = reply.send(Ok(self.state.session.tasks.list(status, cursor.as_deref())));
                        }
                        AgentLoopCommand::ReadToolTaskResult { id, reply } => {
                            let result = self.read_complete_task(&id).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::GetToolTask { id, reply } => {
                            let result = self.state.session.tasks.get(&id).cloned()
                                .map_err(|error| AgentRuntimeError::InvalidInput(error.to_string()));
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::CancelToolTask { id, reply } => {
                            let result = self.cancel_session_task(&id).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::ToolTaskRunning { id, reply } => {
                            let result = self.mark_session_task_running(&id).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::WaitSessionEvents { reply } => {
                            self.wait_session_events(reply);
                        }
                        AgentLoopCommand::PublishSessionEvent { source, id, event, reply } => {
                            let result = self.publish_session_event(&source, &id, *event).boxed().await;
                            let _ = reply.send(result);
                        }
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
                        AgentLoopCommand::ChangeIdleThreadMode { mode_id, reply } => {
                            let result = self.change_idle_thread_mode(mode_id).await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::PreviewConversationRecovery { target, reply } => {
                            let _ = reply.send(self.preview_conversation_recovery(target));
                        }
                        AgentLoopCommand::RecoverConversation { request, reply } => {
                            let result = self.recover_conversation(request).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::RecoverFaulted { reply } => {
                            let result = self.recover_faulted().boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::CancelTurn { turn_id, reply } => {
                            let result = self.cancel_turn(turn_id).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::SetActivity {
                            turn_id,
                            activity,
                            reply,
                        } => {
                            let result = self.set_activity(turn_id, activity).boxed().await;
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::Checkpoint { checkpoint, reply } => {
                            let result = self.checkpoint(*checkpoint).boxed().await;
                            if let Err(error) = &result {
                                tracing::error!(
                                    agent_id = %self.state.snapshot.identity.id,
                                    error = %error,
                                    "checkpoint was rejected without faulting the agent"
                                );
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
                                tracing::error!(
                                    agent_id = %self.state.snapshot.identity.id,
                                    error = %error,
                                    "thread facts were rejected without faulting the agent"
                                );
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
                        AgentLoopCommand::ReadThreadContext { reply } => {
                            let _ = reply.send(Ok(self.state.session.clone()));
                        }
                        AgentLoopCommand::ReadSubmissions {
                            offset,
                            limit,
                            reply,
                        } => {
                            let result = self.read_submissions(offset, limit);
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::StartPendingInputs { reply } => {
                            self.dispatch_enabled = true;
                            self.begin_next_turn().boxed().await;
                            let _ = reply.send(Ok(()));
                        }
                        AgentLoopCommand::Close {
                            workspace_disposition,
                            reply,
                        } => {
                            let result = self.close(workspace_disposition).boxed().await;
                            self.wake_session_waiter();
                            let _ = reply.send(result);
                        }
                        AgentLoopCommand::TurnFinished(completion) => {
                            self.finish_turn(*completion).boxed().await;
                        }
                        AgentLoopCommand::Evict { reply } => {
                            // 与输入接受共享 owner 命令序列，避免检查后又提交了新事实。
                            let snapshot = &self.state.snapshot;
                            if (snapshot.identity.parent_id.is_some() && !matches!(snapshot.state, AgentState::Closed(_)))
                                || self.active.is_some() || snapshot.pending_inputs > 0
                                || self.session_waiter.as_ref().is_some_and(|waiter| !waiter.is_closed())
                                || self.task_resources.has_work() || self.state.session.tasks.active_ids().next().is_some()
                                || self.session_runtime.has_sources()
                                || self.closing.is_some() || matches!(snapshot.state, AgentState::Closing(_))
                                || snapshot.active_turn_id().is_some() || snapshot.state.is_budget_paused()
                                || !self.host.repository().is_durable(&snapshot.identity.id, snapshot.revision)
                            {
                                let _ = reply.send(Err(AgentRuntimeError::InvalidInput(
                                    format!("agent {} is busy or has unsaved facts", snapshot.identity.id),
                                )));
                            } else {
                                let _ = reply.send(Ok(snapshot.clone()));
                                break;
                            }
                        }
                        AgentLoopCommand::Shutdown { reply } => {
                            let result = self.shutdown().boxed().await;
                            let finished = result.is_ok();
                            if let Err(error) = &result {
                                if matches!(self.state.snapshot.state, AgentState::Closing(_)) {
                                    let _ = self.record_close_error(error.clone()).await;
                                } else { self.fault(error.to_string()).await; }
                            }
                            let _ = reply.send(result);
                            if finished { break; }
                        }
                    }
                }
                trace = self.channels.trace_receiver.recv() => {
                    let Some(trace) = trace else {
                        continue;
                    };
                    if let Err(error) = self.persist_trace_batch(vec![trace]).boxed().await {
                        self.mark_projection_failure(&error);
                        tracing::error!(
                            agent_id = %self.state.snapshot.identity.id,
                            error = %error,
                            "trace projection was rejected without faulting the agent"
                        );
                    }
                }
                observation = self.channels.observation_receiver.recv() => {
                    let Some(observation) = observation else {
                        continue;
                    };
                    if let Err(error) = self.persist_observation(observation).boxed().await {
                        self.mark_projection_failure(&error);
                        tracing::error!(
                            agent_id = %self.state.snapshot.identity.id,
                            error = %error,
                            "observation projection was rejected without faulting the agent"
                        );
                    }
                }
            }
        }
        self.stop_active_turn();
        if let Err(error) = self.release_turn_deliveries(None).await {
            tracing::error!(%error, "undelivered task results retained during fault settlement");
        }
        if let Some(reply) = self.session_waiter.take() {
            let _ = reply.send(Err(crate::session_runtime::SessionInboxError::Closed.into()));
        }
    }

    /// 合法模型输入产生的 Thread/trace 投影错误只终结当前 Turn，不把 Agent
    /// 永久置为 Faulted。持久化或外部观察者错误不属于此分类。
    fn mark_projection_failure(&mut self, error: &AgentRuntimeError) {
        let AgentRuntimeError::ThreadEvents(detail) = error else {
            return;
        };
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if active.projection_failure.is_none() {
            active.projection_failure = Some(format!("turn protocol projection failed: {detail}"));
            active.cancellation.cancel();
            // 复用 owner 的取消收束路径：生产者不响应令牌时仍有宽限期与强制停止。
            let (reply, _receiver) = tokio::sync::oneshot::channel();
            self.channels
                .deferred_commands
                .push_front(AgentLoopCommand::CancelTurn {
                    turn_id: active.turn_id.clone(),
                    reply,
                });
        }
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
        self.interrupt_active_turn(pl_protocol::TurnCancellationCause::UserRequested)
            .await
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
            .or_else(|| self.state.snapshot.active_turn_id().cloned());
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
                turn_outcome(
                    turn_id,
                    thread_id,
                    TurnExecutionTerminal::WorkerFailed {
                        error: reason.clone(),
                    },
                    None,
                )
                .outcome
            });
        tracing::error!(
            agent_id = %self.state.snapshot.identity.id,
            turn_id = turn_id.as_ref().map(TurnId::as_str),
            thread_id = thread_id.as_ref().map(ThreadId::as_str),
            reason_bytes = reason.len(),
            "agent runtime is committing a faulted state"
        );
        self.stop_active_turn();
        if let Err(error) = self.release_turn_deliveries(turn_id.as_ref()).await {
            tracing::error!(%error, "undelivered task results retained during fault settlement");
        }
        let mut next = self.state.clone();
        if let Err(error) = next.snapshot.transition(AgentCommand::Fault {
            error: pl_protocol::StateError {
                code: "agentRuntimeRecoverable".to_string(),
                message: reason.clone(),
                retryable: false,
            },
            turn_id: turn_id.clone(),
            classification: AgentFaultClassification::RecoverableRuntime,
        }) {
            tracing::error!(
                agent_id = %self.state.snapshot.identity.id,
                transition_error = %error,
                "agent fault transition was rejected"
            );
            return;
        }
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
            "fault mutation admission failed; retaining the previous hot state"
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_runtime::test_support::{FactoryMode, TestRepository};
    use crate::agent_runtime::tests::{TestHost, registration, test_options};
    use pl_trace::{
        InMemoryTraceEventSink, TraceEventDraft, TraceEventSink, TracePart, TraceTextChannel,
    };

    #[tokio::test]
    async fn invalid_batch_keeps_prefix_settles_blocked_producer_and_isolates_next_turn() {
        let repository = TestRepository::empty();
        let host = TestHost::new(repository.clone(), FactoryMode::Block);
        let runtime = AgentRuntime::start(host.clone(), test_options())
            .await
            .unwrap();
        let handle = runtime.handle();
        let id = ThreadId::new("isolated").unwrap();
        handle
            .register(registration(id.as_str(), "chat"))
            .await
            .unwrap();
        let prepared = prepare_agent_loop(
            host,
            repository.state(&id),
            handle,
            Duration::from_millis(10),
            true,
            32,
        )
        .await
        .unwrap();
        prepared.handle.mark_ready();
        let mut owner = prepared.owner;
        let first = owner
            .submit(AgentSubmitRequest::start(id.clone(), "first"))
            .await
            .unwrap();
        let old_sender = owner.channels.trace_sender.clone();
        let sink = InMemoryTraceEventSink::new(id.to_string(), owner.state.session.trace_sequence);
        let item =
            TracePart::streaming_text(first.as_str(), "kept-prefix", 0, TraceTextChannel::Final, 1);
        let start = sink
            .emit(TraceEventDraft::start(
                1,
                first.to_string(),
                item.item_id().into(),
                item.source(),
                item.state().clone(),
            ))
            .unwrap();
        let invalid = sink
            .emit(TraceEventDraft::start(
                1,
                "wrong-turn".into(),
                "invalid-item".into(),
                item.source(),
                item.state().clone(),
            ))
            .unwrap();
        let error = owner
            .persist_trace_batch(vec![start, invalid.clone()])
            .await
            .unwrap_err();
        let snapshot = owner.runtime.thread_snapshot(&id).unwrap();
        assert!(snapshot.items.iter().any(|item| item.id == "kept-prefix"));
        assert!(!snapshot.items.iter().any(|item| item.id == "invalid-item"));
        owner.mark_projection_failure(&error);
        let Some(AgentLoopCommand::CancelTurn { turn_id, .. }) =
            owner.channels.deferred_commands.pop_front()
        else {
            panic!("protocol failure schedules bounded producer shutdown");
        };
        tokio::time::timeout(Duration::from_secs(2), owner.cancel_turn(turn_id))
            .await
            .unwrap()
            .unwrap();
        assert!(owner.active.is_none());
        assert!(owner.state.snapshot.state.is_idle());
        assert!(matches!(
            owner.state.snapshot.last_turn.as_ref().unwrap().outcome,
            pl_protocol::TurnOutcome::Failed(_)
        ));
        let late_completion = match owner.channels.deferred_commands.pop_front() {
            Some(command) => command,
            None => tokio::time::timeout(
                Duration::from_secs(2),
                owner.channels.command_receiver.recv(),
            )
            .await
            .unwrap()
            .unwrap(),
        };
        let second = owner
            .submit(AgentSubmitRequest::start(id.clone(), "second"))
            .await
            .unwrap();
        assert_ne!(first, second);
        assert!(old_sender.send(invalid).is_err());
        let AgentLoopCommand::TurnFinished(completion) = late_completion else {
            panic!("worker completion");
        };
        owner.finish_turn(*completion).await;
        assert_eq!(owner.active.as_ref().unwrap().turn_id, second);
        assert_eq!(repository.commits().iter().flat_map(|commit| &commit.facts.runtime_events).filter(|event| matches!(&event.kind, AgentRuntimeEventKind::TurnFinished { outcome, .. } if outcome.turn_id == first)).count(), 1);
        owner.cancel_turn(second).await.unwrap();
        runtime.shutdown().await.unwrap();
    }
}
