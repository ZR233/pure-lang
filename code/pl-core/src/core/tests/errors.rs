use super::*;
use pretty_assertions::assert_eq;

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
            &pl_protocol::PureError::transient_model_transport("Responses WebSocket stream failed")
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

    let result = failed_turn_result(
        &mut recorder,
        "turn-1",
        "partial summary".to_string(),
        None,
        "model-a".to_string(),
        TokenUsage::default(),
        3,
        "provider rejected request".to_string(),
        ErrorSeverity::Transient,
        pl_protocol::TurnFailure::permanent(
            pl_protocol::TurnFailureCategory::Provider,
            "provider rejected request",
        ),
    );

    assert_eq!(result.status, TurnResultStatus::Errored);
    assert_eq!(
        result.abort_reason,
        Some(crate::turn::TurnAbortReason::ProviderError),
    );
    assert_eq!(result.content, "partial summary");
    assert_eq!(result.error.as_deref(), Some("provider rejected request"));
    assert_eq!(
        result.failure.as_ref().map(|failure| failure.category),
        Some(pl_protocol::TurnFailureCategory::Provider)
    );
    assert!(matches!(
        event_rx.try_recv().unwrap(),
        AgentEvent::TracePartStarted { item }
            if item.item_id == "turn-1-assistant"
                && item.text_channel == Some(TraceTextChannel::Final)
                && item.content == "partial summary"
    ));
    assert!(matches!(
        event_rx.try_recv().unwrap(),
        AgentEvent::TracePartCompleted { item, .. }
            if item.item_id == "turn-1-assistant"
                && item.text_channel == Some(TraceTextChannel::Final)
                && item.content == "partial summary"
    ));
    assert!(matches!(
        event_rx.try_recv().unwrap(),
        AgentEvent::TracePartFailed { item, .. } if item.item_id == "turn-1-turn"
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
