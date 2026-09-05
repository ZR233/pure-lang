use std::path::PathBuf;

use pl_model::model::ModelCapabilities;
use pl_protocol::{
    BudgetLimitKind, BudgetUsage, ErrorSeverity, ProviderFailureKind, PureError, RetryDisposition,
    TurnCancellationCause, TurnFailure, TurnFailureCategory, TurnOutcome, TurnRolloverOutcome,
};
use pl_trace::AgentEvent;

use crate::trace::TraceRecorder;
use crate::turn::{ToolExecutionMode, TurnOptions, TurnResult};

pub(super) fn provider_error_severity(
    active_subagent: Option<&crate::tool::SubagentContext>,
    error: &PureError,
) -> ErrorSeverity {
    let Some(failure) = error.provider_failure_ref() else {
        return ErrorSeverity::Fatal;
    };
    match (&failure.retry, failure.kind, active_subagent.is_some()) {
        (RetryDisposition::Retryable { .. }, _, false) => ErrorSeverity::Transient,
        (_, ProviderFailureKind::Capacity | ProviderFailureKind::Transport, _) => {
            ErrorSeverity::Recoverable
        }
        _ => ErrorSeverity::Fatal,
    }
}

pub(super) fn normalize_provider_error(
    active_subagent: Option<&crate::tool::SubagentContext>,
    error: PureError,
) -> (String, ErrorSeverity, TurnFailure) {
    if let PureError::Protocol(message) = error {
        return (
            message.clone(),
            ErrorSeverity::Fatal,
            TurnFailure::permanent(TurnFailureCategory::Protocol, message),
        );
    }
    let message = error.to_string();
    let provider_failure = error.provider_failure_ref().cloned();
    let (code, http_status) = error
        .transient_model_metadata()
        .map_or((None, None), |(code, status)| {
            (code.map(ToString::to_string), status)
        });
    let is_capacity = code.as_deref().is_some_and(provider_capacity_code)
        || matches!(http_status, Some(429 | 503));
    if active_subagent.is_some() && is_capacity {
        let message = PureError::ProviderCapacity { message }.to_string();
        return (
            message.clone(),
            ErrorSeverity::Recoverable,
            TurnFailure {
                category: TurnFailureCategory::ProviderCapacity,
                provider_kind: Some(ProviderFailureKind::Capacity),
                code,
                http_status,
                message,
                retry: RetryDisposition::Permanent,
            },
        );
    }
    let severity = provider_error_severity(active_subagent, &error);
    let retry = provider_failure
        .as_ref()
        .map_or(RetryDisposition::Permanent, |failure| failure.retry.clone());
    let category = if retry.is_retryable() && is_capacity {
        TurnFailureCategory::ProviderCapacity
    } else {
        TurnFailureCategory::Provider
    };
    (
        message.clone(),
        severity,
        TurnFailure {
            category,
            provider_kind: provider_failure.map(|failure| failure.kind).or({
                Some(match &error {
                    PureError::ConfigError(_) => ProviderFailureKind::Configuration,
                    PureError::LlmError(_) | PureError::HttpError(_) => {
                        ProviderFailureKind::Protocol
                    }
                    _ => ProviderFailureKind::Unknown,
                })
            }),
            code,
            http_status,
            message,
            retry,
        },
    )
}

fn provider_capacity_code(code: &str) -> bool {
    matches!(
        code,
        "server_is_overloaded" | "rate_limit_exceeded" | "websocket_connection_limit_reached"
    )
}

pub(super) fn should_request_parallel_tool_calls(
    capabilities: ModelCapabilities,
    options: &TurnOptions,
) -> bool {
    match options.tool_execution_mode {
        ToolExecutionMode::Sequential => false,
        ToolExecutionMode::Parallel => true,
        ToolExecutionMode::ModelDefault => capabilities.supports_parallel_tool_calls(),
    }
}

pub(super) fn is_cancelled(options: &TurnOptions) -> bool {
    options
        .cancellation_token
        .as_ref()
        .is_some_and(|token| token.is_cancelled())
}

pub(super) fn budget_limit_message(kind: BudgetLimitKind, usage: &BudgetUsage) -> String {
    format!(
        "budget limited by {} budget (modelSteps={}, toolCalls={}, waitCalls={}, elapsedMs={})",
        kind.as_str(),
        usage.model_steps,
        usage.tool_calls,
        usage.wait_calls,
        usage.elapsed_ms
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn interrupted_turn_result(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    content: String,
    reasoning_content: Option<String>,
    model: String,
    usage: pl_protocol::InferenceTokenUsage,
    session_message_count: usize,
    reason: String,
) -> TurnResult {
    recorder.cancel_open_items(turn_id, &reason);
    let outcome = TurnOutcome::cancelled(TurnCancellationCause::UserRequested);
    let item = recorder.terminal_turn_item(turn_id, &outcome);
    recorder.fail_item(item);
    recorder.broadcast(AgentEvent::TurnInterrupted { reason });
    recorder.broadcast(AgentEvent::Done);

    TurnResult {
        billing: pl_protocol::TurnBillingRecord::new(),
        content,
        reasoning_content,
        model,
        usage,
        last_context_tokens: None,
        context_compactions: Vec::new(),
        session_message_count,
        outcome,
        trace_events: recorder.drain(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn failed_turn_result(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    content: String,
    reasoning_content: Option<String>,
    model: String,
    usage: pl_protocol::InferenceTokenUsage,
    session_message_count: usize,
    error: String,
    severity: ErrorSeverity,
    failure: TurnFailure,
) -> TurnResult {
    failed_turn_result_with_abort_reason(
        recorder,
        turn_id,
        content,
        reasoning_content,
        model,
        usage,
        session_message_count,
        error,
        severity,
        failure,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn failed_turn_result_with_abort_reason(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    content: String,
    reasoning_content: Option<String>,
    model: String,
    usage: pl_protocol::InferenceTokenUsage,
    session_message_count: usize,
    error: String,
    severity: ErrorSeverity,
    mut failure: TurnFailure,
) -> TurnResult {
    failure.message = error.clone();
    recorder.fail_open_items(turn_id, &error);
    let outcome = TurnOutcome::failed(failure);
    let item = recorder.terminal_turn_item(turn_id, &outcome);
    recorder.fail_item(item);
    recorder.broadcast(AgentEvent::Error {
        message: error.clone(),
        severity,
    });
    recorder.broadcast(AgentEvent::Done);

    TurnResult {
        billing: pl_protocol::TurnBillingRecord::new(),
        content,
        reasoning_content,
        model,
        usage,
        last_context_tokens: None,
        context_compactions: Vec::new(),
        session_message_count,
        outcome,
        trace_events: recorder.drain(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn budget_limited_turn_result(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    content: String,
    reasoning_content: Option<String>,
    model: String,
    usage: pl_protocol::InferenceTokenUsage,
    session_message_count: usize,
    limit_kind: BudgetLimitKind,
    budget_usage: BudgetUsage,
    reason: String,
) -> TurnResult {
    recorder.cancel_open_items(turn_id, &reason);
    let outcome = TurnOutcome::budget_limited(
        pl_protocol::BudgetLimitSnapshot {
            kind: limit_kind,
            usage: budget_usage,
        },
        TurnRolloverOutcome::NotAttempted,
    );
    let item = recorder.terminal_turn_item(turn_id, &outcome);
    recorder.fail_item(item);
    recorder.broadcast(AgentEvent::TurnBudgetLimited {
        reason,
        limit_kind,
        usage: budget_usage,
    });
    recorder.broadcast(AgentEvent::Done);

    TurnResult {
        billing: pl_protocol::TurnBillingRecord::new(),
        content,
        reasoning_content,
        model,
        usage,
        last_context_tokens: None,
        context_compactions: Vec::new(),
        session_message_count,
        outcome,
        trace_events: recorder.drain(),
    }
}

pub(super) fn default_workspace_root() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

pub(super) fn looks_like_unexecuted_tool_call_text(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();

    trimmed.contains("<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}tool_calls>")
        || trimmed.contains("<\u{ff5c}\u{ff5c}DSML\u{ff5c}\u{ff5c}invoke name=")
        || lower.contains("<tool_call_>")
        || lower.contains("<tool_calls>")
        || looks_like_json_tool_calls_text(trimmed)
}

pub(super) fn looks_like_json_tool_calls_text(trimmed: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return false;
    };
    has_tool_calls_shape(&value)
}

pub(super) fn has_tool_calls_shape(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => map
            .get("tool_calls")
            .or_else(|| map.get("toolCalls"))
            .is_some_and(|tool_calls| {
                tool_calls
                    .as_array()
                    .is_some_and(|items| has_tool_call_entries(items))
            }),
        serde_json::Value::Array(items) => has_tool_call_entries(items),
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => false,
    }
}

pub(super) fn has_tool_call_entries(items: &[serde_json::Value]) -> bool {
    !items.is_empty()
        && items.iter().all(|item| {
            item.as_object().is_some_and(|entry| {
                entry.contains_key("name")
                    || entry.contains_key("function")
                    || entry.contains_key("arguments")
                    || entry.contains_key("input")
            })
        })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::SubagentContext;
    use pl_protocol::InferenceTokenUsage;

    #[test]
    fn root_provider_429_is_transient_and_subagent_429_is_provider_capacity() {
        let root_error = pl_protocol::PureError::transient_model_failure(
            "API error 429 Too Many Requests",
            None,
            Some("rate_limit_exceeded".to_string()),
            Some(429),
        );
        assert!(matches!(
            provider_error_severity(None, &root_error),
            ErrorSeverity::Transient
        ));

        let subagent = SubagentContext {
            id: "agent-1".to_string(),
            parent_id: None,
            agent_path: Some("/root/worker".to_string()),
            role: "executor".to_string(),
            task: "inspect worker".to_string(),
            depth: 1,
        };
        let (error, severity, failure) = normalize_provider_error(
            Some(&subagent),
            pl_protocol::PureError::transient_model_failure(
                "API error 429 Too Many Requests",
                None,
                Some("rate_limit_exceeded".to_string()),
                Some(429),
            ),
        );
        assert!(error.contains("provider capacity unavailable"));
        assert!(error.contains("API error 429 Too Many Requests"));
        assert!(matches!(severity, ErrorSeverity::Recoverable));
        assert_eq!(
            failure.category,
            pl_protocol::TurnFailureCategory::ProviderCapacity
        );
        assert!(!failure.retry.is_retryable());
        assert!(matches!(
            provider_error_severity(
                None,
                &pl_protocol::PureError::LlmError("API error 500".to_string())
            ),
            ErrorSeverity::Fatal
        ));
        assert!(matches!(
            provider_error_severity(
                None,
                &pl_protocol::PureError::transient_model_transport(
                    "Responses WebSocket stream failed"
                )
            ),
            ErrorSeverity::Transient
        ));
    }

    #[test]
    fn provider_error_text_never_controls_retry() {
        let (_, severity, failure) = normalize_provider_error(
            None,
            pl_protocol::PureError::LlmError(
                "API error 429 and timeout are display text only".to_string(),
            ),
        );

        assert_eq!(severity, ErrorSeverity::Fatal);
        assert_eq!(failure.category, pl_protocol::TurnFailureCategory::Provider);
        assert!(!failure.retry.is_retryable());
    }

    #[test]
    fn structured_http_overload_preserves_capacity_and_retry_semantics() {
        let (_, severity, failure) = normalize_provider_error(
            None,
            pl_protocol::PureError::transient_model_failure(
                "API error 503 Service Unavailable",
                Some(750),
                Some("server_is_overloaded".to_string()),
                Some(503),
            ),
        );

        assert_eq!(severity, ErrorSeverity::Transient);
        assert_eq!(
            failure.category,
            pl_protocol::TurnFailureCategory::ProviderCapacity
        );
        assert_eq!(failure.code.as_deref(), Some("server_is_overloaded"));
        assert_eq!(failure.http_status, Some(503));
        assert_eq!(failure.retry.retry_after_ms(), Some(750));
    }

    #[test]
    fn detects_unexecuted_tool_call_text() {
        assert!(looks_like_unexecuted_tool_call_text(
            "<｜｜DSML｜｜tool_calls>\n<｜｜DSML｜｜invoke name=\"spawn_agent\">"
        ));
        assert!(looks_like_unexecuted_tool_call_text(
            r#"{"tool_calls":[{"name":"spawn_agent"}]}"#
        ));
        assert!(!looks_like_unexecuted_tool_call_text(
            "源码中有 tool_calls 字段、name 字段和 subagent.rs 文件。"
        ));
        assert!(!looks_like_unexecuted_tool_call_text(
            r#"{"summary":"tool_calls and name are discussed in docs"}"#
        ));
        assert!(!looks_like_unexecuted_tool_call_text(
            "已完成探索，没有工具调用文本。"
        ));
    }

    #[test]
    fn failed_turn_result_preserves_error_message() {
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let mut recorder = TraceRecorder::new("session-1".to_string(), event_tx, 0);
        let turn_item = recorder.running_turn_item("turn-1");
        recorder.start_item(turn_item);
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TracePartStarted { item } if item.item_id() == "turn-1-turn"
        ));

        let result = failed_turn_result(
            &mut recorder,
            "turn-1",
            "partial summary".to_string(),
            None,
            "model-a".to_string(),
            InferenceTokenUsage::default(),
            3,
            "provider rejected request".to_string(),
            ErrorSeverity::Transient,
            pl_protocol::TurnFailure::permanent(
                pl_protocol::TurnFailureCategory::Provider,
                "provider rejected request",
            ),
        );

        assert_eq!(result.content, "partial summary");
        assert_eq!(
            result
                .outcome
                .failure()
                .map(|failure| failure.message.as_str()),
            Some("provider rejected request")
        );
        assert_eq!(
            result.outcome.failure().map(|failure| failure.category),
            Some(pl_protocol::TurnFailureCategory::Provider)
        );
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TracePartFailed { item } if item.item_id() == "turn-1-turn"
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::Error {
                severity: ErrorSeverity::Transient,
                ..
            }
        ));
        assert!(matches!(event_rx.try_recv().unwrap(), AgentEvent::Done));
    }
}
