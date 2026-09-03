//! OpenAI 流解码器：把 Responses/Chat 的 wire 事件流归一为 canonical 事件块序列。
//!
//! Responses 的 output text delta 不携带 assistant message phase，解码器记录
//! `response.output_item.added` 元数据，为后续 delta 补出块开/块关生命周期。

use std::collections::HashMap;

use pl_trace::TraceTextChannel;

use crate::completion::stream::event::{ModelBlockKind, ModelStreamEvent};
use crate::runtime::openai::VisibleOutputProtocol;

use super::item::{
    assistant_message_identity, assistant_message_text, output_item_native_context,
    reasoning_item_id, reasoning_summary_texts,
};
use super::{DEFAULT_TEXT_ID, SseStreamEvent, process_sse_events};

/// Stateful OpenAI stream decoder.
///
/// Responses output text deltas do not carry the assistant message phase, so
/// the decoder remembers `response.output_item.added` metadata for the stream.
pub(crate) struct OpenAiStreamDecoder {
    visible_output: VisibleOutputProtocol,
    text_channels: HashMap<String, TraceTextChannel>,
    open_text_blocks: HashMap<String, OpenTextBlock>,
    open_reasoning_blocks: HashMap<String, String>,
    next_text_block_ordinal: HashMap<String, u64>,
    next_reasoning_block_ordinal: HashMap<String, u64>,
}

#[derive(Debug, Clone)]
struct OpenTextBlock {
    id: String,
    channel: TraceTextChannel,
}

impl OpenAiStreamDecoder {
    pub(crate) fn new(visible_output: VisibleOutputProtocol) -> Self {
        Self {
            visible_output,
            text_channels: HashMap::new(),
            open_text_blocks: HashMap::new(),
            open_reasoning_blocks: HashMap::new(),
            next_text_block_ordinal: HashMap::new(),
            next_reasoning_block_ordinal: HashMap::new(),
        }
    }

    pub(crate) fn decode(&mut self, event: &SseStreamEvent) -> Vec<ModelStreamEvent> {
        if matches!(self.visible_output, VisibleOutputProtocol::TaggedText) {
            return self.normalize_fallback_events(process_sse_events(event));
        }

        match event.kind.as_str() {
            "response.output_item.added" => {
                if let Some((item_id, channel)) = assistant_message_identity(event.item.as_ref()) {
                    self.text_channels.insert(item_id.clone(), channel);
                    let (block_id, events) = self.ensure_text_block_open(&item_id, channel);
                    let _ = block_id;
                    return events;
                }
                if let Some(item) = event.item.as_ref()
                    && let Some(item_id) = reasoning_item_id(item)
                {
                    let (block_id, mut events) = self.ensure_reasoning_block_open(&item_id);
                    if let Some(ModelStreamEvent::BlockOpened {
                        provider_metadata, ..
                    }) = events.first_mut()
                    {
                        *provider_metadata = Some(item.clone());
                    }
                    let _ = block_id;
                    return events;
                }
            }
            "response.output_text.delta" => {
                let item_id = event
                    .item_id
                    .clone()
                    .unwrap_or_else(|| DEFAULT_TEXT_ID.to_string());
                let channel = self
                    .text_channels
                    .get(&item_id)
                    .copied()
                    .unwrap_or(TraceTextChannel::Final);
                if let Some(delta) = event.delta.clone() {
                    let (block_id, mut events) = self.ensure_text_block_open(&item_id, channel);
                    events.push(ModelStreamEvent::text_delta(block_id, channel, delta));
                    return events;
                }
                return Vec::new();
            }
            "response.output_item.done" => {
                if let Some(item) = event.item.as_ref()
                    && let Some((item_id, item_channel)) = assistant_message_identity(Some(item))
                {
                    let channel = self.text_channels.remove(&item_id).unwrap_or(item_channel);
                    let authoritative_text = assistant_message_text(item);
                    let was_open = self.open_text_blocks.contains_key(&item_id);
                    let (block_id, mut events) = if authoritative_text.is_some() {
                        self.ensure_text_block_open(&item_id, channel)
                    } else {
                        (item_id.clone(), Vec::new())
                    };
                    if authoritative_text.is_none() && !was_open {
                        return Vec::new();
                    }
                    let block_id = self
                        .open_text_blocks
                        .remove(&item_id)
                        .map(|block| block.id)
                        .unwrap_or(block_id);
                    events.push(ModelStreamEvent::text_completed(
                        block_id,
                        channel,
                        authoritative_text,
                    ));
                    return events;
                }
                if let Some(item) = event.item.as_ref()
                    && let Some(item_id) = reasoning_item_id(item)
                {
                    let authoritative_summary = reasoning_summary_texts(item);
                    let was_open = self.open_reasoning_blocks.contains_key(&item_id);
                    let (block_id, mut events) = if authoritative_summary.is_some() {
                        self.ensure_reasoning_block_open(&item_id)
                    } else {
                        (item_id.clone(), Vec::new())
                    };
                    if authoritative_summary.is_none() && !was_open {
                        return Vec::new();
                    }
                    let block_id = self
                        .open_reasoning_blocks
                        .remove(&item_id)
                        .unwrap_or(block_id);
                    events.push(ModelStreamEvent::reasoning_summary_completed(
                        block_id,
                        Some(item.clone()),
                        authoritative_summary,
                    ));
                    if let Some(native) = output_item_native_context(item) {
                        events.push(native);
                    }
                    return events;
                }
            }
            _ => {
                tracing::trace!(kind = %event.kind, "sse event: no special handling, delegating to shared fallback processor");
            }
        }

        self.normalize_fallback_events(process_sse_events(event))
    }

    fn normalize_fallback_events(
        &mut self,
        events: Vec<ModelStreamEvent>,
    ) -> Vec<ModelStreamEvent> {
        let mut normalized = Vec::new();
        for event in events {
            match event {
                ModelStreamEvent::BlockOpened {
                    id,
                    kind: ModelBlockKind::Text { channel },
                    provider_metadata,
                } => {
                    let (block_id, mut events) = self.ensure_text_block_open(&id, channel);
                    if let Some(ModelStreamEvent::BlockOpened {
                        provider_metadata: metadata,
                        ..
                    }) = events.first_mut()
                    {
                        *metadata = provider_metadata;
                    }
                    let _ = block_id;
                    normalized.extend(events);
                }
                ModelStreamEvent::BlockOpened {
                    id,
                    kind: ModelBlockKind::ReasoningSummary,
                    provider_metadata,
                } => {
                    let (block_id, mut events) = self.ensure_reasoning_block_open(&id);
                    if let Some(ModelStreamEvent::BlockOpened {
                        provider_metadata: metadata,
                        ..
                    }) = events.first_mut()
                    {
                        *metadata = provider_metadata;
                    }
                    let _ = block_id;
                    normalized.extend(events);
                }
                ModelStreamEvent::BlockDelta {
                    id,
                    kind: ModelBlockKind::Text { channel },
                    field,
                    delta,
                    section_index,
                } => {
                    let (block_id, events) = self.ensure_text_block_open(&id, channel);
                    normalized.extend(events);
                    normalized.push(ModelStreamEvent::BlockDelta {
                        id: block_id,
                        kind: ModelBlockKind::Text { channel },
                        field,
                        delta,
                        section_index,
                    });
                }
                ModelStreamEvent::BlockDelta {
                    id,
                    kind: ModelBlockKind::ReasoningSummary,
                    field,
                    delta,
                    section_index,
                } => {
                    let (block_id, events) = self.ensure_reasoning_block_open(&id);
                    normalized.extend(events);
                    normalized.push(ModelStreamEvent::BlockDelta {
                        id: block_id,
                        kind: ModelBlockKind::ReasoningSummary,
                        field,
                        delta,
                        section_index,
                    });
                }
                ModelStreamEvent::BlockClosed {
                    id,
                    kind: ModelBlockKind::Text { channel },
                    authoritative_content,
                    provider_metadata,
                } => {
                    let was_open = self.open_text_blocks.contains_key(&id);
                    let (block_id, events) = if authoritative_content.is_some() {
                        self.ensure_text_block_open(&id, channel)
                    } else {
                        (id.clone(), Vec::new())
                    };
                    normalized.extend(events);
                    if authoritative_content.is_none() && !was_open {
                        continue;
                    }
                    let block_id = self
                        .open_text_blocks
                        .remove(&id)
                        .map(|block| block.id)
                        .unwrap_or(block_id);
                    normalized.push(ModelStreamEvent::BlockClosed {
                        id: block_id,
                        kind: ModelBlockKind::Text { channel },
                        authoritative_content,
                        provider_metadata,
                    });
                }
                ModelStreamEvent::BlockClosed {
                    id,
                    kind: ModelBlockKind::ReasoningSummary,
                    authoritative_content,
                    provider_metadata,
                } => {
                    let was_open = self.open_reasoning_blocks.contains_key(&id);
                    let (block_id, events) = if authoritative_content.is_some() {
                        self.ensure_reasoning_block_open(&id)
                    } else {
                        (id.clone(), Vec::new())
                    };
                    normalized.extend(events);
                    if authoritative_content.is_none() && !was_open {
                        continue;
                    }
                    let block_id = self.open_reasoning_blocks.remove(&id).unwrap_or(block_id);
                    normalized.push(ModelStreamEvent::BlockClosed {
                        id: block_id,
                        kind: ModelBlockKind::ReasoningSummary,
                        authoritative_content,
                        provider_metadata,
                    });
                }
                event @ (ModelStreamEvent::ToolInputStarted { .. }
                | ModelStreamEvent::ToolInputDelta { .. }
                | ModelStreamEvent::ToolCallReady { .. }
                | ModelStreamEvent::ResponseStarted { .. }) => {
                    normalized.extend(self.close_open_content_blocks());
                    normalized.push(event);
                }
                ModelStreamEvent::Completed { response_id } => {
                    normalized.extend(self.close_open_content_blocks());
                    normalized.push(ModelStreamEvent::Completed { response_id });
                }
                other => normalized.push(other),
            }
        }
        normalized
    }

    fn ensure_text_block_open(
        &mut self,
        item_id: &str,
        channel: TraceTextChannel,
    ) -> (String, Vec<ModelStreamEvent>) {
        if let Some(block) = self.open_text_blocks.get(item_id)
            && block.channel == channel
        {
            return (block.id.clone(), Vec::new());
        }
        let mut events = Vec::new();
        if let Some(block) = self.open_text_blocks.remove(item_id) {
            events.push(ModelStreamEvent::text_completed(
                block.id,
                block.channel,
                None,
            ));
        }
        let block_id = self.next_text_block_id(item_id);
        self.open_text_blocks.insert(
            item_id.to_string(),
            OpenTextBlock {
                id: block_id.clone(),
                channel,
            },
        );
        events.push(ModelStreamEvent::text_started(block_id.clone(), channel));
        (block_id, events)
    }

    fn ensure_reasoning_block_open(&mut self, item_id: &str) -> (String, Vec<ModelStreamEvent>) {
        if let Some(block_id) = self.open_reasoning_blocks.get(item_id) {
            return (block_id.clone(), Vec::new());
        }
        let block_id = self.next_reasoning_block_id(item_id);
        self.open_reasoning_blocks
            .insert(item_id.to_string(), block_id.clone());
        (
            block_id.clone(),
            vec![ModelStreamEvent::reasoning_summary_started(block_id, None)],
        )
    }

    fn close_open_content_blocks(&mut self) -> Vec<ModelStreamEvent> {
        let mut events = Vec::new();
        for (_, block) in std::mem::take(&mut self.open_text_blocks) {
            events.push(ModelStreamEvent::text_completed(
                block.id,
                block.channel,
                None,
            ));
        }
        for (_, block_id) in std::mem::take(&mut self.open_reasoning_blocks) {
            events.push(ModelStreamEvent::reasoning_summary_completed(
                block_id, None, None,
            ));
        }
        events
    }

    fn next_text_block_id(&mut self, item_id: &str) -> String {
        let key = text_block_counter_key(item_id);
        let ordinal = self.next_text_block_ordinal.entry(key).or_insert(0);
        *ordinal += 1;
        if *ordinal == 1 {
            item_id.to_string()
        } else {
            let ord = *ordinal;
            format!("{item_id}#{ord}")
        }
    }

    fn next_reasoning_block_id(&mut self, item_id: &str) -> String {
        let ordinal = self
            .next_reasoning_block_ordinal
            .entry(item_id.to_string())
            .or_insert(0);
        *ordinal += 1;
        if *ordinal == 1 {
            item_id.to_string()
        } else {
            let ord = *ordinal;
            format!("{item_id}#{ord}")
        }
    }
}

fn text_block_counter_key(item_id: &str) -> String {
    item_id.to_string()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::completion::stream::StreamCompletionAccumulator;
    use crate::completion::stream::event::{ModelBlockContent, ModelBlockField, ModelBlockKind};
    use crate::completion::{CompletionTraceContext, ToolCallPayload};

    fn chat_event(delta: serde_json::Value) -> SseStreamEvent {
        serde_json::from_value(serde_json::json!({
            "choices": [{
                "delta": delta,
                "finish_reason": null
            }]
        }))
        .unwrap()
    }

    /// 把 SSE wire 事件完整跑过 decoder + accumulator，返回收敛后的补全响应。
    fn run_to_response(
        events: &[SseStreamEvent],
        trace: Option<crate::completion::CompletionTraceContext>,
    ) -> crate::completion::CompletionResponse {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut decoder = OpenAiStreamDecoder::new(VisibleOutputProtocol::TaggedText);
        let mut accumulator = StreamCompletionAccumulator::new(trace);
        for event in events {
            for stream_event in decoder.decode(event) {
                accumulator.apply(stream_event, &event_tx).unwrap();
            }
        }
        accumulator.finish(&event_tx).unwrap()
    }

    #[test]
    fn responses_decoder_preserves_native_text_phase_and_completed_text() {
        let mut decoder = OpenAiStreamDecoder::new(VisibleOutputProtocol::NativePhases);
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
        let mut decoder = OpenAiStreamDecoder::new(VisibleOutputProtocol::NativePhases);
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
        let mut decoder = OpenAiStreamDecoder::new(VisibleOutputProtocol::NativePhases);
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
        let mut decoder = OpenAiStreamDecoder::new(VisibleOutputProtocol::NativePhases);
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
        let mut decoder = OpenAiStreamDecoder::new(VisibleOutputProtocol::NativePhases);
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
    fn chat_completion_split_tool_chunks_finish_as_one_named_call() {
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

        let response = run_to_response(&events, None);

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
    fn responses_id_only_added_and_done_canonicalize_function_identity() {
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

        let response = run_to_response(&events, None);

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "fc_1");
        assert_eq!(response.tool_calls[0].call_id, "fc_1");
    }

    #[test]
    fn responses_done_upgrades_fallback_call_id_without_splitting_custom_tool() {
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

        let response = run_to_response(&events, None);

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "ctc_1");
        assert_eq!(response.tool_calls[0].call_id, "call_1");
    }

    #[test]
    fn responses_call_id_only_delta_upgrades_fallback_identity() {
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

        let response = run_to_response(
            &events,
            Some(CompletionTraceContext {
                session_id: "session-1".to_string(),
                turn_id: "turn-1".to_string(),
                inference_id: "turn-1-inf-0".to_string(),
            }),
        );

        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "fc_1");
        assert_eq!(response.tool_calls[0].call_id, "call_1");
    }
}
