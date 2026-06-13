use std::collections::HashMap;

use pl_protocol::{
    AgentEvent, TimelineDelta, TimelineItem, TimelineItemDeltaEvent, TimelineItemKind,
    TimelineItemStatus, TimelineTextChannel, TimelineThinkingChunk, TimelineToolItem, TraceEvent,
    TraceEventKind,
};

use crate::request::{CompletionTimelineContext, ToolCall};

use super::tool_stream::{ToolCallAccumulatorSnapshot, timeline_tool_item_id};

pub(crate) struct TimelineProjection {
    session_id: String,
    turn_id: String,
    sequence: u64,
    started: HashMap<String, TimelineItem>,
    events: Vec<TraceEvent>,
}

impl TimelineProjection {
    pub(crate) fn new(context: CompletionTimelineContext) -> Self {
        Self {
            session_id: context.session_id,
            turn_id: context.turn_id,
            sequence: context.starting_sequence,
            started: HashMap::new(),
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
        text_channel: TimelineTextChannel,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.namespaced_item_id(item_id);
        let mut events = Vec::new();
        if !self.started.contains_key(&item_id) {
            let item = TimelineItem {
                turn_id: self.turn_id.clone(),
                item_id: item_id.clone(),
                sequence: self.sequence,
                kind: TimelineItemKind::Text,
                status: TimelineItemStatus::Streaming,
                created_at: now,
                updated_at: now,
                text_channel: Some(text_channel),
                content: String::new(),
                attachments: Vec::new(),
                thinking_chunks: Vec::new(),
                tool: None,
                agent: None,
                inference: None,
                usage: None,
            };
            self.record(
                TraceEventKind::TimelineItemStarted { item: item.clone() },
                now,
            );
            events.push(AgentEvent::TimelineItemStarted { item: item.clone() });
            self.started.insert(item_id.clone(), item);
        }
        events
    }

    pub(crate) fn append_text_delta(
        &mut self,
        item_id: &str,
        text_channel: TimelineTextChannel,
        delta: String,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let mut events = self.start_text(item_id, text_channel);
        let item_id = self.namespaced_item_id(item_id);
        if let Some(item) = self.started.get_mut(&item_id) {
            item.status = TimelineItemStatus::Streaming;
            item.updated_at = now;
            item.content.push_str(&delta);
        }
        let event = TimelineItemDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id,
            sequence: self.sequence,
            kind: TimelineItemKind::Text,
            status: TimelineItemStatus::Streaming,
            created_at: now,
            updated_at: now,
            delta: TimelineDelta::Text {
                text_channel,
                delta,
            },
        };
        self.record(
            TraceEventKind::TimelineItemDelta {
                event: event.clone(),
            },
            now,
        );
        events.push(AgentEvent::TimelineItemDelta { event });
        events
    }

    pub(crate) fn complete_text(
        &mut self,
        item_id: &str,
        text_channel: TimelineTextChannel,
    ) -> Vec<AgentEvent> {
        self.complete_item(item_id, TimelineItemKind::Text, Some(text_channel))
    }

    pub(crate) fn start_plan(&mut self, item_id: &str) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.plan_item_id(item_id);
        if self.started.contains_key(&item_id) {
            return Vec::new();
        }
        let item = TimelineItem {
            turn_id: self.turn_id.clone(),
            item_id: item_id.clone(),
            sequence: self.sequence,
            kind: TimelineItemKind::Plan,
            status: TimelineItemStatus::Streaming,
            created_at: now,
            updated_at: now,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: None,
            agent: None,
            inference: None,
            usage: None,
        };
        self.record(
            TraceEventKind::TimelineItemStarted { item: item.clone() },
            now,
        );
        self.started.insert(item_id, item.clone());
        vec![AgentEvent::TimelineItemStarted { item }]
    }

    pub(crate) fn append_plan_delta(&mut self, item_id: &str, delta: String) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let mut events = self.start_plan(item_id);
        let item_id = self.plan_item_id(item_id);
        if let Some(item) = self.started.get_mut(&item_id) {
            item.status = TimelineItemStatus::Streaming;
            item.updated_at = now;
            item.content.push_str(&delta);
        }
        let event = TimelineItemDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id,
            sequence: self.sequence,
            kind: TimelineItemKind::Plan,
            status: TimelineItemStatus::Streaming,
            created_at: now,
            updated_at: now,
            delta: TimelineDelta::Plan { delta },
        };
        self.record(
            TraceEventKind::TimelineItemDelta {
                event: event.clone(),
            },
            now,
        );
        events.push(AgentEvent::TimelineItemDelta { event });
        events
    }

    pub(crate) fn complete_plan(&mut self, item_id: &str) -> Vec<AgentEvent> {
        let item_id = self.plan_item_id(item_id);
        self.complete_item_by_resolved_id(&item_id, TimelineItemKind::Plan, None)
    }

    pub(crate) fn start_thinking(&mut self, item_id: &str) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.namespaced_item_id(item_id);
        let mut events = Vec::new();
        if !self.started.contains_key(&item_id) {
            let item = TimelineItem {
                turn_id: self.turn_id.clone(),
                item_id: item_id.clone(),
                sequence: self.sequence,
                kind: TimelineItemKind::Thinking,
                status: TimelineItemStatus::Streaming,
                created_at: now,
                updated_at: now,
                text_channel: None,
                content: String::new(),
                attachments: Vec::new(),
                thinking_chunks: Vec::new(),
                tool: None,
                agent: None,
                inference: None,
                usage: None,
            };
            self.record(
                TraceEventKind::TimelineItemStarted { item: item.clone() },
                now,
            );
            events.push(AgentEvent::TimelineItemStarted { item: item.clone() });
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
        let mut events = self.start_thinking(item_id);
        let item_id = self.namespaced_item_id(item_id);
        if let Some(item) = self.started.get_mut(&item_id) {
            item.status = TimelineItemStatus::Streaming;
            item.updated_at = now;
            match item
                .thinking_chunks
                .iter_mut()
                .find(|chunk| chunk.chunk_index == chunk_index)
            {
                Some(chunk) => chunk.content.push_str(&delta),
                None => item.thinking_chunks.push(TimelineThinkingChunk {
                    chunk_index,
                    content: delta.clone(),
                }),
            }
            item.thinking_chunks.sort_by_key(|chunk| chunk.chunk_index);
        }
        let event = TimelineItemDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id,
            sequence: self.sequence,
            kind: TimelineItemKind::Thinking,
            status: TimelineItemStatus::Streaming,
            created_at: now,
            updated_at: now,
            delta: TimelineDelta::Thinking { chunk_index, delta },
        };
        self.record(
            TraceEventKind::TimelineItemDelta {
                event: event.clone(),
            },
            now,
        );
        events.push(AgentEvent::TimelineItemDelta { event });
        events
    }

    pub(crate) fn complete_thinking(&mut self, item_id: &str) -> Vec<AgentEvent> {
        self.complete_item(item_id, TimelineItemKind::Thinking, None)
    }

    pub(crate) fn start_tool(&mut self, snapshot: &ToolCallAccumulatorSnapshot) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.namespaced_item_id(&timeline_tool_item_id(
            snapshot.call_id.as_ref(),
            &snapshot.id,
        ));
        if self.started.contains_key(&item_id) {
            return Vec::new();
        }
        let item = TimelineItem {
            turn_id: self.turn_id.clone(),
            item_id: item_id.clone(),
            sequence: self.sequence,
            kind: TimelineItemKind::Tool,
            status: TimelineItemStatus::Streaming,
            created_at: now,
            updated_at: now,
            text_channel: None,
            content: String::new(),
            attachments: Vec::new(),
            thinking_chunks: Vec::new(),
            tool: Some(TimelineToolItem {
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
        self.record(
            TraceEventKind::TimelineItemStarted { item: item.clone() },
            now,
        );
        self.started.insert(item_id, item.clone());
        vec![AgentEvent::TimelineItemStarted { item }]
    }

    pub(crate) fn append_tool_arguments_delta(
        &mut self,
        snapshot: &ToolCallAccumulatorSnapshot,
        delta: String,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.namespaced_item_id(&timeline_tool_item_id(
            snapshot.call_id.as_ref(),
            &snapshot.id,
        ));
        let mut events = self.start_tool(snapshot);
        if let Some(item) = self.started.get_mut(&item_id) {
            item.status = TimelineItemStatus::Streaming;
            item.updated_at = now;
            if let Some(tool) = &mut item.tool {
                tool.name = snapshot.name.clone();
                tool.arguments = snapshot.arguments.clone();
                tool.call_id = snapshot.call_id.clone();
                tool.provider_item_id = (!snapshot.id.is_empty()).then(|| snapshot.id.clone());
            }
        }
        let event = TimelineItemDeltaEvent {
            turn_id: self.turn_id.clone(),
            item_id,
            sequence: self.sequence,
            kind: TimelineItemKind::Tool,
            status: TimelineItemStatus::Streaming,
            created_at: now,
            updated_at: now,
            delta: TimelineDelta::ToolArguments { delta },
        };
        self.record(
            TraceEventKind::TimelineItemDelta {
                event: event.clone(),
            },
            now,
        );
        events.push(AgentEvent::TimelineItemDelta { event });
        events
    }

    pub(crate) fn complete_streaming_items(&mut self) -> Vec<AgentEvent> {
        let item_ids = self
            .started
            .iter()
            .filter(|(_, item)| {
                matches!(
                    item.kind,
                    TimelineItemKind::Text | TimelineItemKind::Thinking | TimelineItemKind::Plan
                )
            })
            .map(|(item_id, _)| item_id.clone())
            .collect::<Vec<_>>();
        let mut events = Vec::new();
        for item_id in item_ids {
            let Some(item) = self.started.get_mut(&item_id) else {
                continue;
            };
            if item.status == TimelineItemStatus::Completed {
                continue;
            }
            item.status = TimelineItemStatus::Completed;
            item.updated_at = unix_seconds();
            let item = item.clone();
            let sequence = self.sequence;
            self.record(
                TraceEventKind::TimelineItemCompleted { item: item.clone() },
                item.updated_at,
            );
            events.push(AgentEvent::TimelineItemCompleted { sequence, item });
        }
        events
    }

    pub(crate) fn update_tool_trace_only(&mut self, call: &ToolCall) {
        let item_id =
            self.namespaced_item_id(&timeline_tool_item_id(call.call_id.as_ref(), &call.id));
        let Some(item) = self.started.get_mut(&item_id) else {
            return;
        };
        item.status = TimelineItemStatus::Started;
        item.updated_at = unix_seconds();
        if let Some(tool) = &mut item.tool {
            tool.tool_call_id = item_id;
            tool.call_id = call.call_id.clone();
            tool.provider_item_id = Some(call.id.clone());
            tool.name = call.name.clone();
            tool.arguments = call.payload_text();
        }
    }

    fn plan_item_id(&self, item_id: &str) -> String {
        let item_id = if item_id.is_empty() { "plan" } else { item_id };
        self.namespaced_item_id(item_id)
    }

    fn namespaced_item_id(&self, item_id: &str) -> String {
        if item_id.starts_with(&self.turn_id) {
            return item_id.to_string();
        }
        format!("{}-{item_id}", self.turn_id)
    }

    fn complete_item(
        &mut self,
        item_id: &str,
        kind: TimelineItemKind,
        text_channel: Option<TimelineTextChannel>,
    ) -> Vec<AgentEvent> {
        let item_id = self.namespaced_item_id(item_id);
        self.complete_item_by_resolved_id(&item_id, kind, text_channel)
    }

    fn complete_item_by_resolved_id(
        &mut self,
        item_id: &str,
        kind: TimelineItemKind,
        text_channel: Option<TimelineTextChannel>,
    ) -> Vec<AgentEvent> {
        let Some(item) = self.started.get_mut(item_id) else {
            return Vec::new();
        };
        if item.kind != kind
            || item.text_channel != text_channel
            || item.status == TimelineItemStatus::Completed
        {
            return Vec::new();
        }
        item.status = TimelineItemStatus::Completed;
        item.updated_at = unix_seconds();
        let item = item.clone();
        let sequence = self.sequence;
        self.record(
            TraceEventKind::TimelineItemCompleted { item: item.clone() },
            item.updated_at,
        );
        vec![AgentEvent::TimelineItemCompleted { sequence, item }]
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
