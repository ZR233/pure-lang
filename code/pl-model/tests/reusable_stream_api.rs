use pl_model::{
    CompletionBlockContent, CompletionBlockKind, CompletionEventStream,
    CompletionStreamAccumulator, CompletionStreamEvent, FinishReason, TokenUsage,
    ToolInputDeltaPayload, ToolInputPayloadKind,
};
use pl_trace::TraceTextChannel;
use pretty_assertions::assert_eq;

fn assert_public_event_stream_type(_: CompletionEventStream) {}

#[test]
fn public_stream_events_accumulate_completion_response() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = CompletionStreamAccumulator::new(None);

    accumulator
        .apply(
            CompletionStreamEvent::ResponseStarted {
                response_id: Some("resp_1".to_string()),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            CompletionStreamEvent::text_started("msg_1".to_string(), TraceTextChannel::Final),
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            CompletionStreamEvent::text_delta(
                "msg_1".to_string(),
                TraceTextChannel::Final,
                "完成".to_string(),
            ),
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            CompletionStreamEvent::BlockClosed {
                id: "msg_1".to_string(),
                kind: CompletionBlockKind::Text {
                    channel: TraceTextChannel::Final,
                },
                authoritative_content: Some(CompletionBlockContent::Text("完成。".to_string())),
                provider_metadata: None,
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            CompletionStreamEvent::ReasoningRawDelta {
                id: "reasoning_1".to_string(),
                content_index: 0,
                delta: "先分析需求。".to_string(),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            CompletionStreamEvent::ToolInputStarted {
                stream_id: Some("tool_stream_1".to_string()),
                item_id: "tool_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("read_file".to_string()),
                payload_kind: ToolInputPayloadKind::FunctionArguments,
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            CompletionStreamEvent::ToolInputDelta {
                stream_id: Some("tool_stream_1".to_string()),
                item_id: "tool_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("read_file".to_string()),
                payload_delta: ToolInputDeltaPayload::FunctionArguments(
                    "{\"path\":\"Cargo.toml\"}".to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            CompletionStreamEvent::ToolCallReady {
                stream_id: Some("tool_stream_1".to_string()),
                item_id: "tool_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("read_file".to_string()),
                payload: None,
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            CompletionStreamEvent::Usage(TokenUsage {
                prompt_tokens: 3,
                completion_tokens: 5,
                total_tokens: 8,
                cached_prompt_tokens: 2,
                reasoning_tokens: 0,
            }),
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            CompletionStreamEvent::Completed {
                response_id: Some("resp_1".to_string()),
            },
            &event_tx,
        )
        .unwrap();

    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content.as_deref(), Some("完成。"));
    assert_eq!(response.reasoning_content.as_deref(), Some("先分析需求。"));
    assert_eq!(response.finish_reason, FinishReason::ToolCalls);
    assert_eq!(response.usage.cached_prompt_tokens, 2);
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "tool_1");
    assert_eq!(response.tool_calls[0].call_id.as_deref(), Some("call_1"));
    assert_eq!(response.tool_calls[0].name, "read_file");
}

#[test]
fn completion_event_stream_is_public_api_type() {
    let stream = futures::stream::empty::<pl_protocol::Result<CompletionStreamEvent>>();

    assert_public_event_stream_type(Box::pin(stream));
}
