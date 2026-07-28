use pl_model::TokenUsage;
use pl_protocol::SessionEventKind;
use pl_trace::{AgentEvent, TraceEvent, TraceEventKind, TracePartKind};
use tokio_util::sync::CancellationToken;

use super::host::AgentTurnFactory;
use super::state::unix_timestamp;
use super::{
    AgentActivityState, AgentExecutionPolicy, AgentRuntimeHost, AgentTurnOutcome,
    AgentTurnPreparationContext, SessionId, TurnFinalizationPolicy, TurnId, TurnOutcomeKind,
};
use crate::session_event::{ObservedTurnEvent, observation_from_agent_event};
use crate::{AgentSession, TraceRecorder, TurnResult, TurnResultStatus};

pub(crate) struct TurnCompletion {
    pub(crate) turn_id: TurnId,
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
    context: AgentTurnPreparationContext,
    cancellation: CancellationToken,
    durable_trace_tx: tokio::sync::mpsc::UnboundedSender<TraceEvent>,
    observation_tx: tokio::sync::mpsc::UnboundedSender<ObservedTurnEvent>,
) -> TurnCompletion
where
    H: AgentRuntimeHost,
{
    let turn_id = context.turn_id.clone();
    let start_revision = context.snapshot.revision;
    let framework_session_id = context.session_id.clone();
    let trace_session_id = context.session_id.to_string();
    let initial_trace_sequence = context.trace_sequence;
    let activity_runtime = context.runtime.clone();
    let activity_agent_id = context.snapshot.identity.id.clone();
    let checkpoint = super::AgentTurnCheckpointHandle::new(
        context.runtime.clone(),
        context.snapshot.identity.id.clone(),
        context.turn_id.clone(),
        context.session_id.clone(),
    );
    let mut session = context.session.clone();
    let (result, session_commit, finalized_with_tool) = match host
        .turn_factory()
        .prepare_turn(context)
        .await
    {
        Ok(prepared) => {
            let prepared =
                prepared.with_runtime_context(&turn_id, cancellation.clone(), checkpoint);
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
            super::AgentSessionCommitPolicy::Persist,
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
        start_revision,
        session: match session_commit {
            super::AgentSessionCommitPolicy::Persist => Some(session),
            super::AgentSessionCommitPolicy::DiscardTurn => None,
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
    if result.trace_events.iter().any(|event| {
        matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if item.tool.as_ref().is_some_and(|tool| tool.name == *name)
        )
    }) {
        return Some(name.clone());
    }
    let failed_tool = result.trace_events.iter().rev().find_map(|event| {
        let TraceEventKind::TracePartFailed { item, error } = &event.kind else {
            return None;
        };
        item.tool
            .as_ref()
            .is_some_and(|tool| tool.name == *name)
            .then_some(error.clone())
    });
    let (category, message) = failed_tool.map_or_else(
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
            "submit_delivery",
            ToolTraceOutcome::Completed,
        )]));

        let finalized = enforce_finalization(&mut result, &required_tool_policy("submit_delivery"));

        let result = result.unwrap();
        assert_eq!(finalized.as_deref(), Some("submit_delivery"));
        assert_eq!(result.status, TurnResultStatus::Completed);
        assert_eq!(result.error, None);
        assert_eq!(result.failure, None);
    }

    #[test]
    fn required_tool_finalization_preserves_matching_tool_failure() {
        let mut result = Ok(completed_result(vec![tool_event(
            "submit_delivery",
            ToolTraceOutcome::Failed("delivery scope is not accepting a delivery"),
        )]));

        let finalized = enforce_finalization(&mut result, &required_tool_policy("submit_delivery"));

        let result = result.unwrap();
        assert_eq!(finalized, None);
        assert_eq!(result.status, TurnResultStatus::Errored);
        assert_eq!(
            result.error.as_deref(),
            Some("delivery scope is not accepting a delivery")
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
                "submit_delivery",
                ToolTraceOutcome::Failed("first delivery failure"),
            ),
            tool_event(
                "submit_delivery",
                ToolTraceOutcome::Failed("latest delivery failure"),
            ),
        ]));

        let finalized = enforce_finalization(&mut result, &required_tool_policy("submit_delivery"));

        assert_eq!(finalized, None);
        assert_eq!(
            result.unwrap().error.as_deref(),
            Some("latest delivery failure")
        );
    }

    #[test]
    fn required_tool_finalization_ignores_other_tool_failure() {
        let mut result = Ok(completed_result(vec![tool_event(
            "exec",
            ToolTraceOutcome::Failed("exec failed"),
        )]));

        let finalized = enforce_finalization(&mut result, &required_tool_policy("submit_delivery"));

        let result = result.unwrap();
        assert_eq!(finalized, None);
        assert_eq!(
            result.error.as_deref(),
            Some("turn must finalize with tool `submit_delivery`")
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
                "submit_delivery",
                ToolTraceOutcome::Failed("transient failure"),
            ),
            tool_event("submit_delivery", ToolTraceOutcome::Completed),
        ]));

        let finalized = enforce_finalization(&mut result, &required_tool_policy("submit_delivery"));

        assert_eq!(finalized.as_deref(), Some("submit_delivery"));
        assert_eq!(result.unwrap().status, TurnResultStatus::Completed);
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
