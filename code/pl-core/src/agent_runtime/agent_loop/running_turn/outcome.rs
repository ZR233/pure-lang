use pl_model::TokenUsage;
use pl_trace::{TraceEvent, TraceEventKind};

use crate::agent_runtime::state::unix_timestamp;
use crate::agent_runtime::*;
use crate::{TurnResult, TurnResultStatus};

pub(crate) fn turn_outcome(
    turn_id: TurnId,
    thread_id: ThreadId,
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
    let budget_limit = (kind == TurnOutcomeKind::BudgetLimited)
        .then(|| {
            let result = result.as_ref()?;
            Some(pl_protocol::BudgetLimitSnapshot {
                kind: result.budget_limit_kind?,
                usage: result.budget_usage?,
            })
        })
        .flatten();
    let rollover_compacted = result
        .as_ref()
        .is_some_and(|result| result.rollover_compacted);
    let rollover_compaction_error = result
        .as_ref()
        .and_then(|result| result.rollover_compaction_error.clone());
    (
        AgentTurnOutcome {
            turn_id,
            thread_id,
            kind,
            reason,
            failure,
            budget_limit,
            rollover_compacted,
            rollover_compaction_error,
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

pub(super) fn enforce_finalization(
    result: &mut std::result::Result<TurnResult, String>,
    policy: &AgentExecutionPolicy,
) -> Option<String> {
    let Ok(result) = result else {
        return None;
    };
    if result.status != TurnResultStatus::Completed {
        return None;
    }
    if result.ended_for_interaction {
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
    let message = latest_tool
        .and_then(Result::err)
        .unwrap_or_else(|| format!("turn must finalize with tool `{name}`"));
    result.status = TurnResultStatus::Errored;
    result.error = Some(message.clone());
    result.failure = Some(pl_protocol::TurnFailure::permanent(
        pl_protocol::TurnFailureCategory::Validation,
        message,
    ));
    None
}

#[cfg(test)]
mod tests {
    use pl_protocol::TurnFailureCategory;

    use super::*;
    use crate::TraceRecorder;

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
    fn required_tool_finalization_accepts_durable_interaction_boundary() {
        let mut completed = completed_result(Vec::new());
        completed.ended_for_interaction = true;
        let mut result = Ok(completed);

        let finalized = enforce_finalization(&mut result, &required_tool_policy("plan_exit"));

        assert_eq!(finalized, None);
        let result = result.unwrap();
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
            Some(TurnFailureCategory::Validation)
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
            Some(TurnFailureCategory::Validation)
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
            ended_for_interaction: false,
            abort_reason: None,
            error: None,
            failure: None,
            budget_limit_kind: None,
            budget_usage: None,
            rollover_compacted: false,
            rollover_compaction_error: None,
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
