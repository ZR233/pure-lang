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

    match error {
        PureError::TransientModelTransport {
            message,
            retry_after_ms,
            ..
        } => {
            assert_eq!(message, "provider stream ended before completion");
            assert_eq!(retry_after_ms, None);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn stream_accumulator_rejects_events_after_completed() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(None);

    apply_completed(&mut accumulator, &event_tx);
    let error = accumulator
        .apply(
            StreamEvent::ReasoningRawDelta {
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
fn stream_accumulator_keeps_raw_reasoning_out_of_trace() {
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
            StreamEvent::ReasoningRawDelta {
                id: "thinking".to_string(),
                content_index: 0,
                delta: "raw only".to_string(),
            },
            &event_tx,
        )
        .unwrap();
    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.reasoning_content.as_deref(), Some("raw only"));
    assert!(response.trace_events.iter().all(|event| !matches!(
        &event.kind,
        TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartFailed { item, .. }
            if item.kind == TracePartKind::Thinking
    )));
}

#[test]
fn stream_accumulator_rejects_tool_delta_without_name() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(None);

    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: Some("chat_tool_call:0".to_string()),
                item_id: "call_1".to_string(),
                call_id: None,
                name: None,
                payload_delta: ToolCallDeltaPayload::FunctionArguments(
                    "{\"path\":\"Cargo.toml\"}".to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    let error = accumulator
        .apply(StreamEvent::Completed { response_id: None }, &event_tx)
        .unwrap_err();

    match error {
        PureError::LlmError(message) => {
            assert_eq!(message, "provider emitted tool call without name");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
