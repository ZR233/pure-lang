use std::collections::HashMap;

use pl_trace::{
    AgentEvent, TraceDelta, TraceEvent, TraceEventKind, TracePart, TracePartDeltaEvent,
    TracePartKind, TracePartSource, TracePartStatus, TraceTextChannel, TraceThinkingChunk,
    TraceToolPart,
};

use crate::request::{CompletionTraceContext, ToolCall};

use super::tool_stream::ToolCallAccumulatorSnapshot;

pub(crate) struct TraceProjection {
    session_id: String,
    turn_id: String,
    inference_id: String,
    sequence: u64,
    started: HashMap<String, TracePart>,
    active_text_items: HashMap<String, String>,
    active_thinking_items: HashMap<String, String>,
    active_tool_items: HashMap<String, String>,
    segment_occurrences: HashMap<String, u64>,
    events: Vec<TraceEvent>,
}

impl TraceProjection {
    pub(crate) fn new(context: CompletionTraceContext) -> Self {
        Self {
            session_id: context.session_id,
            turn_id: context.turn_id,
            inference_id: context.inference_id,
            sequence: context.trace_sequence_base,
            started: HashMap::new(),
            active_text_items: HashMap::new(),
            active_thinking_items: HashMap::new(),
            active_tool_items: HashMap::new(),
            segment_occurrences: HashMap::new(),
            events: Vec::new(),
        }
    }

    pub(crate) fn events(&self) -> Vec<TraceEvent> {
        self.events.clone()
    }

    pub(crate) fn next_sequence(&self) -> u64 {
        self.sequence
    }

    pub(crate) fn start_text(
        &mut self,
        item_id: &str,
        text_channel: TraceTextChannel,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.active_text_item_id(item_id, text_channel);
        let mut events = Vec::new();
        if !self.started.contains_key(&item_id) {
            let item = TracePart {
                turn_id: self.turn_id.clone(),
                item_id: item_id.clone(),
                started_sequence: self.sequence,
                revision: 0,
                kind: TracePartKind::Text,
                status: TracePartStatus::Streaming,
                created_at: now,
                updated_at: now,
                source: TracePartSource::Model,
                text_channel: Some(text_channel),
                content: String::new(),
                attachments: Vec::new(),
                thinking_chunks: Vec::new(),
                tool: None,
                agent: None,
                inference: None,
                usage: None,
            };
            self.record(TraceEventKind::TracePartStarted { item: item.clone() }, now);
            events.push(AgentEvent::TracePartStarted { item: item.clone() });
            self.started.insert(item_id.clone(), item);
        }
        events
    }

    pub(crate) fn append_text_delta(
        &mut self,
        item_id: &str,
        text_channel: TraceTextChannel,
        delta: String,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let mut events = self.start_text(item_id, text_channel);
        let item_id = self.active_text_item_id(item_id, text_channel);
        if let Some(item) = self.started.get_mut(&item_id) {
            item.revision += 1;
            item.status = TracePartStatus::Streaming;
            item.updated_at = now;
            item.content.push_str(&delta);
        }
        let revision = self
            .started
            .get(&item_id)
            .map(|item| item.revision)
            .unwrap_or_default();
        let event = TracePartDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id,
            started_sequence: self.sequence,
            revision,
            kind: TracePartKind::Text,
            status: TracePartStatus::Streaming,
            created_at: now,
            updated_at: now,
            delta: TraceDelta::Text {
                text_channel,
                delta,
            },
        };
        self.record(
            TraceEventKind::TracePartDelta {
                event: event.clone(),
            },
            now,
        );
        events.push(AgentEvent::TracePartDelta { event });
        events
    }

    pub(crate) fn complete_text(
        &mut self,
        item_id: &str,
        text_channel: TraceTextChannel,
        authoritative_text: Option<String>,
    ) -> Vec<AgentEvent> {
        let key = text_provider_key(item_id, text_channel);
        let Some(item_id) = self.active_text_items.remove(&key) else {
            return Vec::new();
        };
        self.complete_item_by_resolved_id(
            &item_id,
            TracePartKind::Text,
            Some(text_channel),
            authoritative_text,
        )
    }

    pub(crate) fn start_thinking(&mut self, item_id: &str, chunk_index: u32) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.active_thinking_item_id(item_id, chunk_index);
        let mut events = Vec::new();
        if !self.started.contains_key(&item_id) {
            let item = TracePart {
                turn_id: self.turn_id.clone(),
                item_id: item_id.clone(),
                started_sequence: self.sequence,
                revision: 0,
                kind: TracePartKind::Thinking,
                status: TracePartStatus::Streaming,
                created_at: now,
                updated_at: now,
                source: TracePartSource::Model,
                text_channel: None,
                content: String::new(),
                attachments: Vec::new(),
                thinking_chunks: Vec::new(),
                tool: None,
                agent: None,
                inference: None,
                usage: None,
            };
            self.record(TraceEventKind::TracePartStarted { item: item.clone() }, now);
            events.push(AgentEvent::TracePartStarted { item: item.clone() });
            self.started.insert(item_id, item);
        }
        events
    }

    pub(crate) fn append_thinking_delta(
        &mut self,
        item_id: &str,
        chunk_index: u32,
        delta: String,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let mut events = self.start_thinking(item_id, chunk_index);
        let item_id = self.active_thinking_item_id(item_id, chunk_index);
        if let Some(item) = self.started.get_mut(&item_id) {
            item.revision += 1;
            item.status = TracePartStatus::Streaming;
            item.updated_at = now;
            match item
                .thinking_chunks
                .iter_mut()
                .find(|chunk| chunk.chunk_index == chunk_index)
            {
                Some(chunk) => chunk.content.push_str(&delta),
                None => item.thinking_chunks.push(TraceThinkingChunk {
                    chunk_index,
                    content: delta.clone(),
                }),
            }
            item.thinking_chunks.sort_by_key(|chunk| chunk.chunk_index);
        }
        let revision = self
            .started
            .get(&item_id)
            .map(|item| item.revision)
            .unwrap_or_default();
        let event = TracePartDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id,
            started_sequence: self.sequence,
            revision,
            kind: TracePartKind::Thinking,
            status: TracePartStatus::Streaming,
            created_at: now,
            updated_at: now,
            delta: TraceDelta::Thinking { chunk_index, delta },
        };
        self.record(
            TraceEventKind::TracePartDelta {
                event: event.clone(),
            },
            now,
        );
        events.push(AgentEvent::TracePartDelta { event });
        events
    }

    pub(crate) fn complete_thinking(
        &mut self,
        item_id: &str,
        authoritative_summary: Option<Vec<String>>,
    ) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        if let Some(summary) = authoritative_summary {
            for (index, text) in summary.into_iter().enumerate() {
                let chunk_index = index as u32;
                events.extend(self.start_thinking(item_id, chunk_index));
                let resolved_id = self.active_thinking_item_id(item_id, chunk_index);
                if let Some(item) = self.started.get_mut(&resolved_id) {
                    item.thinking_chunks = vec![TraceThinkingChunk {
                        chunk_index,
                        content: text,
                    }];
                    item.updated_at = unix_seconds();
                }
            }
        }
        let prefix = thinking_provider_key_prefix(item_id);
        let item_ids = self
            .active_thinking_items
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();
        let mut item_ids = item_ids;
        item_ids.sort();
        for key in item_ids {
            if let Some(item_id) = self.active_thinking_items.remove(&key) {
                events.extend(self.complete_item_by_resolved_id(
                    &item_id,
                    TracePartKind::Thinking,
                    None,
                    None,
                ));
            }
        }
        events
    }

    pub(crate) fn start_tool(&mut self, snapshot: &ToolCallAccumulatorSnapshot) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.active_tool_item_id(snapshot);
        if self.started.contains_key(&item_id) {
            return Vec::new();
        }
        let item = TracePart {
            turn_id: self.turn_id.clone(),
            item_id: item_id.clone(),
            started_sequence: self.sequence,
            revision: 0,
            kind: TracePartKind::Tool,
            status: TracePartStatus::Streaming,
            created_at: now,
            updated_at: now,
            source: TracePartSource::Model,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: Some(TraceToolPart {
                tool_call_id: item_id.clone(),
                call_id: snapshot.call_id.clone(),
                provider_item_id: (!snapshot.id.is_empty()).then(|| snapshot.id.clone()),
                name: snapshot.name.clone(),
                arguments: snapshot.arguments.clone(),
                result: None,
                exit_code: None,
                timed_out: false,
                working_directory: None,
                denial_reason: None,
            }),
            agent: None,
            inference: None,
            usage: None,
        };
        self.record(TraceEventKind::TracePartStarted { item: item.clone() }, now);
        self.started.insert(item_id, item.clone());
        vec![AgentEvent::TracePartStarted { item }]
    }

    pub(crate) fn append_tool_arguments_delta(
        &mut self,
        snapshot: &ToolCallAccumulatorSnapshot,
        delta: String,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.active_tool_item_id(snapshot);
        let mut events = self.start_tool(snapshot);
        if let Some(item) = self.started.get_mut(&item_id) {
            item.revision += 1;
            item.status = TracePartStatus::Streaming;
            item.updated_at = now;
            if let Some(tool) = &mut item.tool {
                tool.name = snapshot.name.clone();
                tool.arguments = snapshot.arguments.clone();
                tool.call_id = snapshot.call_id.clone();
                tool.provider_item_id = (!snapshot.id.is_empty()).then(|| snapshot.id.clone());
            }
        }
        let revision = self
            .started
            .get(&item_id)
            .map(|item| item.revision)
            .unwrap_or_default();
        let event = TracePartDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id,
            started_sequence: self.sequence,
            revision,
            kind: TracePartKind::Tool,
            status: TracePartStatus::Streaming,
            created_at: now,
            updated_at: now,
            delta: TraceDelta::ToolArguments { delta },
        };
        self.record(
            TraceEventKind::TracePartDelta {
                event: event.clone(),
            },
            now,
        );
        events.push(AgentEvent::TracePartDelta { event });
        events
    }

    pub(crate) fn complete_streaming_items(&mut self) -> Vec<AgentEvent> {
        let item_ids = self
            .started
            .iter()
            .filter(|(_, item)| {
                matches!(
                    item.kind,
                    TracePartKind::Text | TracePartKind::Thinking | TracePartKind::Plan
                )
            })
            .map(|(item_id, _)| item_id.clone())
            .collect::<Vec<_>>();
        let mut events = Vec::new();
        for item_id in item_ids {
            let Some(item) = self.started.get_mut(&item_id) else {
                continue;
            };
            if item.status == TracePartStatus::Completed {
                continue;
            }
            item.status = TracePartStatus::Completed;
            item.updated_at = unix_seconds();
            let item = item.clone();
            self.record(
                TraceEventKind::TracePartCompleted { item: item.clone() },
                item.updated_at,
            );
            events.push(AgentEvent::TracePartCompleted { item });
        }
        events
    }

    pub(crate) fn update_tool_trace(&mut self, call: &ToolCall) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.active_tool_call_item_id(call);
        let turn_id = self.turn_id.clone();
        let sequence = self.sequence;
        let mut inserted = false;
        let item = self.started.entry(item_id.clone()).or_insert_with(|| {
            inserted = true;
            TracePart {
                turn_id,
                item_id: item_id.clone(),
                started_sequence: sequence,
                revision: 0,
                kind: TracePartKind::Tool,
                status: TracePartStatus::Started,
                created_at: now,
                updated_at: now,
                source: TracePartSource::Model,
                text_channel: None,
                content: String::new(),
                attachments: Vec::new(),
                thinking_chunks: Vec::new(),
                tool: Some(TraceToolPart {
                    tool_call_id: item_id.clone(),
                    call_id: call.call_id.clone(),
                    provider_item_id: (!call.id.is_empty()).then(|| call.id.clone()),
                    name: call.name.clone(),
                    arguments: call.payload_text(),
                    result: None,
                    exit_code: None,
                    timed_out: false,
                    working_directory: None,
                    denial_reason: None,
                }),
                agent: None,
                inference: None,
                usage: None,
            }
        });
        if inserted {
            item.status = TracePartStatus::Started;
        }
        item.updated_at = now;
        if let Some(tool) = &mut item.tool {
            tool.tool_call_id = item_id.clone();
            tool.call_id = call.call_id.clone();
            tool.provider_item_id = Some(call.id.clone());
            tool.name = call.name.clone();
            tool.arguments = call.payload_text();
        }
        let item = item.clone();
        self.record(TraceEventKind::TracePartStarted { item: item.clone() }, now);
        vec![AgentEvent::TracePartStarted { item }]
    }

    fn namespaced_item_id(&self, item_id: &str) -> String {
        if item_id == self.turn_id || item_id.starts_with(&format!("{}-", self.turn_id)) {
            return item_id.to_string();
        }
        format!("{}-{item_id}", self.turn_id)
    }

    fn active_text_item_id(&mut self, provider_item_id: &str, channel: TraceTextChannel) -> String {
        let key = text_provider_key(provider_item_id, channel);
        if let Some(item_id) = self.active_text_items.get(&key) {
            return item_id.clone();
        }
        let item_id = self.next_segment_item_id(&format!("text-{}", channel.as_str()));
        self.active_text_items.insert(key, item_id.clone());
        item_id
    }

    fn active_thinking_item_id(&mut self, provider_item_id: &str, chunk_index: u32) -> String {
        let key = thinking_provider_key(provider_item_id, chunk_index);
        if let Some(item_id) = self.active_thinking_items.get(&key) {
            return item_id.clone();
        }
        let item_id = self.next_segment_item_id("reasoning");
        self.active_thinking_items.insert(key, item_id.clone());
        item_id
    }

    fn active_tool_item_id(&mut self, snapshot: &ToolCallAccumulatorSnapshot) -> String {
        let aliases = tool_aliases(snapshot.call_id.as_ref(), &snapshot.id, &snapshot.trace_id);
        self.resolve_tool_item_id(aliases, &snapshot.trace_id)
    }

    fn active_tool_call_item_id(&mut self, call: &ToolCall) -> String {
        let trace_id = trace_tool_part_id(call.call_id.as_ref(), &call.id);
        let aliases = tool_aliases(call.call_id.as_ref(), &call.id, &trace_id);
        self.resolve_tool_item_id(aliases, &trace_id)
    }

    fn resolve_tool_item_id(&mut self, aliases: Vec<String>, trace_id: &str) -> String {
        if let Some(item_id) = aliases
            .iter()
            .find_map(|alias| self.active_tool_items.get(alias).cloned())
        {
            for alias in aliases {
                self.active_tool_items
                    .entry(alias)
                    .or_insert_with(|| item_id.clone());
            }
            return item_id;
        }
        let item_id = self.namespaced_item_id(trace_id);
        for alias in aliases {
            self.active_tool_items.insert(alias, item_id.clone());
        }
        item_id
    }

    fn next_segment_item_id(&mut self, segment: &str) -> String {
        let occurrence = self
            .segment_occurrences
            .entry(segment.to_string())
            .or_insert(0);
        *occurrence += 1;
        format!("{}-{segment}-{}", self.inference_id, *occurrence)
    }

    fn complete_item_by_resolved_id(
        &mut self,
        item_id: &str,
        kind: TracePartKind,
        text_channel: Option<TraceTextChannel>,
        authoritative_text: Option<String>,
    ) -> Vec<AgentEvent> {
        let Some(item) = self.started.get_mut(item_id) else {
            return Vec::new();
        };
        if item.kind != kind
            || item.text_channel != text_channel
            || item.status == TracePartStatus::Completed
        {
            return Vec::new();
        }
        if let Some(text) = authoritative_text
            && item.content != text
        {
            item.content = text;
        }
        item.status = TracePartStatus::Completed;
        item.updated_at = unix_seconds();
        let item = item.clone();
        self.record(
            TraceEventKind::TracePartCompleted { item: item.clone() },
            item.updated_at,
        );
        vec![AgentEvent::TracePartCompleted { item }]
    }

    fn record(&mut self, kind: TraceEventKind, timestamp: i64) {
        self.events.push(TraceEvent {
            session_id: self.session_id.clone(),
            sequence: self.sequence,
            timestamp,
            kind,
        });
        self.sequence += 1;
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn text_provider_key(provider_item_id: &str, channel: TraceTextChannel) -> String {
    format!("text:{}:{provider_item_id}", channel.as_str())
}

fn thinking_provider_key(provider_item_id: &str, chunk_index: u32) -> String {
    format!(
        "{}:{chunk_index}",
        thinking_provider_key_prefix(provider_item_id)
    )
}

fn thinking_provider_key_prefix(provider_item_id: &str) -> String {
    format!("reasoning:{provider_item_id}:")
}

fn trace_tool_part_id(call_id: Option<&String>, id: &str) -> String {
    if !id.is_empty() {
        return id.to_string();
    }
    call_id
        .filter(|call_id| !call_id.is_empty())
        .cloned()
        .unwrap_or_else(|| "tool_call".to_string())
}

fn tool_aliases(call_id: Option<&String>, id: &str, trace_id: &str) -> Vec<String> {
    let mut aliases = Vec::new();
    push_tool_alias(&mut aliases, trace_id);
    push_tool_alias(&mut aliases, id);
    if let Some(call_id) = call_id {
        push_tool_alias(&mut aliases, call_id);
    }
    aliases
}

fn push_tool_alias(aliases: &mut Vec<String>, value: &str) {
    if !value.is_empty() && !aliases.iter().any(|alias| alias == value) {
        aliases.push(value.to_string());
    }
}

#[cfg(test)]
mod tests;
