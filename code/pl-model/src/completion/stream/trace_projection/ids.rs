//! trace part item id 的解析、别名收敛与段落编号。

use pl_trace::{
    AgentEvent, TraceEventKind, TracePartAction, TracePartCompletion, TracePartKind,
    TracePartState, TraceTextChannel,
};

use crate::completion::ToolCall;

use super::super::tool_stream::ToolCallAccumulatorSnapshot;
use super::TraceProjection;
use super::text::{text_provider_key, thinking_provider_key_prefix};
use super::tool::{tool_aliases, trace_tool_part_id};
use super::unix_seconds;

impl TraceProjection {
    fn namespaced_item_id(&self, item_id: &str) -> String {
        let turn_id = &self.turn_id;
        if item_id == self.turn_id || item_id.starts_with(&format!("{turn_id}-")) {
            return item_id.to_string();
        }
        format!("{turn_id}-{item_id}")
    }

    pub(super) fn active_text_item_id(
        &mut self,
        provider_item_id: &str,
        channel: TraceTextChannel,
    ) -> String {
        let key = text_provider_key(provider_item_id, channel);
        if let Some(item_id) = self.active_text_items.get(&key) {
            return item_id.clone();
        }
        let channel_str = channel.as_str();
        let item_id = self.next_segment_item_id(&format!("text-{channel_str}"));
        self.active_text_items.insert(key, item_id.clone());
        item_id
    }

    pub(super) fn active_thinking_item_id(
        &mut self,
        provider_item_id: &str,
        chunk_index: u32,
    ) -> String {
        let key = thinking_provider_key(provider_item_id, chunk_index);
        if let Some(item_id) = self.active_thinking_items.get(&key) {
            return item_id.clone();
        }
        let item_id = self.next_segment_item_id("reasoning");
        self.active_thinking_items.insert(key, item_id.clone());
        item_id
    }

    pub(super) fn active_tool_item_id(&mut self, snapshot: &ToolCallAccumulatorSnapshot) -> String {
        let aliases = tool_aliases(snapshot.call_id.as_ref(), &snapshot.id, &snapshot.trace_id);
        self.resolve_tool_item_id(aliases, &snapshot.trace_id)
    }

    pub(super) fn active_tool_call_item_id(&mut self, call: &ToolCall) -> String {
        let trace_id = trace_tool_part_id(Some(&call.call_id), &call.id);
        let aliases = tool_aliases(Some(&call.call_id), &call.id, &trace_id);
        self.resolve_tool_item_id(aliases, &trace_id)
    }

    pub(super) fn resolve_tool_item_id(&mut self, aliases: Vec<String>, trace_id: &str) -> String {
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
        let inference_id = &self.inference_id;
        let occ = *occurrence;
        format!("{inference_id}-{segment}-{occ}")
    }

    pub(super) fn complete_item_by_resolved_id(
        &mut self,
        item_id: &str,
        kind: TracePartKind,
        completion: TracePartCompletion,
    ) -> Vec<AgentEvent> {
        let Some(item) = self.started.get_mut(item_id) else {
            return Vec::new();
        };
        if item.kind() != kind || item.is_terminal() {
            return Vec::new();
        }
        if let (TracePartState::Text(part), TracePartCompletion::Text { .. }) =
            (item.state(), &completion)
            && kind == TracePartKind::Text
            && !matches!(
                part.channel(),
                TraceTextChannel::User | TraceTextChannel::Commentary | TraceTextChannel::Final
            )
        {
            return Vec::new();
        }
        let now = unix_seconds();
        if let Err(error) = item.apply(item.command(now, TracePartAction::Complete(completion))) {
            tracing::error!(%error, "failed to complete resolved trace item");
            return Vec::new();
        }
        let item = item.clone();
        self.record(
            TraceEventKind::TracePartCompleted { item: item.clone() },
            item.updated_at(),
        );
        vec![AgentEvent::TracePartCompleted { item }]
    }
}

fn thinking_provider_key(provider_item_id: &str, chunk_index: u32) -> String {
    format!(
        "{}{chunk_index}",
        thinking_provider_key_prefix(provider_item_id)
    )
}
