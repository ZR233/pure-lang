//! OpenAI 流解码器：把 Responses/Chat 的 wire 事件流归一为 canonical 事件块序列。
//!
//! Responses 的 output text delta 不携带 assistant message phase，解码器记录
//! `response.output_item.added` 元数据，为后续 delta 补出块开/块关生命周期。

use std::collections::{BTreeSet, HashMap};

use pl_protocol::trace::TraceTextChannel;

use crate::completion::stream::event::{ModelBlockKind, ModelStreamEvent, ProviderBlockIdentity};
use crate::runtime::openai::VisibleOutputProtocol;

use super::item::{
    assistant_message_identity, assistant_message_parts, output_item_native_context,
    presentation_item, reasoning_item_id, reasoning_summary_texts,
};
use super::{SseStreamEvent, process_sse_events, response_model_observation};

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
    /// Provider item id announced for each Responses `output_index`.
    ///
    /// A delta that omits `item_id` is attributed to the item its `output_index` was announced for,
    /// never to a synthetic default that could duplicate the real item.
    output_index_items: HashMap<u32, String>,
    /// Set when the decoder rejected the stream as a protocol error; later events are dropped.
    protocol_failed: bool,
}

#[derive(Debug, Clone)]
struct OpenTextBlock {
    id: String,
    channel: TraceTextChannel,
    provider: Option<ProviderBlockIdentity>,
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
            output_index_items: HashMap::new(),
            protocol_failed: false,
        }
    }

    pub(crate) fn decode(&mut self, event: &SseStreamEvent) -> Vec<ModelStreamEvent> {
        if self.protocol_failed {
            return Vec::new();
        }
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
                self.remember_output_index(event);
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
                let Some(delta) = event.delta.clone() else {
                    return with_response_model_observation(event, Vec::new());
                };
                let Some(item_id) = self.event_item_id(event) else {
                    return self.protocol_failure(
                        "provider stream protocol error: output text delta carries no item id and no announced output index",
                    );
                };
                let channel = self
                    .text_channels
                    .get(&item_id)
                    .copied()
                    .unwrap_or(TraceTextChannel::Final);
                let identity = ProviderBlockIdentity {
                    item_id: item_id.clone(),
                    content_index: event.content_index.unwrap_or(0).max(0) as u32,
                };
                let (block_id, mut events) = self.ensure_text_block_open(
                    &item_id,
                    identity.content_index,
                    channel,
                    Some(identity.clone()),
                );
                events.push(ModelStreamEvent::text_delta(
                    block_id,
                    channel,
                    delta,
                    Some(identity),
                ));
                return with_response_model_observation(event, events);
            }
            "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                let Some(delta) = event.delta.clone() else {
                    return with_response_model_observation(event, Vec::new());
                };
                let is_summary = event.kind == "response.reasoning_summary_text.delta";
                let Some(item_id) = self.event_item_id(event) else {
                    return self.protocol_failure(
                        "provider stream protocol error: reasoning delta carries no item id and no announced output index",
                    );
                };
                // A summary part is one content part of its item, identified by the provider's
                // `summary_index`; raw reasoning is one content part identified by its
                // `content_index`. Both carry the index their terminal presentation part finalizes,
                // so multiple summaries of one item stay distinct parts instead of collapsing into
                // index zero and overwriting each other.
                let content_index = if is_summary {
                    event.summary_index.unwrap_or(0).max(0) as u32
                } else {
                    event.content_index.unwrap_or(0).max(0) as u32
                };
                let identity = ProviderBlockIdentity {
                    item_id: item_id.clone(),
                    content_index,
                };
                if is_summary {
                    let (block_id, mut events) = self.ensure_reasoning_block_open(&item_id);
                    events.push(ModelStreamEvent::reasoning_summary_delta(
                        block_id,
                        content_index,
                        delta,
                        Some(identity),
                    ));
                    return with_response_model_observation(event, events);
                }
                return with_response_model_observation(
                    event,
                    vec![ModelStreamEvent::ReasoningRawDelta {
                        id: item_id,
                        content_index,
                        delta,
                        provider: Some(identity),
                    }],
                );
            }
            "response.output_item.done" => {
                self.remember_output_index(event);
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
                        let identity = ProviderBlockIdentity {
                            item_id: item_id.clone(),
                            content_index: index,
                        };
                        if authoritative_text.is_some() {
                            events.extend(
                                self.ensure_text_block_open(
                                    &item_id,
                                    index,
                                    channel,
                                    Some(identity),
                                )
                                .1,
                            );
                        }
                        if let Some(block) = self.open_text_blocks.remove(&(item_id.clone(), index))
                        {
                            events.push(ModelStreamEvent::text_completed(
                                block.id,
                                channel,
                                authoritative_text,
                                block.provider,
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
                    let identity = ProviderBlockIdentity {
                        item_id: item_id.clone(),
                        content_index: 0,
                    };
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
                        Some(identity),
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

    /// Remembers which provider item an `output_index` was announced for.
    fn remember_output_index(&mut self, event: &SseStreamEvent) {
        let Some(index) = event.output_index else {
            return;
        };
        let Some(item_id) = event
            .item
            .as_ref()
            .and_then(|item| item.get("id"))
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        self.output_index_items.insert(index, item_id.to_owned());
    }

    /// Provider item id of a delta: its own `item_id`, or the item its `output_index` announced.
    fn event_item_id(&self, event: &SseStreamEvent) -> Option<String> {
        match event.item_id.as_deref() {
            Some(item_id) if !item_id.is_empty() => Some(item_id.to_owned()),
            _ => event
                .output_index
                .and_then(|index| self.output_index_items.get(&index).cloned()),
        }
    }

    /// Rejects the stream as a protocol error instead of attributing content to a fabricated item.
    ///
    /// The failure closes the invocation through the accumulator, so every observation received
    /// before it is still retained in the failure receipt.
    fn protocol_failure(&mut self, message: &str) -> Vec<ModelStreamEvent> {
        self.protocol_failed = true;
        vec![ModelStreamEvent::Failed {
            code: Some("invalid_response".to_owned()),
            http_status: None,
            retry_after_ms: None,
            message: message.to_owned(),
        }]
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
                    provider,
                } => {
                    let (block_id, mut events) =
                        self.ensure_text_block_open(&id, 0, channel, provider);
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
                    ..
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
                    provider,
                } => {
                    let (block_id, events) =
                        self.ensure_text_block_open(&id, 0, channel, provider.clone());
                    normalized.extend(events);
                    normalized.push(ModelStreamEvent::BlockDelta {
                        id: block_id,
                        kind: ModelBlockKind::Text { channel },
                        field,
                        delta,
                        section_index,
                        provider,
                    });
                }
                ModelStreamEvent::BlockDelta {
                    id,
                    kind: ModelBlockKind::ReasoningSummary,
                    field,
                    delta,
                    section_index,
                    provider,
                } => {
                    let (block_id, events) = self.ensure_reasoning_block_open(&id);
                    normalized.extend(events);
                    normalized.push(ModelStreamEvent::BlockDelta {
                        id: block_id,
                        kind: ModelBlockKind::ReasoningSummary,
                        field,
                        delta,
                        section_index,
                        provider,
                    });
                }
                ModelStreamEvent::BlockClosed {
                    id,
                    kind: ModelBlockKind::Text { channel },
                    authoritative_content,
                    provider_metadata,
                    provider,
                } => {
                    let key = (id.clone(), 0);
                    let was_open = self.open_text_blocks.contains_key(&key);
                    let (block_id, events) = if authoritative_content.is_some() {
                        self.ensure_text_block_open(&id, 0, channel, provider.clone())
                    } else {
                        (id.clone(), Vec::new())
                    };
                    normalized.extend(events);
                    if authoritative_content.is_none() && !was_open {
                        continue;
                    }
                    let (block_id, provider) = self
                        .open_text_blocks
                        .remove(&key)
                        .map(|block| (block.id, block.provider))
                        .unwrap_or((block_id, provider));
                    normalized.push(ModelStreamEvent::BlockClosed {
                        id: block_id,
                        kind: ModelBlockKind::Text { channel },
                        authoritative_content,
                        provider_metadata,
                        provider,
                    });
                }
                ModelStreamEvent::BlockClosed {
                    id,
                    kind: ModelBlockKind::ReasoningSummary,
                    authoritative_content,
                    provider_metadata,
                    provider,
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
                        provider,
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
        provider: Option<ProviderBlockIdentity>,
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
                block.provider,
            ));
        }
        let block_id = self.next_text_block_id(item_id, content_index);
        self.open_text_blocks.insert(
            key,
            OpenTextBlock {
                id: block_id.clone(),
                channel,
                provider: provider.clone(),
            },
        );
        events.push(ModelStreamEvent::text_started(
            block_id.clone(),
            channel,
            provider,
        ));
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
            vec![ModelStreamEvent::reasoning_summary_started(
                block_id,
                None,
                Some(ProviderBlockIdentity {
                    item_id: item_id.to_string(),
                    content_index: 0,
                }),
            )],
        )
    }

    fn close_open_content_blocks(&mut self) -> Vec<ModelStreamEvent> {
        let mut events = Vec::new();
        for (_, block) in std::mem::take(&mut self.open_text_blocks) {
            events.push(ModelStreamEvent::text_completed(
                block.id,
                block.channel,
                None,
                block.provider,
            ));
        }
        for (item_id, block_id) in std::mem::take(&mut self.open_reasoning_blocks) {
            events.push(ModelStreamEvent::reasoning_summary_completed(
                block_id,
                None,
                None,
                Some(ProviderBlockIdentity {
                    item_id,
                    content_index: 0,
                }),
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
