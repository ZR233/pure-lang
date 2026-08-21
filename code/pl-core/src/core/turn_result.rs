use std::path::PathBuf;

use pl_model::ModelCapabilities;
use pl_protocol::{
    BudgetLimitKind, BudgetUsage, ErrorSeverity, ProviderFailureKind, PureError, RetryDisposition,
    TurnFailure, TurnFailureCategory,
};
use pl_trace::{AgentEvent, TracePartStatus};

use crate::trace::TraceRecorder;
use crate::turn::{ToolExecutionMode, TurnAbortReason, TurnOptions, TurnResult, TurnResultStatus};

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
    mut usage: pl_model::TokenUsage,
    session_message_count: usize,
    reason: String,
) -> TurnResult {
    usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;
    recorder.ensure_assistant_text_item(turn_id, &content);
    let mut item = recorder.turn_item(turn_id, TracePartStatus::Interrupted);
    item.content = content.clone();
    recorder.fail_item(item, reason.clone());
    recorder.broadcast(AgentEvent::TurnInterrupted { reason });
    recorder.broadcast(AgentEvent::Done);

    TurnResult {
        content,
        reasoning_content,
        model,
        usage,
        last_context_tokens: None,
        context_compactions: Vec::new(),
        session_message_count,
        status: TurnResultStatus::Aborted,
        ended_for_interaction: false,
        abort_reason: Some(crate::turn::TurnAbortReason::Interrupted),
        error: None,
        failure: None,
        budget_limit_kind: None,
        budget_usage: None,
        rollover_compacted: false,
        rollover_compaction_error: None,
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
    usage: pl_model::TokenUsage,
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
        TurnAbortReason::ProviderError,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn failed_turn_result_with_abort_reason(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    content: String,
    reasoning_content: Option<String>,
    model: String,
    mut usage: pl_model::TokenUsage,
    session_message_count: usize,
    error: String,
    severity: ErrorSeverity,
    failure: TurnFailure,
    abort_reason: TurnAbortReason,
) -> TurnResult {
    usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;
    recorder.ensure_assistant_text_item(turn_id, &content);
    let mut item = recorder.turn_item(turn_id, TracePartStatus::Failed);
    item.content = content.clone();
    recorder.fail_item(item, error.clone());
    recorder.broadcast(AgentEvent::Error {
        message: error.clone(),
        severity,
    });
    recorder.broadcast(AgentEvent::Done);

    TurnResult {
        content,
        reasoning_content,
        model,
        usage,
        last_context_tokens: None,
        context_compactions: Vec::new(),
        session_message_count,
        status: TurnResultStatus::Errored,
        ended_for_interaction: false,
        abort_reason: Some(abort_reason),
        error: Some(error),
        failure: Some(failure),
        budget_limit_kind: None,
        budget_usage: None,
        rollover_compacted: false,
        rollover_compaction_error: None,
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
    mut usage: pl_model::TokenUsage,
    session_message_count: usize,
    limit_kind: BudgetLimitKind,
    budget_usage: BudgetUsage,
    reason: String,
) -> TurnResult {
    usage.total_tokens = usage.prompt_tokens + usage.completion_tokens;
    recorder.ensure_assistant_text_item(turn_id, &content);
    let mut item = recorder.turn_item(turn_id, TracePartStatus::BudgetLimited);
    item.content = content.clone();
    recorder.fail_item(item, reason.clone());
    recorder.broadcast(AgentEvent::TurnBudgetLimited {
        reason,
        limit_kind,
        usage: budget_usage,
    });
    recorder.broadcast(AgentEvent::Done);

    TurnResult {
        content,
        reasoning_content,
        model,
        usage,
        last_context_tokens: None,
        context_compactions: Vec::new(),
        session_message_count,
        status: TurnResultStatus::Aborted,
        ended_for_interaction: false,
        abort_reason: Some(crate::turn::TurnAbortReason::BudgetLimited),
        error: None,
        failure: None,
        budget_limit_kind: Some(limit_kind),
        budget_usage: Some(budget_usage),
        rollover_compacted: false,
        rollover_compaction_error: None,
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
