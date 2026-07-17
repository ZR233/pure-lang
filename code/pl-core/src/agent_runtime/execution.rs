use pl_model::TokenUsage;
use pl_trace::{AgentEvent, TraceEvent, TraceEventKind, TracePartKind};
use tokio_util::sync::CancellationToken;

use super::host::AgentTurnFactory;
use super::state::unix_timestamp;
use super::{
    AgentActivityState, AgentExecutionPolicy, AgentRuntimeHost, AgentTurnOutcome,
    AgentTurnPreparationContext, SessionId, TurnFinalizationPolicy, TurnId, TurnOutcomeKind,
};
use crate::{AgentSession, TraceRecorder, TurnResult, TurnResultStatus};

pub(crate) struct TurnCompletion {
    pub(crate) turn_id: TurnId,
    pub(crate) start_revision: u64,
    /// worker 正常返回时携带更新后的 session；任务被 abort 时必须保留 actor 中的原 session。
    pub(crate) session: Option<AgentSession>,
    pub(crate) result: std::result::Result<TurnResult, String>,
    pub(crate) cancelled: bool,
    pub(crate) next_trace_sequence: u64,
}

pub(crate) async fn execute_turn<H>(
    host: H,
    context: AgentTurnPreparationContext,
    cancellation: CancellationToken,
    durable_trace_tx: tokio::sync::mpsc::UnboundedSender<TraceEvent>,
) -> TurnCompletion
where
    H: AgentRuntimeHost,
{
    let turn_id = context.turn_id.clone();
    let start_revision = context.snapshot.revision;
    let trace_session_id = context.session_id.to_string();
    let initial_trace_sequence = context.trace_sequence;
    let activity_runtime = context.runtime.clone();
    let activity_agent_id = context.snapshot.identity.id.clone();
    let mut session = context.session.clone();
    let (result, session_commit) = match host.turn_factory().prepare_turn(context).await {
        Ok(prepared) => {
            let prepared = prepared.with_runtime_context(&turn_id, cancellation.clone());
            let policy = prepared.policy.clone();
            let session_commit = prepared.session_commit;
            let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(128);
            let activity_turn_id = turn_id.clone();
            tokio::spawn(async move {
                loop {
                    match event_rx.recv().await {
                        Ok(event) => {
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
            let mut result = prepared
                .kernel
                .run_turn_with_trace(
                    &mut session,
                    prepared.request,
                    &mut recorder,
                    prepared.options,
                )
                .await
                .map_err(|error| error.to_string());
            enforce_finalization(&mut result, &policy);
            (result, session_commit)
        }
        Err(error) => (
            Err(error.to_string()),
            super::AgentSessionCommitPolicy::Persist,
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
        start_revision,
        session: match session_commit {
            super::AgentSessionCommitPolicy::Persist => Some(session),
            super::AgentSessionCommitPolicy::DiscardTurn => None,
        },
        result,
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
    let (kind, reason, usage, traces, result) = match result {
        Ok(result) if cancelled => (
            TurnOutcomeKind::Cancelled,
            Some("cancelled".to_string()),
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
                result.usage.clone(),
                result.trace_events.clone(),
                Some(result),
            )
        }
        Err(error) if cancelled => (
            TurnOutcomeKind::Cancelled,
            Some(error),
            TokenUsage::default(),
            Vec::new(),
            None,
        ),
        Err(error) => (
            TurnOutcomeKind::Failed,
            Some(error),
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
) {
    let Ok(result) = result else {
        return;
    };
    if result.status != TurnResultStatus::Completed {
        return;
    }
    let TurnFinalizationPolicy::RequiredTool { name } = &policy.finalization else {
        return;
    };
    let finalized = result.trace_events.iter().any(|event| match &event.kind {
        TraceEventKind::TracePartCompleted { item } => {
            item.tool.as_ref().is_some_and(|tool| tool.name == *name)
        }
        TraceEventKind::TracePartStarted { .. }
        | TraceEventKind::TracePartDelta { .. }
        | TraceEventKind::TracePartFailed { .. }
        | TraceEventKind::PlanLifecycleChanged { .. }
        | TraceEventKind::InteractionChanged { .. }
        | TraceEventKind::SkillActivated { .. }
        | TraceEventKind::EnabledToolsRecorded { .. } => false,
    });
    if !finalized {
        result.status = TurnResultStatus::Errored;
        result.error = Some(format!("turn must finalize with tool `{name}`"));
    }
}
