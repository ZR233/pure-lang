use pl_model::TokenUsage;
use pl_trace::TraceEventKind;

use crate::TurnResult;
use crate::agent_runtime::state::unix_timestamp;
use crate::agent_runtime::*;

#[derive(Debug)]
pub(crate) enum TurnWorkerOutcome {
    Returned(Box<TurnResult>),
    Failed { error: String },
}

impl TurnWorkerOutcome {
    pub(crate) fn returned(&self) -> Option<&TurnResult> {
        match self {
            Self::Returned(result) => Some(result),
            Self::Failed { .. } => None,
        }
    }
}

impl From<std::result::Result<TurnResult, String>> for TurnWorkerOutcome {
    fn from(result: std::result::Result<TurnResult, String>) -> Self {
        match result {
            Ok(result) => Self::Returned(Box::new(result)),
            Err(error) => Self::Failed { error },
        }
    }
}

#[derive(Debug)]
pub(crate) enum TurnExecutionTerminal {
    Returned(TurnResult),
    CancelledAfterReturn {
        cause: pl_protocol::TurnCancellationCause,
        result: TurnResult,
    },
    CancelledBeforeReturn {
        cause: pl_protocol::TurnCancellationCause,
    },
    WorkerFailed {
        error: String,
    },
}

impl From<TurnWorkerOutcome> for TurnExecutionTerminal {
    fn from(outcome: TurnWorkerOutcome) -> Self {
        match outcome {
            TurnWorkerOutcome::Returned(result) => Self::Returned(*result),
            TurnWorkerOutcome::Failed { error } => Self::WorkerFailed { error },
        }
    }
}

#[derive(Debug)]
pub(crate) enum RetainedTurnResult {
    Present(Box<TurnResult>),
    Absent,
}

impl RetainedTurnResult {
    pub(crate) fn as_ref(&self) -> Option<&TurnResult> {
        match self {
            Self::Present(result) => Some(result),
            Self::Absent => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct TurnFinalization {
    pub(crate) outcome: AgentTurnOutcome,
    pub(crate) retained_result: RetainedTurnResult,
}

pub(crate) fn turn_outcome(
    turn_id: TurnId,
    thread_id: ThreadId,
    terminal: TurnExecutionTerminal,
    started_at: Option<i64>,
) -> TurnFinalization {
    let (outcome, usage, retained_result) = match terminal {
        TurnExecutionTerminal::CancelledAfterReturn { cause, result } => (
            pl_protocol::TurnOutcome::cancelled(cause),
            result.usage.clone(),
            RetainedTurnResult::Present(Box::new(result)),
        ),
        TurnExecutionTerminal::Returned(result) => (
            result.outcome.clone(),
            result.usage.clone(),
            RetainedTurnResult::Present(Box::new(result)),
        ),
        TurnExecutionTerminal::CancelledBeforeReturn { cause } => (
            pl_protocol::TurnOutcome::cancelled(cause),
            TokenUsage::default(),
            RetainedTurnResult::Absent,
        ),
        TurnExecutionTerminal::WorkerFailed { error } => (
            pl_protocol::TurnOutcome::failed(pl_protocol::TurnFailure::permanent(
                pl_protocol::TurnFailureCategory::Internal,
                error,
            )),
            TokenUsage::default(),
            RetainedTurnResult::Absent,
        ),
    };
    TurnFinalization {
        outcome: AgentTurnOutcome {
            turn_id,
            thread_id,
            outcome,
            usage,
            started_at,
            finished_at: unix_timestamp(),
        },
        retained_result,
    }
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
) {
    let Ok(result) = result else {
        return;
    };
    if !result.outcome.is_completed() {
        return;
    }
    if result.outcome.is_interaction_boundary() {
        return;
    }
    let TurnFinalizationPolicy::RequiredTool { name } = &policy.finalization else {
        return;
    };
    let latest_tool = result
        .trace_events
        .iter()
        .rev()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item }
                if item
                    .tool()
                    .is_some_and(|tool| tool.invocation().name() == name) =>
            {
                Some(Ok(()))
            }
            TraceEventKind::TracePartFailed { item }
                if item
                    .tool()
                    .is_some_and(|tool| tool.invocation().name() == name) =>
            {
                Some(Err(item
                    .failure()
                    .unwrap_or("finalization tool did not succeed")
                    .to_string()))
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        });
    if matches!(latest_tool, Some(Ok(()))) {
        return;
    }
    let message = latest_tool
        .and_then(Result::err)
        .unwrap_or_else(|| format!("turn must finalize with tool `{name}`"));
    result.outcome = pl_protocol::TurnOutcome::failed(pl_protocol::TurnFailure::permanent(
        pl_protocol::TurnFailureCategory::Validation,
        message,
    ));
}

#[cfg(test)]
mod tests {
    use pl_protocol::{TurnCompletion, TurnFailureCategory, TurnOutcome};
    use pl_trace::TraceEvent;

    use super::*;
    use crate::TraceRecorder;

    #[test]
    fn required_tool_finalization_accepts_completed_tool() {
        let mut result = Ok(completed_result(vec![tool_event(
            "report_completion",
            ToolTraceOutcome::Completed,
        )]));

        enforce_finalization(&mut result, &required_tool_policy("report_completion"));

        let result = result.unwrap();
        assert!(matches!(result.outcome, TurnOutcome::Completed(_)));
    }

    #[test]
    fn required_tool_finalization_accepts_durable_interaction_boundary() {
        let mut completed = completed_result(Vec::new());
        completed.outcome = TurnOutcome::completed(TurnCompletion::InteractionRequested);
        let mut result = Ok(completed);

        enforce_finalization(&mut result, &required_tool_policy("plan_exit"));

        let result = result.unwrap();
        assert!(result.outcome.is_interaction_boundary());
    }

    #[test]
    fn required_tool_finalization_preserves_matching_tool_failure() {
        let mut result = Ok(completed_result(vec![tool_event(
            "report_completion",
            ToolTraceOutcome::Failed("completion scope is not ready for review"),
        )]));

        enforce_finalization(&mut result, &required_tool_policy("report_completion"));

        let result = result.unwrap();
        assert_eq!(
            result
                .outcome
                .failure()
                .map(|failure| failure.message.as_str()),
            Some("completion scope is not ready for review")
        );
        assert_eq!(
            result.outcome.failure().map(|failure| failure.category),
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

        enforce_finalization(&mut result, &required_tool_policy("report_completion"));

        assert_eq!(
            result
                .unwrap()
                .outcome
                .failure()
                .map(|failure| failure.message.as_str()),
            Some("latest completion failure")
        );
    }

    #[test]
    fn required_tool_finalization_ignores_other_tool_failure() {
        let mut result = Ok(completed_result(vec![tool_event(
            "exec",
            ToolTraceOutcome::Failed("exec failed"),
        )]));

        enforce_finalization(&mut result, &required_tool_policy("report_completion"));

        let result = result.unwrap();
        assert_eq!(
            result
                .outcome
                .failure()
                .map(|failure| failure.message.as_str()),
            Some("turn must finalize with tool `report_completion`")
        );
        assert_eq!(
            result.outcome.failure().map(|failure| failure.category),
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

        enforce_finalization(&mut result, &required_tool_policy("report_completion"));

        assert!(result.unwrap().is_completed());
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

        enforce_finalization(&mut result, &required_tool_policy("report_completion"));

        let result = result.unwrap();
        assert_eq!(
            result
                .outcome
                .failure()
                .map(|failure| failure.message.as_str()),
            Some("latest completion failure")
        );
        assert_eq!(
            result.outcome.failure().map(|failure| failure.category),
            Some(TurnFailureCategory::Validation)
        );
    }

    #[derive(Clone, Copy)]
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
            outcome: TurnOutcome::completed(TurnCompletion::Normal),
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
        let action = match outcome {
            ToolTraceOutcome::Completed => {
                pl_trace::TracePartAction::Complete(pl_trace::TracePartCompletion::Tool {
                    output: pl_trace::TraceToolOutput::new(String::new()),
                })
            }
            ToolTraceOutcome::Failed(error) => pl_trace::TracePartAction::FailTool {
                failure: pl_trace::TraceToolFailure::new(
                    pl_trace::TraceToolFailureKind::Execution,
                    error.to_string(),
                ),
                output: None,
            },
        };
        let command = pl_trace::TracePartCommand {
            item_id: item.item_id().to_string(),
            expected_revision: item.revision(),
            updated_at: crate::time::unix_seconds(),
            action,
        };
        item.apply(command).expect("valid tool terminal state");
        match outcome {
            ToolTraceOutcome::Completed => recorder.complete_item(item),
            ToolTraceOutcome::Failed(_) => recorder.fail_item(item),
        }
        recorder.drain().pop().unwrap()
    }
}
