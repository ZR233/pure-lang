//! 正文与 reasoning 流的 trace part 投影。

use pl_trace::{
    AgentEvent, TraceDelta, TraceEventKind, TracePart, TracePartDeltaEvent, TracePartKind,
    TracePartSource, TracePartStatus, TraceTextChannel, TraceThinkingChunk,
};

use super::TraceProjection;
use super::unix_seconds;

impl TraceProjection {
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
                reasoning_content_chunks: Vec::new(),
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
                reasoning_content_chunks: Vec::new(),
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

    pub(crate) fn append_reasoning_content_delta(
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
                .reasoning_content_chunks
                .iter_mut()
                .find(|chunk| chunk.chunk_index == chunk_index)
            {
                Some(chunk) => chunk.content.push_str(&delta),
                None => item.reasoning_content_chunks.push(TraceThinkingChunk {
                    chunk_index,
                    content: delta.clone(),
                }),
            }
            item.reasoning_content_chunks
                .sort_by_key(|chunk| chunk.chunk_index);
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
            delta: TraceDelta::ReasoningContent { chunk_index, delta },
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
}

pub(super) fn text_provider_key(provider_item_id: &str, channel: TraceTextChannel) -> String {
    let channel_str = channel.as_str();
    format!("text:{channel_str}:{provider_item_id}")
}

pub(super) fn thinking_provider_key_prefix(provider_item_id: &str) -> String {
    format!("reasoning:{provider_item_id}:")
}
