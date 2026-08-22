use super::super::*;
use pretty_assertions::assert_eq;

#[test]
fn stream_accumulator_requires_completed_event() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(None);

    accumulator
        .apply(final_started("final"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_delta("final", "partial"), &event_tx)
        .unwrap();

    let error = accumulator.finish(&event_tx).unwrap_err();

    let failure = error
        .provider_failure_ref()
        .expect("typed provider failure");
    assert_eq!(failure.kind, pl_protocol::ProviderFailureKind::Transport);
    assert_eq!(failure.message, "provider stream ended before completion");
    assert_eq!(failure.retry.retry_after_ms(), None);
}

#[test]
fn stream_accumulator_rejects_events_after_completed() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(None);

    apply_completed(&mut accumulator, &event_tx);
    let error = accumulator
        .apply(
            ModelStreamEvent::ReasoningRawDelta {
                id: "thinking".to_string(),
                content_index: 0,
                delta: "late".to_string(),
            },
            &event_tx,
        )
        .unwrap_err();

    match error {
        PureError::LlmError(message) => {
            assert_eq!(message, "provider stream emitted event after completion");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn stream_accumulator_projects_raw_reasoning_into_thinking_trace() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(
            ModelStreamEvent::ReasoningRawDelta {
                id: "thinking".to_string(),
                content_index: 0,
                delta: "raw only".to_string(),
            },
            &event_tx,
        )
        .unwrap();
    apply_completed(&mut accumulator, &event_tx);
    let response = finish_with_trace(accumulator, &event_tx).unwrap();

    assert_eq!(response.reasoning_content.as_deref(), Some("raw only"));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.kind() == TracePartKind::Thinking && trace_part_text(item) == "raw only"
    )));
}

#[test]
fn stream_accumulator_rejects_tool_delta_without_name() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(None);

    accumulator
        .apply(
            ModelStreamEvent::ToolInputDelta {
                stream_id: Some("chat_tool_call:0".to_string()),
                item_id: "call_1".to_string(),
                call_id: None,
                name: None,
                payload_delta: ToolInputDeltaPayload::FunctionArguments(
                    "{\"path\":\"Cargo.toml\"}".to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    let error = accumulator
        .apply(ModelStreamEvent::Completed { response_id: None }, &event_tx)
        .unwrap_err();

    match error {
        PureError::LlmError(message) => {
            assert_eq!(message, "provider emitted tool call without name");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
