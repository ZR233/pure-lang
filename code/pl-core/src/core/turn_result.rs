use std::path::PathBuf;

use pl_model::ModelCapabilities;
use pl_protocol::{BudgetLimitKind, BudgetUsage, ErrorSeverity, PureError};
use pl_trace::{AgentEvent, TracePartStatus};

use crate::trace::TraceRecorder;
use crate::turn::{
    CompileMode, ToolExecutionMode, TurnAbortReason, TurnOptions, TurnResult, TurnResultStatus,
};
#[cfg(test)]
use crate::turn::{ToolEffect, TurnExecutionProfile};

pub(super) fn provider_error_severity(
    active_subagent: Option<&crate::tool::SubagentContext>,
    error: &str,
) -> ErrorSeverity {
    if active_subagent.is_none() && crate::provider_error::is_provider_429_error(error) {
        ErrorSeverity::Transient
    } else {
        ErrorSeverity::Recoverable
    }
}

pub(super) fn normalize_provider_error(
    active_subagent: Option<&crate::tool::SubagentContext>,
    error: String,
) -> (String, ErrorSeverity) {
    if active_subagent.is_some() && crate::provider_error::is_provider_429_error(&error) {
        return (
            PureError::ProviderCapacity { message: error }.to_string(),
            ErrorSeverity::Recoverable,
        );
    }
    let severity = provider_error_severity(active_subagent, &error);
    (error, severity)
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

#[cfg(test)]
pub(super) fn tool_allowed_in_mode(mode: CompileMode, name: &str) -> bool {
    TurnExecutionProfile::root(mode).allows_tool(name, ToolEffect::for_builtin_name(name))
}

pub(super) fn is_cancelled(options: &TurnOptions) -> bool {
    options
        .cancellation_token
        .as_ref()
        .is_some_and(|token| token.is_cancelled())
}

pub(super) fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
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
    mode: CompileMode,
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
        mode,
        session_message_count,
        status: TurnResultStatus::Aborted,
        abort_reason: Some(crate::turn::TurnAbortReason::Interrupted),
        error: None,
        budget_limit_kind: None,
        budget_usage: None,
        trace_events: recorder.drain(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn failed_turn_result(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    mode: CompileMode,
    content: String,
    reasoning_content: Option<String>,
    model: String,
    usage: pl_model::TokenUsage,
    session_message_count: usize,
    error: String,
    severity: ErrorSeverity,
) -> TurnResult {
    failed_turn_result_with_abort_reason(
        recorder,
        turn_id,
        mode,
        content,
        reasoning_content,
        model,
        usage,
        session_message_count,
        error,
        severity,
        TurnAbortReason::ProviderError,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn failed_turn_result_with_abort_reason(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    mode: CompileMode,
    content: String,
    reasoning_content: Option<String>,
    model: String,
    mut usage: pl_model::TokenUsage,
    session_message_count: usize,
    error: String,
    severity: ErrorSeverity,
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
        mode,
        session_message_count,
        status: TurnResultStatus::Errored,
        abort_reason: Some(abort_reason),
        error: Some(error),
        budget_limit_kind: None,
        budget_usage: None,
        trace_events: recorder.drain(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn budget_limited_turn_result(
    recorder: &mut TraceRecorder,
    turn_id: &str,
    mode: CompileMode,
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
        mode,
        session_message_count,
        status: TurnResultStatus::Aborted,
        abort_reason: Some(crate::turn::TurnAbortReason::BudgetLimited),
        error: None,
        budget_limit_kind: Some(limit_kind),
        budget_usage: Some(budget_usage),
        trace_events: recorder.drain(),
    }
}

pub(crate) fn compact_text(text: &str) -> String {
    const MAX_CHARS: usize = 240;
    let trimmed = text.trim();
    let mut result = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if index >= MAX_CHARS {
            result.push_str("...");
            return result;
        }
        result.push(ch);
    }
    result
}

pub(super) fn default_workspace_root() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

pub(super) fn prompt_requires_subagent_dispatch(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    let lower_without_file_mentions = lower
        .replace("subagent.rs", "")
        .replace("subagent.md", "")
        .replace("subagent.toml", "");
    let mentions_subagent = lower_without_file_mentions.contains("subagent")
        || prompt.contains("子代理")
        || prompt.contains("分代理");
    let requests_partition =
        lower.contains("crate") || prompt.contains("每个") || prompt.contains("分别");
    mentions_subagent && requests_partition
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
