use std::sync::Arc;
use std::time::Duration;

use pl_model::TokenUsage;
use pl_protocol::SessionEventKind;
use pl_trace::{AgentEvent, TraceEvent, TraceEventKind, TracePartKind};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

use super::super::host::AgentTurnFactory;
use super::super::state::{AgentRuntimeError, unix_timestamp};
use super::super::{
    AgentActivityState, AgentExecutionPolicy, AgentRuntimeEventKind, AgentRuntimeHost,
    AgentRuntimeResult, AgentSessionCommitPolicy, AgentTurnCheckpointHandle,
    AgentTurnMailboxHandle, AgentTurnOutcome, AgentTurnPreparationContext, MailboxDeliveryState,
    SessionId, TurnFinalizationPolicy, TurnId, TurnOutcomeKind,
};
use super::{AgentLoop, AgentLoopCommand};
use crate::session_event::{ObservedTurnEvent, observation_from_agent_event};
use crate::{AgentSession, TraceRecorder, TurnResult, TurnResultStatus};

pub(super) struct RunningTurn {
    pub(super) turn_id: TurnId,
    pub(super) session_id: SessionId,
    pub(super) identity: std::sync::Arc<()>,
    pub(super) start_revision: u64,
    pub(super) cancellation: CancellationToken,
    pub(super) abort_handle: AbortHandle,
    pub(super) settled: oneshot::Receiver<()>,
    pub(super) cancelling: bool,
    pub(super) checkpoint_sequence: u64,
    pub(super) steer_sender: mpsc::UnboundedSender<super::super::PendingAgentInput>,
}

impl<H> AgentLoop<H>
where
    H: AgentRuntimeHost,
{
    pub(super) async fn begin_next_turn(&mut self) {
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
            .is_some_and(|input| input.session_id != self.state.session.id)
        {
            self.fault_in_memory(
                AgentRuntimeError::SessionMismatch {
                    agent_id: self.state.snapshot.identity.id.clone(),
                    expected: self.state.session.id.clone(),
                    actual: self.state.pending_inputs[0].session_id.clone(),
                }
                .to_string(),
            );
            return;
        }
        let mut next = self.state.clone();
        let Some(mut input) = next.pending_inputs.pop_front() else {
            return;
        };
        input.claim(input.turn_id.clone());
        next.active_input = Some(input.clone());
        next.refresh_mailbox_snapshot();
        next.snapshot.activity = AgentActivityState::Running;
        next.snapshot.active_turn_id = Some(input.turn_id.clone());
        let committed = self
            .commit_transition(next, Vec::new(), |snapshot| {
                AgentRuntimeEventKind::TurnStarted {
                    turn_id: input.turn_id.clone(),
                    session_id: input.session_id.clone(),
                    claimed_inputs: Vec::new(),
                    snapshot,
                }
            })
            .await;
        if committed.is_err() {
            return;
        }

        let cancellation = CancellationToken::new();
        let (steer_sender, steer_receiver) = mpsc::unbounded_channel();
        let mailbox = AgentTurnMailboxHandle::new(steer_receiver, Vec::new());
        let session_id = self.state.session.id.clone();
        let context = AgentTurnPreparationContext {
            snapshot: self.state.snapshot.clone(),
            turn_id: input.turn_id.clone(),
            session_id: input.session_id.clone(),
            input,
            leading_inputs: Vec::new(),
            session: self.state.session.session.clone(),
            trace_sequence: self.state.session.trace_sequence,
            runtime: self.runtime.clone(),
            cancellation_token: cancellation.clone(),
            mailbox,
        };
        let start_revision = self.state.snapshot.revision;
        let identity = Arc::new(());
        let initial_trace_sequence = context.trace_sequence;
        let worker_host = self.host.clone();
        let worker_cancellation = cancellation.clone();
        let worker_identity = identity.clone();
        let durable_trace_tx = self.trace_sender.clone();
        let observation_tx = self.observation_sender.clone();
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
        let completion_sender = self.sender.clone();
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
            session_id,
            identity,
            start_revision,
            cancellation,
            abort_handle,
            settled,
            cancelling: false,
            checkpoint_sequence: 0,
            steer_sender,
        });
    }

    pub(super) async fn interrupt_active_turn(&mut self, reason: &str) -> AgentRuntimeResult<()> {
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
        let mut next = self.state.clone();
        next.snapshot.activity = AgentActivityState::Cancelling;
        self.commit_transition(next, Vec::new(), |snapshot| {
            AgentRuntimeEventKind::StateChanged { snapshot }
        })
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
            active.session_id,
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
        next.snapshot.activity = if next.has_triggering_input() {
            AgentActivityState::Queued
        } else {
            AgentActivityState::Idle
        };
        self.commit_transition(next, Vec::new(), |snapshot| {
            AgentRuntimeEventKind::TurnFinished {
                outcome,
                snapshot,
                finalized_with_tool: None,
            }
        })
        .await?;
        if self.dispatch_enabled && self.state.has_triggering_input() {
            self.begin_next_turn().await;
        }
        Ok(())
    }
}

pub(crate) struct TurnCompletion {
    pub(crate) turn_id: TurnId,
    pub(crate) identity: std::sync::Arc<()>,
    pub(crate) start_revision: u64,
    /// worker 正常返回时携带更新后的 session；任务被 abort 时必须保留 actor 中的原 session。
    pub(crate) session: Option<AgentSession>,
    pub(crate) result: std::result::Result<TurnResult, String>,
    pub(crate) finalized_with_tool: Option<String>,
    pub(crate) cancelled: bool,
    pub(crate) next_trace_sequence: u64,
}

pub(crate) async fn execute_turn<H>(
    host: H,
    mut context: AgentTurnPreparationContext,
    identity: std::sync::Arc<()>,
    cancellation: CancellationToken,
    durable_trace_tx: tokio::sync::mpsc::UnboundedSender<TraceEvent>,
    observation_tx: tokio::sync::mpsc::UnboundedSender<ObservedTurnEvent>,
) -> TurnCompletion
where
    H: AgentRuntimeHost,
{
    let leading_inputs = context.leading_inputs.clone();
    for input in &leading_inputs {
        context.session.push_user_prompt(input.message.clone());
    }
    let turn_id = context.turn_id.clone();
    let start_revision = context.snapshot.revision;
    let framework_session_id = context.session_id.clone();
    let trace_session_id = context.session_id.to_string();
    let initial_trace_sequence = context.trace_sequence;
    let activity_runtime = context.runtime.clone();
    let activity_agent_id = context.snapshot.identity.id.clone();
    let checkpoint = AgentTurnCheckpointHandle::new(
        context.runtime.clone(),
        context.snapshot.identity.id.clone(),
        context.turn_id.clone(),
        context.session_id.clone(),
    );
    let mailbox = context.mailbox.clone();
    let mut session = context.session.clone();
    let (result, session_commit, finalized_with_tool) = match host
        .turn_factory()
        .prepare_turn(context)
        .await
    {
        Ok(prepared) => {
            let prepared =
                prepared.with_runtime_context(&turn_id, cancellation.clone(), checkpoint, mailbox);
            for section in &prepared.pinned_context {
                session.upsert_pinned_context(section.clone());
            }
            let policy = prepared.policy.clone();
            let session_commit = prepared.session_commit;
            let session_runtime_result = if let Some(runtime) = &prepared.session_runtime {
                match activity_runtime.session_snapshot(&framework_session_id) {
                    Ok(current) => {
                        let updated_at = unix_timestamp();
                        let snapshot =
                            runtime.merge_with(&framework_session_id, &current, updated_at);
                        activity_runtime
                            .record_session_facts(
                                activity_agent_id.clone(),
                                framework_session_id.clone(),
                                vec![crate::SessionEventFact::durable(
                                    Some(activity_agent_id.to_string()),
                                    Some(turn_id.to_string()),
                                    updated_at,
                                    SessionEventKind::RuntimeChanged {
                                        runtime: Box::new(snapshot),
                                    },
                                )],
                            )
                            .await
                    }
                    Err(error) => Err(error),
                }
            } else {
                Ok(())
            };
            let mut result = 'execute: {
                if let Err(error) = session_runtime_result {
                    break 'execute Err(error.to_string());
                }
                let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(128);
                let activity_turn_id = turn_id.clone();
                let observation_turn_id = turn_id.to_string();
                let observation_session_id = trace_session_id.clone();
                let event_task = tokio::spawn(async move {
                    loop {
                        match event_rx.recv().await {
                            Ok(event) => {
                                if let Some(observation) = observation_from_agent_event(&event) {
                                    let _ = observation_tx.send(ObservedTurnEvent {
                                        turn_id: observation_turn_id.clone(),
                                        session_id: observation_session_id.clone(),
                                        observation,
                                    });
                                }
                                if let Some(activity) = activity_for_event(&event) {
                                    let _ = activity_runtime
                                        .set_activity(
                                            activity_agent_id.clone(),
                                            activity_turn_id.clone(),
                                            activity,
                                        )
                                        .await;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        }
                    }
                });
                let mut recorder = TraceRecorder::streaming(
                    trace_session_id,
                    event_tx,
                    initial_trace_sequence,
                    durable_trace_tx,
                );
                for input in &leading_inputs {
                    recorder.user_text_item_with_id(
                        turn_id.as_str(),
                        format!("{turn_id}-mail-{}", input.mail_id),
                        input.message.clone(),
                        Vec::new(),
                    );
                }
                let result = prepared
                    .kernel
                    .run_turn_with_trace(
                        &mut session,
                        prepared.request,
                        &mut recorder,
                        prepared.options,
                    )
                    .await
                    .map_err(|error| error.to_string());
                drop(recorder);
                let _ = event_task.await;
                break 'execute result;
            };
            let finalized_with_tool = enforce_finalization(&mut result, &policy);
            (result, session_commit, finalized_with_tool)
        }
        Err(error) => (
            Err(error.to_string()),
            AgentSessionCommitPolicy::Persist,
            None,
        ),
    };
    let next_trace_sequence = result
        .as_ref()
        .ok()
        .and_then(|result| result.trace_events.iter().map(|event| event.sequence).max())
        .map(|sequence| sequence.saturating_add(1))
        .unwrap_or(initial_trace_sequence);
    TurnCompletion {
        turn_id,
        identity,
        start_revision,
        session: match session_commit {
            AgentSessionCommitPolicy::Persist => Some(session),
            AgentSessionCommitPolicy::DiscardTurn => None,
        },
        result,
        finalized_with_tool,
        cancelled: cancellation.is_cancelled(),
        next_trace_sequence,
    }
}

fn activity_for_event(event: &AgentEvent) -> Option<AgentActivityState> {
    match event {
        AgentEvent::TracePartStarted { item } => match item.kind {
            TracePartKind::Tool | TracePartKind::Agent => Some(AgentActivityState::WaitingTool),
            TracePartKind::Text
            | TracePartKind::Thinking
            | TracePartKind::Turn
            | TracePartKind::Inference
            | TracePartKind::Plan => Some(AgentActivityState::Running),
        },
        AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. } => Some(AgentActivityState::Running),
        AgentEvent::InteractionChanged { event } => match event.interaction.status {
            pl_protocol::InteractionStatus::Pending => Some(AgentActivityState::WaitingInteraction),
            pl_protocol::InteractionStatus::Resolved
            | pl_protocol::InteractionStatus::Cancelled
            | pl_protocol::InteractionStatus::Expired => Some(AgentActivityState::Running),
        },
        AgentEvent::AgentStateChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::SubAgentActivity { .. }
        | AgentEvent::TodoListUpdated { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. } => None,
    }
}

pub(crate) fn turn_outcome(
    turn_id: TurnId,
    session_id: SessionId,
    result: std::result::Result<TurnResult, String>,
    cancelled: bool,
) -> (AgentTurnOutcome, Vec<TraceEvent>, Option<TurnResult>) {
    let (kind, reason, failure, usage, traces, result) = match result {
        Ok(result) if cancelled => (
            TurnOutcomeKind::Cancelled,
            Some("cancelled".to_string()),
            None,
            result.usage.clone(),
            result.trace_events.clone(),
            Some(result),
        ),
        Ok(result) => {
            let kind = match result.status {
                TurnResultStatus::Completed => TurnOutcomeKind::Completed,
                TurnResultStatus::Aborted if result.budget_limit_kind.is_some() => {
                    TurnOutcomeKind::BudgetLimited
                }
                TurnResultStatus::Aborted => TurnOutcomeKind::Cancelled,
                TurnResultStatus::Errored => TurnOutcomeKind::Failed,
            };
            let reason = result.error.clone().or_else(|| {
                result
                    .abort_reason
                    .map(|reason| reason.as_str().to_string())
            });
            (
                kind,
                reason,
                result.failure.clone(),
                result.usage.clone(),
                result.trace_events.clone(),
                Some(result),
            )
        }
        Err(error) if cancelled => (
            TurnOutcomeKind::Cancelled,
            Some(error),
            None,
            TokenUsage::default(),
            Vec::new(),
            None,
        ),
        Err(error) => (
            TurnOutcomeKind::Failed,
            Some(error),
            Some(pl_protocol::TurnFailure::permanent(
                pl_protocol::TurnFailureCategory::Internal,
                "agent runtime execution failed",
            )),
            TokenUsage::default(),
            Vec::new(),
            None,
        ),
    };
    (
        AgentTurnOutcome {
            turn_id,
            session_id,
            kind,
            reason,
            failure,
            usage,
            finished_at: unix_timestamp(),
        },
        traces,
        result,
    )
}

pub(crate) fn add_usage(total: &mut TokenUsage, delta: &TokenUsage) {
    total.prompt_tokens = total.prompt_tokens.saturating_add(delta.prompt_tokens);
    total.completion_tokens = total
        .completion_tokens
        .saturating_add(delta.completion_tokens);
    total.total_tokens = total.total_tokens.saturating_add(delta.total_tokens);
    total.cached_prompt_tokens = total
        .cached_prompt_tokens
        .saturating_add(delta.cached_prompt_tokens);
    total.reasoning_tokens = total
        .reasoning_tokens
        .saturating_add(delta.reasoning_tokens);
}

fn enforce_finalization(
    result: &mut std::result::Result<TurnResult, String>,
    policy: &AgentExecutionPolicy,
) -> Option<String> {
    let Ok(result) = result else {
        return None;
    };
    if result.status != TurnResultStatus::Completed {
        return None;
    }
    let TurnFinalizationPolicy::RequiredTool { name } = &policy.finalization else {
        return None;
    };
    let latest_tool = result
        .trace_events
        .iter()
        .rev()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item }
                if item.tool.as_ref().is_some_and(|tool| tool.name == *name) =>
            {
                Some(Ok(()))
            }
            TraceEventKind::TracePartFailed { item, error }
                if item.tool.as_ref().is_some_and(|tool| tool.name == *name) =>
            {
                Some(Err(error.clone()))
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        });
    if matches!(latest_tool, Some(Ok(()))) {
        return Some(name.clone());
    }
    let (category, message) = latest_tool.and_then(Result::err).map_or_else(
        || {
            (
                pl_protocol::TurnFailureCategory::Validation,
                format!("turn must finalize with tool `{name}`"),
            )
        },
        |error| (pl_protocol::TurnFailureCategory::Tool, error),
    );
    result.status = TurnResultStatus::Errored;
    result.error = Some(message.clone());
    result.failure = Some(pl_protocol::TurnFailure::permanent(category, message));
    None
}

#[cfg(test)]
mod tests {
    use pl_protocol::TurnFailureCategory;

    use super::*;

    #[test]
    fn required_tool_finalization_accepts_completed_tool() {
        let mut result = Ok(completed_result(vec![tool_event(
            "report_completion",
            ToolTraceOutcome::Completed,
        )]));

        let finalized =
            enforce_finalization(&mut result, &required_tool_policy("report_completion"));

        let result = result.unwrap();
        assert_eq!(finalized.as_deref(), Some("report_completion"));
        assert_eq!(result.status, TurnResultStatus::Completed);
        assert_eq!(result.error, None);
        assert_eq!(result.failure, None);
    }

    #[test]
    fn required_tool_finalization_preserves_matching_tool_failure() {
        let mut result = Ok(completed_result(vec![tool_event(
            "report_completion",
            ToolTraceOutcome::Failed("completion scope is not ready for review"),
        )]));

        let finalized =
            enforce_finalization(&mut result, &required_tool_policy("report_completion"));

        let result = result.unwrap();
        assert_eq!(finalized, None);
        assert_eq!(result.status, TurnResultStatus::Errored);
        assert_eq!(
            result.error.as_deref(),
            Some("completion scope is not ready for review")
        );
        assert_eq!(
            result.failure.as_ref().map(|failure| failure.category),
            Some(TurnFailureCategory::Tool)
        );
    }

    #[test]
    fn required_tool_finalization_uses_latest_matching_failure() {
        let mut result = Ok(completed_result(vec![
            tool_event(
                "report_completion",
                ToolTraceOutcome::Failed("first completion failure"),
            ),
            tool_event(
                "report_completion",
                ToolTraceOutcome::Failed("latest completion failure"),
            ),
        ]));

        let finalized =
            enforce_finalization(&mut result, &required_tool_policy("report_completion"));

        assert_eq!(finalized, None);
        assert_eq!(
            result.unwrap().error.as_deref(),
            Some("latest completion failure")
        );
    }

    #[test]
    fn required_tool_finalization_ignores_other_tool_failure() {
        let mut result = Ok(completed_result(vec![tool_event(
            "exec",
            ToolTraceOutcome::Failed("exec failed"),
        )]));

        let finalized =
            enforce_finalization(&mut result, &required_tool_policy("report_completion"));

        let result = result.unwrap();
        assert_eq!(finalized, None);
        assert_eq!(
            result.error.as_deref(),
            Some("turn must finalize with tool `report_completion`")
        );
        assert_eq!(
            result.failure.as_ref().map(|failure| failure.category),
            Some(TurnFailureCategory::Validation)
        );
    }

    #[test]
    fn required_tool_finalization_accepts_success_after_failure() {
        let mut result = Ok(completed_result(vec![
            tool_event(
                "report_completion",
                ToolTraceOutcome::Failed("transient failure"),
            ),
            tool_event("report_completion", ToolTraceOutcome::Completed),
        ]));

        let finalized =
            enforce_finalization(&mut result, &required_tool_policy("report_completion"));

        assert_eq!(finalized.as_deref(), Some("report_completion"));
        assert_eq!(result.unwrap().status, TurnResultStatus::Completed);
    }

    #[test]
    fn required_tool_finalization_rejects_failure_after_success() {
        let mut result = Ok(completed_result(vec![
            tool_event("report_completion", ToolTraceOutcome::Completed),
            tool_event(
                "report_completion",
                ToolTraceOutcome::Failed("latest completion failure"),
            ),
        ]));

        let finalized =
            enforce_finalization(&mut result, &required_tool_policy("report_completion"));

        assert_eq!(finalized, None);
        let result = result.unwrap();
        assert_eq!(result.status, TurnResultStatus::Errored);
        assert_eq!(result.error.as_deref(), Some("latest completion failure"));
        assert_eq!(
            result.failure.as_ref().map(|failure| failure.category),
            Some(TurnFailureCategory::Tool)
        );
    }

    enum ToolTraceOutcome {
        Completed,
        Failed(&'static str),
    }

    fn required_tool_policy(name: &str) -> AgentExecutionPolicy {
        AgentExecutionPolicy {
            finalization: TurnFinalizationPolicy::RequiredTool {
                name: name.to_string(),
            },
            ..AgentExecutionPolicy::default()
        }
    }

    fn completed_result(trace_events: Vec<TraceEvent>) -> TurnResult {
        TurnResult {
            content: String::new(),
            reasoning_content: None,
            model: "test".to_string(),
            usage: TokenUsage::default(),
            last_context_tokens: None,
            context_compactions: Vec::new(),
            session_message_count: 0,
            status: TurnResultStatus::Completed,
            abort_reason: None,
            error: None,
            failure: None,
            budget_limit_kind: None,
            budget_usage: None,
            trace_events,
        }
    }

    fn tool_event(name: &str, outcome: ToolTraceOutcome) -> TraceEvent {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(1);
        let mut recorder = TraceRecorder::new("test-session".to_string(), event_tx, 0);
        let mut item = recorder.tool_item(
            "turn-1",
            "call-1",
            name.to_string(),
            "{}".to_string(),
            None,
            None,
        );
        match outcome {
            ToolTraceOutcome::Completed => {
                item.status = pl_trace::TracePartStatus::Completed;
                recorder.complete_item(item);
            }
            ToolTraceOutcome::Failed(error) => {
                item.status = pl_trace::TracePartStatus::Failed;
                recorder.fail_item(item, error.to_string());
            }
        }
        recorder.drain().pop().unwrap()
    }
}
