use std::collections::HashMap;

use pl_trace::{
    AgentEvent, TraceDelta, TraceEvent, TraceEventKind, TracePart, TracePartDeltaEvent,
    TracePartKind, TracePartStatus, TraceTextChannel, TraceThinkingChunk, TraceToolPart,
};

use crate::request::{CompletionTraceContext, ToolCall};

use super::tool_stream::{ToolCallAccumulatorSnapshot, trace_tool_part_id};

pub(crate) struct TraceProjection {
    session_id: String,
    turn_id: String,
    inference_id: String,
    sequence: u64,
    started: HashMap<String, TracePart>,
    active_text_items: HashMap<String, String>,
    active_thinking_items: HashMap<String, String>,
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

    pub(crate) fn start_thinking(&mut self, item_id: &str) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.active_thinking_item_id(item_id);
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
        let mut events = self.start_thinking(item_id);
        let item_id = self.active_thinking_item_id(item_id);
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

    pub(crate) fn complete_thinking(&mut self, item_id: &str) -> Vec<AgentEvent> {
        let key = thinking_provider_key(item_id);
        let Some(item_id) = self.active_thinking_items.remove(&key) else {
            return Vec::new();
        };
        self.complete_item_by_resolved_id(&item_id, TracePartKind::Thinking, None, None)
    }

    pub(crate) fn start_tool(&mut self, snapshot: &ToolCallAccumulatorSnapshot) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id =
            self.namespaced_item_id(&trace_tool_part_id(snapshot.call_id.as_ref(), &snapshot.id));
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
        let item_id =
            self.namespaced_item_id(&trace_tool_part_id(snapshot.call_id.as_ref(), &snapshot.id));
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
        let item_id = self.namespaced_item_id(&trace_tool_part_id(call.call_id.as_ref(), &call.id));
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

    fn active_thinking_item_id(&mut self, provider_item_id: &str) -> String {
        let key = thinking_provider_key(provider_item_id);
        if let Some(item_id) = self.active_thinking_items.get(&key) {
            return item_id.clone();
        }
        let item_id = self.next_segment_item_id("reasoning");
        self.active_thinking_items.insert(key, item_id.clone());
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

fn thinking_provider_key(provider_item_id: &str) -> String {
    format!("reasoning:{provider_item_id}")
}

#[cfg(test)]
mod tests {
    use pl_trace::{AgentEvent, TracePart, TracePartKind};

    use super::*;
    use crate::{ToolCall, ToolCallPayload};

    fn trace() -> TraceProjection {
        TraceProjection::new(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inference-1".to_string(),
            plan_mode: false,
            trace_sequence_base: 0,
        })
    }

    #[test]
    fn repeated_provider_thinking_id_after_completion_gets_new_part_id() {
        let mut trace = trace();

        let first = trace.append_thinking_delta("thinking", 0, "first".to_string());
        let first_completed = trace.complete_thinking("thinking");
        let second = trace.append_thinking_delta("thinking", 0, "second".to_string());
        let second_completed = trace.complete_thinking("thinking");

        let first_delta = first
            .into_iter()
            .find_map(delta_item_id)
            .expect("first delta");
        let first_completed = first_completed
            .into_iter()
            .find_map(completed_item_id)
            .expect("first complete");
        let second_delta = second
            .into_iter()
            .find_map(delta_item_id)
            .expect("second delta");
        let second_completed = second_completed
            .into_iter()
            .find_map(completed_item_id)
            .expect("second complete");

        assert_eq!(first_delta, "inference-1-reasoning-1");
        assert_eq!(first_completed, first_delta);
        assert_eq!(second_delta, "inference-1-reasoning-2");
        assert_eq!(second_completed, second_delta);
    }

    #[test]
    fn generated_part_ids_are_scoped_to_inference() {
        let mut first = trace();
        let mut second = TraceProjection::new(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inference-2".to_string(),
            plan_mode: false,
            trace_sequence_base: 0,
        });

        let first_delta = first
            .append_thinking_delta("thinking", 0, "one".to_string())
            .into_iter()
            .find_map(delta_item_id)
            .expect("first delta");
        let second_delta = second
            .append_thinking_delta("thinking", 0, "two".to_string())
            .into_iter()
            .find_map(delta_item_id)
            .expect("second delta");

        assert_eq!(first_delta, "inference-1-reasoning-1");
        assert_eq!(second_delta, "inference-2-reasoning-1");
    }

    #[test]
    fn trace_sequence_base_offsets_started_sequence() {
        let mut first = TraceProjection::new(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-0".to_string(),
            plan_mode: false,
            trace_sequence_base: 10,
        });
        let mut second = TraceProjection::new(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-1".to_string(),
            plan_mode: false,
            trace_sequence_base: 20,
        });

        let first_sequence = first
            .start_thinking("thinking")
            .into_iter()
            .find_map(started_sequence)
            .expect("first started sequence");
        let second_sequence = second
            .start_thinking("thinking")
            .into_iter()
            .find_map(started_sequence)
            .expect("second started sequence");

        assert_eq!(first_sequence, 10);
        assert_eq!(second_sequence, 20);
    }

    #[test]
    fn completed_text_uses_authoritative_text_and_revision() {
        let mut trace = trace();
        let _ = trace.append_text_delta("msg_1", TraceTextChannel::Final, "par".to_string());
        let completed = trace
            .complete_text(
                "msg_1",
                TraceTextChannel::Final,
                Some("final text".to_string()),
            )
            .into_iter()
            .find_map(completed_text_item)
            .expect("completed text item");

        assert_eq!(completed.content, "final text");
        assert_eq!(completed.revision, 1);
    }

    #[test]
    fn update_tool_trace_keeps_streaming_tool_status_after_arguments_delta() {
        let mut trace = trace();
        let snapshot = ToolCallAccumulatorSnapshot {
            id: "provider-tool-1".to_string(),
            call_id: Some("call-1".to_string()),
            name: "bash".to_string(),
            arguments: "{\"cmd\":\"ec".to_string(),
        };
        let _ = trace.append_tool_arguments_delta(&snapshot, "{\"cmd\":\"ec".to_string());
        let updated = trace
            .update_tool_trace(&ToolCall {
                id: "provider-tool-1".to_string(),
                call_id: Some("call-1".to_string()),
                name: "bash".to_string(),
                payload: ToolCallPayload::Function {
                    arguments: serde_json::json!({"cmd": "echo hi"}),
                },
            })
            .into_iter()
            .find_map(started_tool_item)
            .expect("updated tool snapshot");

        assert_eq!(updated.item_id, "turn-1-provider-tool-1");
        assert_eq!(updated.status, TracePartStatus::Streaming);
        assert_eq!(updated.revision, 1);
        let tool = updated.tool.expect("tool metadata");
        assert_eq!(tool.arguments, "{\"cmd\":\"echo hi\"}");
    }

    fn started_sequence(event: AgentEvent) -> Option<u64> {
        match event {
            AgentEvent::TracePartStarted { item } => Some(item.started_sequence),
            AgentEvent::TracePartDelta { .. }
            | AgentEvent::TracePartCompleted { .. }
            | AgentEvent::TracePartFailed { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::AgentStateChanged { .. }
            | AgentEvent::CollabAgentSpawnBegin { .. }
            | AgentEvent::CollabAgentSpawnEnd { .. }
            | AgentEvent::CollabAgentInteractionBegin { .. }
            | AgentEvent::CollabAgentInteractionEnd { .. }
            | AgentEvent::CollabWaitingBegin { .. }
            | AgentEvent::CollabWaitingEnd { .. }
            | AgentEvent::CollabCloseBegin { .. }
            | AgentEvent::CollabCloseEnd { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::Done
            | AgentEvent::Error { .. } => None,
        }
    }

    fn delta_item_id(event: AgentEvent) -> Option<String> {
        match event {
            AgentEvent::TracePartDelta { event } if event.kind == TracePartKind::Thinking => {
                Some(event.item_id)
            }
            AgentEvent::TracePartStarted { .. }
            | AgentEvent::TracePartCompleted { .. }
            | AgentEvent::TracePartFailed { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::AgentStateChanged { .. }
            | AgentEvent::CollabAgentSpawnBegin { .. }
            | AgentEvent::CollabAgentSpawnEnd { .. }
            | AgentEvent::CollabAgentInteractionBegin { .. }
            | AgentEvent::CollabAgentInteractionEnd { .. }
            | AgentEvent::CollabWaitingBegin { .. }
            | AgentEvent::CollabWaitingEnd { .. }
            | AgentEvent::CollabCloseBegin { .. }
            | AgentEvent::CollabCloseEnd { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::Done
            | AgentEvent::Error { .. }
            | AgentEvent::TracePartDelta { .. } => None,
        }
    }

    fn completed_item_id(event: AgentEvent) -> Option<String> {
        match event {
            AgentEvent::TracePartCompleted { item } if item.kind == TracePartKind::Thinking => {
                Some(item.item_id)
            }
            AgentEvent::TracePartStarted { .. }
            | AgentEvent::TracePartDelta { .. }
            | AgentEvent::TracePartFailed { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::AgentStateChanged { .. }
            | AgentEvent::CollabAgentSpawnBegin { .. }
            | AgentEvent::CollabAgentSpawnEnd { .. }
            | AgentEvent::CollabAgentInteractionBegin { .. }
            | AgentEvent::CollabAgentInteractionEnd { .. }
            | AgentEvent::CollabWaitingBegin { .. }
            | AgentEvent::CollabWaitingEnd { .. }
            | AgentEvent::CollabCloseBegin { .. }
            | AgentEvent::CollabCloseEnd { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::Done
            | AgentEvent::Error { .. }
            | AgentEvent::TracePartCompleted { .. } => None,
        }
    }

    fn completed_text_item(event: AgentEvent) -> Option<TracePart> {
        match event {
            AgentEvent::TracePartCompleted { item }
                if item.kind == TracePartKind::Text
                    && item.text_channel == Some(TraceTextChannel::Final) =>
            {
                Some(item)
            }
            AgentEvent::TracePartStarted { .. }
            | AgentEvent::TracePartDelta { .. }
            | AgentEvent::TracePartFailed { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::AgentStateChanged { .. }
            | AgentEvent::CollabAgentSpawnBegin { .. }
            | AgentEvent::CollabAgentSpawnEnd { .. }
            | AgentEvent::CollabAgentInteractionBegin { .. }
            | AgentEvent::CollabAgentInteractionEnd { .. }
            | AgentEvent::CollabWaitingBegin { .. }
            | AgentEvent::CollabWaitingEnd { .. }
            | AgentEvent::CollabCloseBegin { .. }
            | AgentEvent::CollabCloseEnd { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::Done
            | AgentEvent::Error { .. }
            | AgentEvent::TracePartCompleted { .. } => None,
        }
    }

    fn started_tool_item(event: AgentEvent) -> Option<TracePart> {
        match event {
            AgentEvent::TracePartStarted { item } if item.kind == TracePartKind::Tool => Some(item),
            AgentEvent::TracePartDelta { .. }
            | AgentEvent::TracePartCompleted { .. }
            | AgentEvent::TracePartFailed { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::AgentStateChanged { .. }
            | AgentEvent::CollabAgentSpawnBegin { .. }
            | AgentEvent::CollabAgentSpawnEnd { .. }
            | AgentEvent::CollabAgentInteractionBegin { .. }
            | AgentEvent::CollabAgentInteractionEnd { .. }
            | AgentEvent::CollabWaitingBegin { .. }
            | AgentEvent::CollabWaitingEnd { .. }
            | AgentEvent::CollabCloseBegin { .. }
            | AgentEvent::CollabCloseEnd { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::Done
            | AgentEvent::Error { .. }
            | AgentEvent::TracePartStarted { .. } => None,
        }
    }
}
