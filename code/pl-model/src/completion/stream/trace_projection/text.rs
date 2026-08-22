//! 正文与 reasoning 流的 trace part 投影。

use pl_trace::{
    AgentEvent, TraceDelta, TraceEventKind, TracePart, TracePartAction, TracePartCompletion,
    TracePartKind, TraceTextChannel,
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
            let item = TracePart::streaming_text(
                self.turn_id.clone(),
                item_id.clone(),
                self.sequence,
                text_channel,
                now,
            );
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
        let trace_delta = TraceDelta::Text {
            channel: text_channel,
            delta,
        };
        let Some(item) = self.started.get_mut(&item_id) else {
            return events;
        };
        if let Err(error) =
            item.apply(item.command(now, TracePartAction::Append(trace_delta.clone())))
        {
            tracing::error!(%error, "failed to append text trace delta");
            return events;
        }
        let Ok(event) = item.delta_event(trace_delta) else {
            return events;
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
            TracePartCompletion::Text {
                authoritative_content: authoritative_text,
            },
        )
    }

    pub(crate) fn start_thinking(&mut self, item_id: &str, chunk_index: u32) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.active_thinking_item_id(item_id, chunk_index);
        let mut events = Vec::new();
        if !self.started.contains_key(&item_id) {
            let item = TracePart::streaming_thinking(
                self.turn_id.clone(),
                item_id.clone(),
                self.sequence,
                now,
            );
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
        let trace_delta = TraceDelta::Thinking { chunk_index, delta };
        let Some(item) = self.started.get_mut(&item_id) else {
            return events;
        };
        if let Err(error) =
            item.apply(item.command(now, TracePartAction::Append(trace_delta.clone())))
        {
            tracing::error!(%error, "failed to append thinking trace delta");
            return events;
        }
        let Ok(event) = item.delta_event(trace_delta) else {
            return events;
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
        let trace_delta = TraceDelta::ReasoningContent { chunk_index, delta };
        let Some(item) = self.started.get_mut(&item_id) else {
            return events;
        };
        if let Err(error) =
            item.apply(item.command(now, TracePartAction::Append(trace_delta.clone())))
        {
            tracing::error!(%error, "failed to append reasoning content trace delta");
            return events;
        }
        let Ok(event) = item.delta_event(trace_delta) else {
            return events;
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
        if let Some(summary) = authoritative_summary.as_ref() {
            for (index, _text) in summary.iter().enumerate() {
                let chunk_index = index as u32;
                events.extend(self.start_thinking(item_id, chunk_index));
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
                let authoritative_summary = authoritative_summary.as_ref().and_then(|summary| {
                    key.strip_prefix(&prefix)
                        .and_then(|index| index.parse::<usize>().ok())
                        .and_then(|index| summary.get(index))
                        .map(|text| vec![text.clone()])
                });
                events.extend(self.complete_item_by_resolved_id(
                    &item_id,
                    TracePartKind::Thinking,
                    TracePartCompletion::Thinking {
                        authoritative_summary,
                    },
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
