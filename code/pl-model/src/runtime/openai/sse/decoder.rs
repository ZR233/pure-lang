//! OpenAI 流解码器：把 Responses/Chat 的 wire 事件流归一为 canonical 事件块序列。
//!
//! Responses 的 output text delta 不携带 assistant message phase，解码器记录
//! `response.output_item.added` 元数据，为后续 delta 补出块开/块关生命周期。

use std::collections::HashMap;

use pl_trace::TraceTextChannel;

use crate::completion::stream::event::{ModelBlockKind, ModelStreamEvent};

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
    use_native_phases: bool,
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
    pub(crate) fn new(use_native_phases: bool) -> Self {
        Self {
            use_native_phases,
            text_channels: HashMap::new(),
            open_text_blocks: HashMap::new(),
            open_reasoning_blocks: HashMap::new(),
            next_text_block_ordinal: HashMap::new(),
            next_reasoning_block_ordinal: HashMap::new(),
        }
    }

    pub(crate) fn decode(&mut self, event: &SseStreamEvent) -> Vec<ModelStreamEvent> {
        if !self.use_native_phases {
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
