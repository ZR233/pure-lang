use std::collections::HashMap;

use serde::Deserialize;

use crate::completion::TokenUsage;
use crate::completion::stream::event::{ModelBlockKind, ModelStreamEvent, ToolInputDeltaPayload};
use pl_trace::TraceTextChannel;

mod item;

use item::{
    assistant_message_identity, assistant_message_text, cache_write_tokens_from_details,
    cached_tokens_from_details, output_item_native_context, output_item_tool_completed,
    output_item_tool_started, reasoning_item_id, reasoning_summary_texts,
    web_search_lifecycle_event,
};

/// SSE 流事件原始结构（从 JSON 解析）
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SseStreamEvent {
    #[serde(rename = "type")]
    #[serde(default)]
    pub kind: String,
    pub delta: Option<String>,
    pub item: Option<serde_json::Value>,
    pub item_id: Option<String>,
    pub call_id: Option<String>,
    pub response: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
    pub code: Option<String>,
    pub message: Option<String>,
    pub retry_after_ms: Option<u64>,
    pub status: Option<serde_json::Value>,
    pub status_code: Option<serde_json::Value>,
    pub summary_index: Option<i64>,
    pub content_index: Option<i64>,
    pub choices: Option<Vec<ChatStreamChoice>>,
    pub usage: Option<ChatTokenUsage>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatStreamChoice {
    pub delta: ChatStreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatStreamDelta {
    pub content: Option<String>,
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<ChatStreamToolCallDelta>>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatStreamToolCallDelta {
    pub index: Option<usize>,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub function: Option<ChatStreamFunctionDelta>,
    pub custom: Option<ChatStreamCustomDelta>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatStreamFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatStreamCustomDelta {
    pub name: Option<String>,
    pub input: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ChatTokenUsage {
    pub prompt_tokens: Option<u64>,
    pub input_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub prompt_tokens_details: Option<serde_json::Value>,
    pub input_tokens_details: Option<serde_json::Value>,
    pub completion_tokens_details: Option<serde_json::Value>,
    pub output_tokens_details: Option<serde_json::Value>,
    pub prompt_cache_hit_tokens: Option<u64>,
    pub cached_prompt_tokens: Option<u64>,
}

const DEFAULT_TEXT_ID: &str = "final";
const DEFAULT_REASONING_ID: &str = "thinking";

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

/// 将原始 SSE 事件解析为 canonical stream event。
pub fn process_sse_events(event: &SseStreamEvent) -> Vec<ModelStreamEvent> {
    match process_sse_event(event) {
        Some(StreamEventBatch::Single(event)) => vec![event],
        Some(StreamEventBatch::Many(events)) => events,
        None => Vec::new(),
    }
}

enum StreamEventBatch {
    Single(ModelStreamEvent),
    Many(Vec<ModelStreamEvent>),
}

fn process_sse_event(event: &SseStreamEvent) -> Option<StreamEventBatch> {
    if let Some(event) = web_search_lifecycle_event(event) {
        return Some(StreamEventBatch::Single(event));
    }
    if let Some(choice) = event.choices.as_ref().and_then(|choices| choices.first()) {
        let mut events = Vec::new();
        if let Some(delta) = &choice.delta.reasoning_content
            && !delta.is_empty()
        {
            events.push(ModelStreamEvent::ReasoningRawDelta {
                id: DEFAULT_REASONING_ID.to_string(),
                content_index: 0,
                delta: delta.clone(),
            });
        }

        if let Some(content) = &choice.delta.content
            && !content.is_empty()
        {
            events.push(ModelStreamEvent::text_delta(
                DEFAULT_TEXT_ID.to_string(),
                TraceTextChannel::Final,
                content.clone(),
            ));
        }

        if let Some(tool_calls) = &choice.delta.tool_calls {
            for tool_call in tool_calls {
                let index = tool_call.index.unwrap_or_default();
                let stream_id = Some(format!("chat_tool_call:{index}"));
                let item_id = tool_call.id.clone().unwrap_or_default();
                // Chat Completions 只暴露 item id；确定性赋 call_id = item_id。
                let call_id = (!item_id.is_empty()).then(|| item_id.clone());
                if let Some(custom) = &tool_call.custom {
                    events.push(ModelStreamEvent::ToolInputDelta {
                        stream_id,
                        item_id,
                        call_id,
                        name: custom.name.clone(),
                        payload_delta: ToolInputDeltaPayload::CustomInput(
                            custom.input.clone().unwrap_or_default(),
                        ),
                    });
                    continue;
                }
                if let Some(function) = &tool_call.function {
                    events.push(ModelStreamEvent::ToolInputDelta {
                        stream_id,
                        item_id,
                        call_id,
                        name: function.name.clone(),
                        payload_delta: ToolInputDeltaPayload::FunctionArguments(
                            function.arguments.clone().unwrap_or_default(),
                        ),
                    });
                }
            }
        }

        if choice.finish_reason.is_some() {
            let usage = event.usage.as_ref().map(|u| TokenUsage {
                prompt_tokens: u.prompt_tokens.or(u.input_tokens).unwrap_or(0),
                completion_tokens: u.completion_tokens.or(u.output_tokens).unwrap_or(0),
                total_tokens: u.total_tokens.unwrap_or(0),
                cached_prompt_tokens: u
                    .prompt_cache_hit_tokens
                    .or(u.cached_prompt_tokens)
                    .or_else(|| {
                        u.prompt_tokens_details
                            .as_ref()
                            .and_then(cached_tokens_from_details)
                    })
                    .or_else(|| {
                        u.input_tokens_details
                            .as_ref()
                            .and_then(cached_tokens_from_details)
                    })
                    .unwrap_or(0),
                cache_write_tokens: u
                    .input_tokens_details
                    .as_ref()
                    .and_then(cache_write_tokens_from_details)
                    .or_else(|| {
                        u.prompt_tokens_details
                            .as_ref()
                            .and_then(cache_write_tokens_from_details)
                    })
                    .unwrap_or(0),
                reasoning_tokens: u
                    .output_tokens_details
                    .as_ref()
                    .and_then(|details| details.get("reasoning_tokens"))
                    .and_then(serde_json::Value::as_u64)
                    .or_else(|| {
                        u.completion_tokens_details
                            .as_ref()
                            .and_then(|details| details.get("reasoning_tokens"))
                            .and_then(serde_json::Value::as_u64)
                    })
                    .unwrap_or(0),
            });
            if let Some(usage) = usage {
                events.push(ModelStreamEvent::Usage(usage));
            }
            events.push(ModelStreamEvent::Completed { response_id: None });
        }

        if !events.is_empty() {
            return Some(StreamEventBatch::Many(events));
        }
    }

    match event.kind.as_str() {
        "response.created" => Some(StreamEventBatch::Single(
            ModelStreamEvent::ResponseStarted {
                response_id: event
                    .response
                    .as_ref()
                    .and_then(|r| r.get("id")?.as_str().map(String::from)),
            },
        )),

        "response.output_text.delta" => event.delta.as_ref().map(|d| {
            StreamEventBatch::Single(ModelStreamEvent::text_delta(
                event
                    .item_id
                    .clone()
                    .unwrap_or_else(|| DEFAULT_TEXT_ID.to_string()),
                TraceTextChannel::Final,
                d.clone(),
            ))
        }),

        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            event.delta.as_ref().map(|d| {
                if event.kind == "response.reasoning_summary_text.delta" {
                    StreamEventBatch::Single(ModelStreamEvent::reasoning_summary_delta(
                        event
                            .item_id
                            .clone()
                            .unwrap_or_else(|| DEFAULT_REASONING_ID.to_string()),
                        event.summary_index.unwrap_or(0).max(0) as u32,
                        d.clone(),
                    ))
                } else {
                    StreamEventBatch::Single(ModelStreamEvent::ReasoningRawDelta {
                        id: event
                            .item_id
                            .clone()
                            .unwrap_or_else(|| DEFAULT_REASONING_ID.to_string()),
                        content_index: event.content_index.unwrap_or(0).max(0) as u32,
                        delta: d.clone(),
                    })
                }
            })
        }

        "response.function_call_arguments.delta" => {
            let (item_id, call_id) = responses_tool_identity(event);
            Some(StreamEventBatch::Single(ModelStreamEvent::ToolInputDelta {
                stream_id: None,
                item_id,
                call_id: Some(call_id),
                name: None,
                payload_delta: ToolInputDeltaPayload::FunctionArguments(
                    event.delta.clone().unwrap_or_default(),
                ),
            }))
        }

        "response.custom_tool_call_input.delta" => {
            let (item_id, call_id) = responses_tool_identity(event);
            Some(StreamEventBatch::Single(ModelStreamEvent::ToolInputDelta {
                stream_id: None,
                item_id,
                call_id: Some(call_id),
                name: None,
                payload_delta: ToolInputDeltaPayload::CustomInput(
                    event.delta.clone().unwrap_or_default(),
                ),
            }))
        }

        "response.output_item.added" => event
            .item
            .as_ref()
            .and_then(output_item_tool_started)
            .map(StreamEventBatch::Single),

        "response.output_item.done" => event.item.as_ref().and_then(|item| {
            let mut events = output_item_tool_completed(item).unwrap_or_default();
            if let Some(native) = output_item_native_context(item) {
                events.push(native);
            }
            (!events.is_empty()).then_some(StreamEventBatch::Many(events))
        }),

        "response.completed" => {
            let usage = event.response.as_ref().and_then(|r| {
                r.get("usage").and_then(|u| {
                    Some(TokenUsage {
                        prompt_tokens: u.get("input_tokens")?.as_u64()?,
                        completion_tokens: u.get("output_tokens")?.as_u64()?,
                        total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                        cached_prompt_tokens: u
                            .get("input_tokens_details")
                            .and_then(cached_tokens_from_details)
                            .or_else(|| {
                                u.get("prompt_tokens_details")
                                    .and_then(cached_tokens_from_details)
                            })
                            .or_else(|| u.get("prompt_cache_hit_tokens").and_then(|v| v.as_u64()))
                            .unwrap_or(0),
                        cache_write_tokens: u
                            .get("input_tokens_details")
                            .and_then(cache_write_tokens_from_details)
                            .or_else(|| {
                                u.get("prompt_tokens_details")
                                    .and_then(cache_write_tokens_from_details)
                            })
                            .unwrap_or(0),
                        reasoning_tokens: u
                            .get("output_tokens_details")
                            .and_then(|details| details.get("reasoning_tokens"))
                            .and_then(serde_json::Value::as_u64)
                            .or_else(|| {
                                u.get("completion_tokens_details")
                                    .and_then(|details| details.get("reasoning_tokens"))
                                    .and_then(serde_json::Value::as_u64)
                            })
                            .unwrap_or(0),
                    })
                })
            });
            let mut events = Vec::new();
            if let Some(usage) = usage {
                events.push(ModelStreamEvent::Usage(usage));
            }
            events.push(ModelStreamEvent::Completed {
                response_id: event
                    .response
                    .as_ref()
                    .and_then(|r| r.get("id")?.as_str().map(String::from)),
            });
            Some(StreamEventBatch::Many(events))
        }

        "response.failed" => {
            let response = event.response.as_ref();
            Some(StreamEventBatch::Single(provider_failure_event(
                response.and_then(|response| response.get("error")),
                response.and_then(|response| response.get("code").and_then(|value| value.as_str())),
                response
                    .and_then(|response| response.get("message").and_then(|value| value.as_str())),
                response.and_then(provider_status),
                response.and_then(|response| {
                    response
                        .get("retry_after_ms")
                        .and_then(|value| value.as_u64())
                }),
                "response failed",
            )))
        }

        "response.incomplete" => {
            let response = event.response.as_ref();
            let reason = response
                .and_then(|response| response.get("incomplete_details")?.get("reason")?.as_str());
            Some(StreamEventBatch::Single(provider_failure_event(
                response.and_then(|response| response.get("error")),
                reason,
                None,
                response.and_then(provider_status),
                None,
                "response incomplete",
            )))
        }

        "error" => Some(StreamEventBatch::Single(provider_failure_event(
            event.error.as_ref(),
            event.code.as_deref(),
            event.message.as_deref(),
            event
                .status
                .as_ref()
                .and_then(provider_status_value)
                .or_else(|| event.status_code.as_ref().and_then(provider_status_value)),
            event.retry_after_ms,
            "provider stream failed",
        ))),

        _ => None,
    }
}

fn provider_failure_event(
    error: Option<&serde_json::Value>,
    fallback_code: Option<&str>,
    fallback_message: Option<&str>,
    fallback_status: Option<u16>,
    fallback_retry_after_ms: Option<u64>,
    default_message: &str,
) -> ModelStreamEvent {
    ModelStreamEvent::Failed {
        code: error
            .and_then(|error| error.get("code"))
            .and_then(serde_json::Value::as_str)
            .or(fallback_code)
            .map(ToString::to_string),
        http_status: error.and_then(provider_status).or(fallback_status),
        retry_after_ms: error
            .and_then(|error| error.get("retry_after_ms"))
            .and_then(serde_json::Value::as_u64)
            .or(fallback_retry_after_ms),
        message: error
            .and_then(|error| error.get("message"))
            .and_then(serde_json::Value::as_str)
            .or(fallback_message)
            .unwrap_or(default_message)
            .to_string(),
    }
}

fn provider_status(value: &serde_json::Value) -> Option<u16> {
    value
        .get("status")
        .or_else(|| value.get("status_code"))
        .and_then(provider_status_value)
}

fn provider_status_value(value: &serde_json::Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(|status| u16::try_from(status).ok())
        .or_else(|| value.as_str()?.parse().ok())
}

/// 解析 Responses 流事件携带的工具调用身份。
///
/// `item_id` 取事件 `item_id`（缺失时回落 `call_id`）；`call_id` 取事件
/// `call_id`，缺失时确定性赋 `item_id` 并记录——这是 late call_id 升级场景的
/// 确定性赋值，不是 optional 语义。两者都缺失由 accumulator 以协议错误拒绝。
fn responses_tool_identity(event: &SseStreamEvent) -> (String, String) {
    let item_id = event
        .item_id
        .as_deref()
        .filter(|item_id| !item_id.is_empty())
        .or_else(|| {
            event
                .call_id
                .as_deref()
                .filter(|call_id| !call_id.is_empty())
        })
        .unwrap_or_default()
        .to_string();
    let call_id = event
        .call_id
        .as_deref()
        .filter(|call_id| !call_id.is_empty())
        .map(String::from)
        .unwrap_or_else(|| {
            tracing::trace!(
                item_id = %item_id,
                kind = %event.kind,
                "responses tool event missing call_id; assigning item id"
            );
            item_id.clone()
        });
    (item_id, call_id)
}

#[cfg(test)]
mod unit_tests;
