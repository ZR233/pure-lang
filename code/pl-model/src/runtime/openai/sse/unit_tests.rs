use pretty_assertions::assert_eq;

use crate::completion::stream::StreamCompletionAccumulator;
use crate::completion::stream::event::{
    ModelBlockContent, ModelBlockField, ModelBlockKind, ToolInputPayloadKind,
};
use crate::{CompletionTraceContext, ToolCallPayload};

use super::*;

fn single_event(event: &SseStreamEvent) -> Option<ModelStreamEvent> {
    let events = process_sse_events(event);
    assert!(events.len() <= 1, "expected at most one event: {events:?}");
    events.into_iter().next()
}

fn chat_event(delta: serde_json::Value) -> SseStreamEvent {
    serde_json::from_value(serde_json::json!({
        "choices": [{
            "delta": delta,
            "finish_reason": null
        }]
    }))
    .unwrap()
}

#[test]
fn process_chat_reasoning_content_as_thinking_delta() {
    let event = chat_event(serde_json::json!({
        "reasoning_content": "先比较整数位。"
    }));

    match single_event(&event) {
        Some(ModelStreamEvent::ReasoningRawDelta { delta, .. }) => {
            assert_eq!(delta, "先比较整数位。");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn process_chat_content_as_output_text_delta() {
    let event = chat_event(serde_json::json!({
        "content": "9.11 更大。"
    }));

    match single_event(&event) {
        Some(ModelStreamEvent::BlockDelta {
            field: ModelBlockField::Text,
            delta,
            ..
        }) => {
            assert_eq!(delta, "9.11 更大。");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn process_chat_reasoning_and_content_from_same_chunk() {
    let event = chat_event(serde_json::json!({
        "reasoning_content": "先比较整数位。",
        "content": "<final>9.11 更大。</final>"
    }));

    match process_sse_events(&event).as_slice() {
        [
            ModelStreamEvent::ReasoningRawDelta {
                delta: reasoning, ..
            },
            ModelStreamEvent::BlockDelta {
                field: ModelBlockField::Text,
                delta: content,
                ..
            },
        ] => {
            assert_eq!(reasoning, "先比较整数位。");
            assert_eq!(content, "<final>9.11 更大。</final>");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn process_chat_completed_reads_deepseek_cached_token_aliases() {
    for cached_usage in [
        serde_json::json!({"prompt_cache_hit_tokens": 35}),
        serde_json::json!({"cached_prompt_tokens": 35}),
        serde_json::json!({"prompt_tokens_details": {"cached_tokens": 35}}),
    ] {
        let mut usage = serde_json::json!({
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120
        });
        usage.as_object_mut().unwrap().extend(
            cached_usage
                .as_object()
                .unwrap()
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        let event: SseStreamEvent = serde_json::from_value(serde_json::json!({
            "choices": [{
                "delta": {},
                "finish_reason": "stop"
            }],
            "usage": usage
        }))
        .unwrap();

        match process_sse_events(&event).as_slice() {
            [
                ModelStreamEvent::Usage(usage),
                ModelStreamEvent::Completed { response_id: None },
            ] => {
                assert_eq!(usage.cached_prompt_tokens, 35);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}

#[test]
fn process_chat_completed_reads_responses_style_token_usage() {
    let event: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "choices": [{
            "delta": {},
            "finish_reason": "stop"
        }],
        "usage": {
            "input_tokens": 100,
            "output_tokens": 20,
            "total_tokens": 120,
            "input_tokens_details": {
                "cached_tokens": 35,
                "cache_write_tokens": 11
            },
            "output_tokens_details": {
                "reasoning_tokens": 8
            }
        }
    }))
    .unwrap();

    match process_sse_events(&event).as_slice() {
        [
            ModelStreamEvent::Usage(usage),
            ModelStreamEvent::Completed { response_id: None },
        ] => {
            assert_eq!(usage.prompt_tokens, 100);
            assert_eq!(usage.completion_tokens, 20);
            assert_eq!(usage.total_tokens, 120);
            assert_eq!(usage.cached_prompt_tokens, 35);
            assert_eq!(usage.cache_write_tokens, 11);
            assert_eq!(usage.reasoning_tokens, 8);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn process_responses_completed_reads_cache_write_tokens() {
    let event: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.completed",
        "response": {
            "id": "resp_1",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 20,
                "total_tokens": 120,
                "input_tokens_details": {
                    "cached_tokens": 40,
                    "cache_write_tokens": 15
                }
            }
        }
    }))
    .unwrap();

    match process_sse_events(&event).as_slice() {
        [
            ModelStreamEvent::Usage(usage),
            ModelStreamEvent::Completed { response_id },
        ] => {
            assert_eq!(usage.cached_prompt_tokens, 40);
            assert_eq!(usage.cache_write_tokens, 15);
            assert_eq!(response_id.as_deref(), Some("resp_1"));
        }
        other => panic!("unexpected events: {other:?}"),
    }
}

#[test]
fn process_responses_marks_summary_and_raw_reasoning() {
    let summary: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.reasoning_summary_text.delta",
        "item_id": "rs_1",
        "summary_index": 1,
        "delta": "摘要"
    }))
    .unwrap();
    let raw: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.reasoning_text.delta",
        "item_id": "rt_1",
        "content_index": 2,
        "delta": "内部推理"
    }))
    .unwrap();

    match single_event(&summary) {
        Some(ModelStreamEvent::BlockDelta {
            id,
            kind: ModelBlockKind::ReasoningSummary,
            field: ModelBlockField::ReasoningSummary,
            section_index,
            delta,
        }) => {
            assert_eq!(id, "rs_1");
            assert_eq!(section_index, Some(1));
            assert_eq!(delta, "摘要");
        }
        other => panic!("unexpected event: {other:?}"),
    }
    match single_event(&raw) {
        Some(ModelStreamEvent::ReasoningRawDelta {
            id,
            content_index,
            delta,
        }) => {
            assert_eq!(id, "rt_1");
            assert_eq!(content_index, 2);
            assert_eq!(delta, "内部推理");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn responses_decoder_preserves_native_text_phase_and_completed_text() {
    let mut decoder = OpenAiStreamDecoder::new(true);
    let commentary_added: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_item.added",
        "item": {
            "id": "msg_progress",
            "type": "message",
            "role": "assistant",
            "phase": "commentary",
            "content": []
        }
    }))
    .unwrap();
    match decoder.decode(&commentary_added).as_slice() {
        [
            ModelStreamEvent::BlockOpened {
                id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Commentary,
                    },
                ..
            },
        ] => {
            assert_eq!(id, "msg_progress");
        }
        other => panic!("unexpected events: {other:?}"),
    }

    let commentary_delta: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_text.delta",
        "item_id": "msg_progress",
        "delta": "正在检查。"
    }))
    .unwrap();
    match decoder.decode(&commentary_delta).as_slice() {
        [
            ModelStreamEvent::BlockDelta {
                id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Commentary,
                    },
                field: ModelBlockField::Text,
                delta,
                ..
            },
        ] => {
            assert_eq!(id, "msg_progress");
            assert_eq!(delta, "正在检查。");
        }
        other => panic!("unexpected events: {other:?}"),
    }

    let final_done: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "id": "msg_final",
            "type": "message",
            "role": "assistant",
            "phase": "final_answer",
            "content": [
                {"type": "output_text", "text": "完成。"}
            ]
        }
    }))
    .unwrap();
    match decoder.decode(&final_done).as_slice() {
        [
            ModelStreamEvent::BlockOpened {
                id: opened_id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                ..
            },
            ModelStreamEvent::BlockClosed {
                id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                authoritative_content: Some(ModelBlockContent::Text(authoritative_text)),
                ..
            },
        ] => {
            assert_eq!(opened_id, "msg_final");
            assert_eq!(id, "msg_final");
            assert_eq!(authoritative_text, "完成。");
        }
        other => panic!("unexpected events: {other:?}"),
    }
}

#[test]
fn responses_decoder_tracks_reasoning_summary_lifecycle() {
    let mut decoder = OpenAiStreamDecoder::new(true);
    let reasoning_added: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_item.added",
        "item": {
            "id": "rs_1",
            "type": "reasoning",
            "summary": []
        }
    }))
    .unwrap();
    match decoder.decode(&reasoning_added).as_slice() {
        [
            ModelStreamEvent::BlockOpened {
                id,
                kind: ModelBlockKind::ReasoningSummary,
                ..
            },
        ] => {
            assert_eq!(id, "rs_1");
        }
        other => panic!("unexpected events: {other:?}"),
    }

    let summary_delta: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.reasoning_summary_text.delta",
        "item_id": "rs_1",
        "summary_index": 0,
        "delta": "先检查输入。"
    }))
    .unwrap();
    match decoder.decode(&summary_delta).as_slice() {
        [
            ModelStreamEvent::BlockDelta {
                id,
                kind: ModelBlockKind::ReasoningSummary,
                field: ModelBlockField::ReasoningSummary,
                section_index: Some(0),
                delta,
            },
        ] => {
            assert_eq!(id, "rs_1");
            assert_eq!(delta, "先检查输入。");
        }
        other => panic!("unexpected events: {other:?}"),
    }

    let reasoning_done: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_item.done",
        "item": {
            "id": "rs_1",
            "type": "reasoning",
            "summary": [
                {"type": "summary_text", "text": "最终摘要。"}
            ]
        }
    }))
    .unwrap();
    match decoder.decode(&reasoning_done).as_slice() {
        [
            ModelStreamEvent::BlockClosed {
                id,
                kind: ModelBlockKind::ReasoningSummary,
                authoritative_content: Some(ModelBlockContent::ReasoningSummary(summary)),
                ..
            },
            ModelStreamEvent::ResponsesContextItem { item },
        ] => {
            assert_eq!(id, "rs_1");
            assert_eq!(summary, &vec!["最终摘要。".to_string()]);
            assert_eq!(item.kind, pl_protocol::ResponsesContextItemKind::Reasoning);
            assert_eq!(item.value["id"], "rs_1");
        }
        other => panic!("unexpected events: {other:?}"),
    }
}

#[test]
fn responses_decoder_closes_content_at_tool_boundary_once() {
    let mut decoder = OpenAiStreamDecoder::new(true);
    let reasoning_delta: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.reasoning_summary_text.delta",
        "item_id": "thinking",
        "summary_index": 0,
        "delta": "before tool"
    }))
    .unwrap();
    assert_eq!(decoder.decode(&reasoning_delta).len(), 2);

    let text_delta: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_text.delta",
        "item_id": "msg_1",
        "delta": "before "
    }))
    .unwrap();
    assert_eq!(decoder.decode(&text_delta).len(), 2);

    let tool_added: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_item.added",
        "item": {
            "type": "function_call",
            "id": "fc_1",
            "call_id": "call_1",
            "name": "list_files"
        }
    }))
    .unwrap();
    let boundary_events = decoder.decode(&tool_added);
    assert_eq!(boundary_events.len(), 3);
    assert!(boundary_events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::BlockClosed {
            id,
            kind: ModelBlockKind::ReasoningSummary,
            ..
        } if id == "thinking"
    )));
    assert!(boundary_events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::BlockClosed {
            id,
            kind:
                ModelBlockKind::Text {
                    channel: TraceTextChannel::Final,
                },
            ..
        } if id == "msg_1"
    )));
    assert!(boundary_events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ToolInputStarted { item_id, .. } if item_id == "fc_1"
    )));

    let completed: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.completed",
        "response": {"id": "resp_1"}
    }))
    .unwrap();
    match decoder.decode(&completed).as_slice() {
        [
            ModelStreamEvent::Completed {
                response_id: Some(response_id),
            },
        ] => assert_eq!(response_id, "resp_1"),
        other => panic!("unexpected events: {other:?}"),
    }
}

#[test]
fn responses_decoder_allocates_new_blocks_after_tool_boundary() {
    let mut decoder = OpenAiStreamDecoder::new(true);
    let reasoning_delta: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.reasoning_summary_text.delta",
        "item_id": "thinking",
        "summary_index": 0,
        "delta": "before tool"
    }))
    .unwrap();
    let first_reasoning = decoder.decode(&reasoning_delta);
    assert!(matches!(
        first_reasoning.as_slice(),
        [
            ModelStreamEvent::BlockOpened {
                id: opened_id,
                kind: ModelBlockKind::ReasoningSummary,
                ..
            },
            ModelStreamEvent::BlockDelta {
                id: delta_id,
                kind: ModelBlockKind::ReasoningSummary,
                ..
            },
        ] if opened_id == "thinking" && delta_id == opened_id
    ));

    let text_delta: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_text.delta",
        "item_id": "msg_1",
        "delta": "before "
    }))
    .unwrap();
    let first_text = decoder.decode(&text_delta);
    assert!(matches!(
        first_text.as_slice(),
        [
            ModelStreamEvent::BlockOpened {
                id: opened_id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                ..
            },
            ModelStreamEvent::BlockDelta {
                id: delta_id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                ..
            },
        ] if opened_id == "msg_1" && delta_id == opened_id
    ));

    let tool_added: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_item.added",
        "item": {
            "type": "function_call",
            "id": "fc_1",
            "call_id": "call_1",
            "name": "list_files"
        }
    }))
    .unwrap();
    let _ = decoder.decode(&tool_added);

    let second_reasoning = decoder.decode(&reasoning_delta);
    assert!(matches!(
        second_reasoning.as_slice(),
        [
            ModelStreamEvent::BlockOpened {
                id: opened_id,
                kind: ModelBlockKind::ReasoningSummary,
                ..
            },
            ModelStreamEvent::BlockDelta {
                id: delta_id,
                kind: ModelBlockKind::ReasoningSummary,
                ..
            },
        ] if opened_id == "thinking#2" && delta_id == opened_id
    ));

    let second_text = decoder.decode(&text_delta);
    assert!(matches!(
        second_text.as_slice(),
        [
            ModelStreamEvent::BlockOpened {
                id: opened_id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                ..
            },
            ModelStreamEvent::BlockDelta {
                id: delta_id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                ..
            },
        ] if opened_id == "msg_1#2" && delta_id == opened_id
    ));
}

#[test]
fn responses_decoder_reopens_text_block_when_phase_arrives_late() {
    let mut decoder = OpenAiStreamDecoder::new(true);
    let default_delta: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_text.delta",
        "item_id": "msg_1",
        "delta": "default "
    }))
    .unwrap();
    let first_text = decoder.decode(&default_delta);
    assert!(matches!(
        first_text.as_slice(),
        [
            ModelStreamEvent::BlockOpened {
                id: opened_id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                ..
            },
            ModelStreamEvent::BlockDelta {
                id: delta_id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                ..
            },
        ] if opened_id == "msg_1" && delta_id == opened_id
    ));

    let commentary_added: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_item.added",
        "item": {
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "phase": "commentary",
            "content": []
        }
    }))
    .unwrap();
    let channel_boundary = decoder.decode(&commentary_added);
    assert!(matches!(
        channel_boundary.as_slice(),
        [
            ModelStreamEvent::BlockClosed {
                id: closed_id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Final,
                    },
                ..
            },
            ModelStreamEvent::BlockOpened {
                id: opened_id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Commentary,
                    },
                ..
            },
        ] if closed_id == "msg_1" && opened_id == "msg_1#2"
    ));

    let commentary_delta: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_text.delta",
        "item_id": "msg_1",
        "delta": "commentary"
    }))
    .unwrap();
    match decoder.decode(&commentary_delta).as_slice() {
        [
            ModelStreamEvent::BlockDelta {
                id,
                kind:
                    ModelBlockKind::Text {
                        channel: TraceTextChannel::Commentary,
                    },
                delta,
                ..
            },
        ] => {
            assert_eq!(id, "msg_1#2");
            assert_eq!(delta, "commentary");
        }
        other => panic!("unexpected events: {other:?}"),
    }
}

#[test]
fn process_responses_custom_tool_delta() {
    let event: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.custom_tool_call_input.delta",
        "item_id": "ctc_1",
        "call_id": "call_1",
        "delta": "*** Begin Patch\n"
    }))
    .unwrap();

    match single_event(&event) {
        Some(ModelStreamEvent::ToolInputDelta {
            item_id,
            call_id,
            payload_delta: ToolInputDeltaPayload::CustomInput(delta),
            ..
        }) => {
            assert_eq!(item_id, "ctc_1");
            assert_eq!(call_id.as_deref(), Some("call_1"));
            assert_eq!(delta, "*** Begin Patch\n");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn process_chat_custom_tool_delta() {
    let event = chat_event(serde_json::json!({
        "tool_calls": [{
            "index": 0,
            "id": "call_1",
            "type": "custom",
            "custom": {
                "name": "apply_patch",
                "input": "*** Begin Patch\n"
            }
        }]
    }));

    match single_event(&event) {
        Some(ModelStreamEvent::ToolInputDelta {
            stream_id,
            item_id,
            name,
            payload_delta: ToolInputDeltaPayload::CustomInput(delta),
            ..
        }) => {
            assert_eq!(stream_id.as_deref(), Some("chat_tool_call:0"));
            assert_eq!(item_id, "call_1");
            assert_eq!(name.as_deref(), Some("apply_patch"));
            assert_eq!(delta, "*** Begin Patch\n");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn process_chat_followup_tool_delta_keeps_stream_id_without_item_id() {
    let event = chat_event(serde_json::json!({
        "tool_calls": [{
            "index": 0,
            "function": {
                "arguments": "{\"path\":\"Cargo.toml\"}"
            }
        }]
    }));

    match single_event(&event) {
        Some(ModelStreamEvent::ToolInputDelta {
            stream_id,
            item_id,
            name,
            payload_delta: ToolInputDeltaPayload::FunctionArguments(delta),
            ..
        }) => {
            assert_eq!(stream_id.as_deref(), Some("chat_tool_call:0"));
            assert_eq!(item_id, "");
            assert_eq!(name, None);
            assert_eq!(delta, "{\"path\":\"Cargo.toml\"}");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn chat_completion_split_tool_chunks_finish_as_one_named_call() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut decoder = OpenAiStreamDecoder::new(false);
    let mut accumulator = StreamCompletionAccumulator::new(None);
    let events = [
        chat_event(serde_json::json!({
            "tool_calls": [{
                "index": 0,
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "list_agents",
                    "arguments": ""
                }
            }]
        })),
        chat_event(serde_json::json!({
            "tool_calls": [{
                "index": 0,
                "function": {
                    "arguments": "{}"
                }
            }]
        })),
        serde_json::from_value(serde_json::json!({
            "choices": [{
                "delta": {},
                "finish_reason": "tool_calls"
            }]
        }))
        .unwrap(),
    ];

    for event in &events {
        for stream_event in decoder.decode(event) {
            accumulator.apply(stream_event, &event_tx).unwrap();
        }
    }
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "call_1");
    assert_eq!(response.tool_calls[0].name, "list_agents");
    match &response.tool_calls[0].payload {
        ToolCallPayload::Function { arguments } => {
            assert_eq!(arguments, &serde_json::json!({}));
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn process_responses_output_item_added_captures_tool_name() {
    let event: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.output_item.added",
        "item": {
            "type": "custom_tool_call",
            "id": "ctc_1",
            "call_id": "call_1",
            "name": "apply_patch"
        }
    }))
    .unwrap();

    match single_event(&event) {
        Some(ModelStreamEvent::ToolInputStarted {
            item_id,
            call_id,
            name,
            payload_kind,
            ..
        }) => {
            assert_eq!(item_id, "ctc_1");
            assert_eq!(call_id.as_deref(), Some("call_1"));
            assert_eq!(name.as_deref(), Some("apply_patch"));
            assert_eq!(payload_kind, ToolInputPayloadKind::CustomInput);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn responses_id_only_added_and_done_canonicalize_function_identity() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut decoder = OpenAiStreamDecoder::new(false);
    let mut accumulator = StreamCompletionAccumulator::new(None);
    let events = [
        serde_json::from_value(serde_json::json!({
            "type": "response.output_item.added",
            "item": {
                "type": "function_call",
                "id": "fc_1",
                "name": "read_file"
            }
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "id": "fc_1",
                "name": "read_file",
                "arguments": "{}"
            }
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type": "response.completed",
            "response": {"id": "resp_1"}
        }))
        .unwrap(),
    ];

    for event in &events {
        for stream_event in decoder.decode(event) {
            accumulator.apply(stream_event, &event_tx).unwrap();
        }
    }
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "fc_1");
    assert_eq!(response.tool_calls[0].call_id, "fc_1");
}

#[test]
fn responses_done_upgrades_fallback_call_id_without_splitting_custom_tool() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut decoder = OpenAiStreamDecoder::new(false);
    let mut accumulator = StreamCompletionAccumulator::new(None);
    let events = [
        serde_json::from_value(serde_json::json!({
            "type": "response.output_item.added",
            "item": {
                "type": "custom_tool_call",
                "id": "ctc_1",
                "name": "apply_patch"
            }
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "type": "custom_tool_call",
                "id": "ctc_1",
                "call_id": "call_1",
                "name": "apply_patch",
                "input": "*** Begin Patch\n*** End Patch"
            }
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type": "response.completed",
            "response": {"id": "resp_1"}
        }))
        .unwrap(),
    ];

    for event in &events {
        for stream_event in decoder.decode(event) {
            accumulator.apply(stream_event, &event_tx).unwrap();
        }
    }
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "ctc_1");
    assert_eq!(response.tool_calls[0].call_id, "call_1");
}

#[test]
fn responses_call_id_only_delta_upgrades_fallback_identity() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut decoder = OpenAiStreamDecoder::new(false);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
    }));
    let events = [
        serde_json::from_value(serde_json::json!({
            "type": "response.output_item.added",
            "item": {
                "type": "function_call",
                "id": "fc_1",
                "name": "read_file"
            }
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type": "response.function_call_arguments.delta",
            "call_id": "call_1",
            "delta": "{}"
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "read_file",
                "arguments": "{}"
            }
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "type": "response.completed",
            "response": {"id": "resp_1"}
        }))
        .unwrap(),
    ];

    for event in &events {
        for stream_event in decoder.decode(event) {
            accumulator.apply(stream_event, &event_tx).unwrap();
        }
    }
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "fc_1");
    assert_eq!(response.tool_calls[0].call_id, "call_1");
}

#[test]
fn responses_id_only_delta_populates_call_id() {
    let event: SseStreamEvent = serde_json::from_value(serde_json::json!({
        "type": "response.function_call_arguments.delta",
        "item_id": "fc_1",
        "delta": "{}"
    }))
    .unwrap();

    assert!(matches!(
        single_event(&event),
        Some(ModelStreamEvent::ToolInputDelta { item_id, call_id: Some(call_id), .. })
            if item_id == "fc_1" && call_id == "fc_1"
    ));
}
