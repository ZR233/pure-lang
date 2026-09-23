//! OpenAI 流解码器：把 Responses/Chat 的 wire 事件流归一为 canonical 事件块序列。
//!
//! Responses 的 output text delta 不携带 assistant message phase，解码器记录
//! `response.output_item.added` 元数据，为后续 delta 补出块开/块关生命周期。

use std::collections::{BTreeSet, HashMap};

use pl_protocol::trace::TraceTextChannel;

use crate::completion::stream::event::{ModelBlockKind, ModelStreamEvent};
use crate::runtime::openai::VisibleOutputProtocol;

use super::item::{
    assistant_message_identity, assistant_message_parts, output_item_native_context,
    presentation_item, reasoning_item_id, reasoning_summary_texts,
};
use super::{DEFAULT_TEXT_ID, SseStreamEvent, process_sse_events, response_model_observation};

/// Stateful OpenAI stream decoder.
///
/// Responses output text deltas do not carry the assistant message phase, so
/// the decoder remembers `response.output_item.added` metadata for the stream.
pub(crate) struct OpenAiStreamDecoder {
    visible_output: VisibleOutputProtocol,
    chat_finished: bool,
    chat_response_id: Option<String>,
    terminal_received: bool,
    text_channels: HashMap<String, TraceTextChannel>,
    open_text_blocks: HashMap<(String, u32), OpenTextBlock>,
    open_reasoning_blocks: HashMap<String, String>,
    next_text_block_ordinal: HashMap<(String, u32), u64>,
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
            chat_finished: false,
            chat_response_id: None,
            terminal_received: false,
            text_channels: HashMap::new(),
            open_text_blocks: HashMap::new(),
            open_reasoning_blocks: HashMap::new(),
            next_text_block_ordinal: HashMap::new(),
            next_reasoning_block_ordinal: HashMap::new(),
        }
    }

    pub(crate) fn decode(&mut self, event: &SseStreamEvent) -> Vec<ModelStreamEvent> {
        if matches!(
            event.kind.as_str(),
            "response.completed" | "response.failed" | "response.incomplete"
        ) {
            if self.terminal_received {
                return Vec::new();
            }
            self.terminal_received = true;
        }
        if let Some(choices) = &event.choices {
            if event.id.is_some() {
                self.chat_response_id.clone_from(&event.id);
            }
            self.chat_finished |= choices.iter().any(|choice| choice.finish_reason.is_some());
        }

        if matches!(self.visible_output, VisibleOutputProtocol::TaggedText) {
            return self.normalize_fallback_events(with_presentation_item(
                event,
                process_sse_events(event),
            ));
        }

        match event.kind.as_str() {
            "response.output_item.added" => {
                if let Some((item_id, channel)) = assistant_message_identity(event.item.as_ref()) {
                    self.text_channels.insert(item_id, channel);
                    return with_response_model_observation(event, Vec::new());
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
                    return with_response_model_observation(event, events);
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
                    let (block_id, mut events) = self.ensure_text_block_open(
                        &item_id,
                        event.content_index.unwrap_or(0).max(0) as u32,
                        channel,
                    );
                    events.push(ModelStreamEvent::text_delta(block_id, channel, delta));
                    return with_response_model_observation(event, events);
                }
                return with_response_model_observation(event, Vec::new());
            }
            "response.output_item.done" => {
                if let Some(item) = event.item.as_ref()
                    && let Some((item_id, item_channel)) = assistant_message_identity(Some(item))
                {
                    let channel = self.text_channels.remove(&item_id).unwrap_or(item_channel);
                    let parts = assistant_message_parts(item);
                    let mut indexes = parts
                        .iter()
                        .map(|(index, _)| *index)
                        .collect::<BTreeSet<_>>();
                    indexes.extend(
                        self.open_text_blocks
                            .keys()
                            .filter(|(id, _)| id == &item_id)
                            .map(|(_, index)| *index),
                    );
                    let mut events = Vec::new();
                    for index in indexes {
                        let authoritative_text = parts
                            .iter()
                            .find(|(part_index, _)| *part_index == index)
                            .map(|(_, text)| text.clone());
                        if authoritative_text.is_some() {
                            events.extend(self.ensure_text_block_open(&item_id, index, channel).1);
                        }
                        if let Some(block) = self.open_text_blocks.remove(&(item_id.clone(), index))
                        {
                            events.push(ModelStreamEvent::text_completed(
                                block.id,
                                channel,
                                authoritative_text,
                            ));
                        }
                    }
                    return with_response_model_observation(
                        event,
                        with_presentation_item(event, events),
                    );
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
                        return with_response_model_observation(
                            event,
                            with_presentation_item(event, Vec::new()),
                        );
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
                    return with_response_model_observation(
                        event,
                        with_presentation_item(event, events),
                    );
                }
            }
            _ => {
                tracing::trace!(kind = %event.kind, "sse event: no special handling, delegating to shared fallback processor");
            }
        }

        self.normalize_fallback_events(with_presentation_item(event, process_sse_events(event)))
    }

    /// Chat finish_reason closes content, while trailing chunks can still carry final usage.
    pub(crate) fn finish(&mut self) -> Vec<ModelStreamEvent> {
        if self.chat_finished && !self.terminal_received {
            self.terminal_received = true;
            let response_id = self.chat_response_id.take();
            self.normalize_fallback_events(vec![ModelStreamEvent::Completed { response_id }])
        } else {
            Vec::new()
        }
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
                    let (block_id, mut events) = self.ensure_text_block_open(&id, 0, channel);
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
                    let (block_id, events) = self.ensure_text_block_open(&id, 0, channel);
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
                    let key = (id.clone(), 0);
                    let was_open = self.open_text_blocks.contains_key(&key);
                    let (block_id, events) = if authoritative_content.is_some() {
                        self.ensure_text_block_open(&id, 0, channel)
                    } else {
                        (id.clone(), Vec::new())
                    };
                    normalized.extend(events);
                    if authoritative_content.is_none() && !was_open {
                        continue;
                    }
                    let block_id = self
                        .open_text_blocks
                        .remove(&key)
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
        content_index: u32,
        channel: TraceTextChannel,
    ) -> (String, Vec<ModelStreamEvent>) {
        let key = (item_id.to_owned(), content_index);
        if let Some(block) = self.open_text_blocks.get(&key)
            && block.channel == channel
        {
            return (block.id.clone(), Vec::new());
        }
        let mut events = Vec::new();
        if let Some(block) = self.open_text_blocks.remove(&key) {
            events.push(ModelStreamEvent::text_completed(
                block.id,
                block.channel,
                None,
            ));
        }
        let block_id = self.next_text_block_id(item_id, content_index);
        self.open_text_blocks.insert(
            key,
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

    fn next_text_block_id(&mut self, item_id: &str, content_index: u32) -> String {
        let key = (item_id.to_owned(), content_index);
        let ordinal = self.next_text_block_ordinal.entry(key).or_insert(0);
        *ordinal += 1;
        let part_id = if content_index == 0 {
            item_id.to_owned()
        } else {
            format!("{}:{item_id}:part:{content_index}", item_id.len())
        };
        if *ordinal == 1 {
            part_id
        } else {
            format!("{part_id}#{}", *ordinal)
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

fn with_response_model_observation(
    event: &SseStreamEvent,
    mut events: Vec<ModelStreamEvent>,
) -> Vec<ModelStreamEvent> {
    if let Some(model_observation) = response_model_observation(event) {
        events.insert(0, model_observation);
    }
    events
}

fn with_presentation_item(
    event: &SseStreamEvent,
    mut events: Vec<ModelStreamEvent>,
) -> Vec<ModelStreamEvent> {
    if event.kind == "response.output_item.done"
        && let Some(item) = event
            .item
            .as_ref()
            .and_then(|item| presentation_item(item, event.output_index))
    {
        events.push(ModelStreamEvent::PresentationItem { item });
    }
    events
}
