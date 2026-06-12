use std::collections::HashMap;

use async_openai::types::stream::StreamResponse;
use futures::StreamExt;
use pl_protocol::{
    AgentEvent, AgentEventSender, PureError, Result, TimelineDelta, TimelineItem,
    TimelineItemDeltaEvent, TimelineItemKind, TimelineItemStatus, TimelineTextChannel,
    TimelineThinkingChunk, TimelineToolItem, TraceEvent, TraceEventKind,
};

use crate::proposed_plan::{VisibleTextParser, VisibleTextSegment};
use crate::protocol::openai::OpenAiProtocol;
use crate::protocol::openai::sse::{self, StreamEvent, ToolCallDeltaPayload};
use crate::request::{
    CompletionResponse, CompletionTimelineContext, FinishReason, TokenUsage, ToolCall,
};
pub(crate) async fn process_provider_stream(
    stream: StreamResponse<sse::SseStreamEvent>,
    event_tx: &AgentEventSender,
    protocol: &OpenAiProtocol,
    timeline: Option<CompletionTimelineContext>,
) -> Result<CompletionResponse> {
    let mut accumulator = StreamCompletionAccumulator::new(timeline);

    let mut stream = std::pin::pin!(stream);

    while let Some(event) = stream.next().await {
        let sse_event = match event {
            Ok(e) => e,
            Err(e) => {
                return Err(PureError::LlmError(format!("provider stream error: {e}")));
            }
        };

        let stream_event = match protocol.parse_stream_event(&sse_event)? {
            Some(e) => e,
            None => continue,
        };

        accumulator.apply(stream_event, event_tx)?;
    }

    accumulator.finish(event_tx)
}

pub(crate) struct StreamCompletionAccumulator {
    content_parts: Vec<String>,
    raw_content_parts: Vec<String>,
    reasoning_parts: Vec<String>,
    tool_calls: Vec<ToolCall>,
    tool_call_accumulators: HashMap<String, ToolCallAccumulator>,
    final_usage: Option<TokenUsage>,
    completed: bool,
    timeline: Option<TimelineState>,
    commentary_item_id: Option<String>,
    final_item_id: Option<String>,
    thinking_item_id: Option<String>,
    text_parser: VisibleTextParser,
    saw_tagged_content: bool,
    untagged_content: String,
}

impl StreamCompletionAccumulator {
    pub(crate) fn new(timeline: Option<CompletionTimelineContext>) -> Self {
        let plan_mode = timeline.as_ref().is_some_and(|context| context.plan_mode);
        Self {
            content_parts: Vec::new(),
            raw_content_parts: Vec::new(),
            reasoning_parts: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_accumulators: HashMap::new(),
            final_usage: None,
            completed: false,
            timeline: timeline.map(TimelineState::new),
            commentary_item_id: None,
            final_item_id: None,
            thinking_item_id: None,
            text_parser: VisibleTextParser::new(plan_mode),
            saw_tagged_content: false,
            untagged_content: String::new(),
        }
    }

    pub(crate) fn apply(
        &mut self,
        stream_event: StreamEvent,
        event_tx: &AgentEventSender,
    ) -> Result<()> {
        match stream_event {
            StreamEvent::OutputTextDelta { item_id, delta } => {
                self.raw_content_parts.push(delta.clone());
                let segments = self.text_parser.push_str(&delta).segments;
                self.apply_visible_text_segments(item_id, segments, event_tx);
            }

            StreamEvent::ThinkingDelta {
                item_id,
                chunk_index,
                delta,
            } => {
                self.reasoning_parts.push(delta.clone());
                self.record_thinking_delta(item_id, chunk_index, delta, event_tx);
            }

            StreamEvent::ToolCallDelta {
                stream_id,
                item_id,
                call_id,
                name,
                payload_delta,
            } => {
                let key = tool_call_accumulator_key(stream_id.as_ref(), call_id.as_ref(), &item_id);
                let initial_id = if item_id.is_empty() {
                    key.clone()
                } else {
                    item_id.clone()
                };
                let initial_call_id = call_id
                    .as_ref()
                    .filter(|call_id| !call_id.is_empty())
                    .cloned();
                let (snapshot, delta_text) = {
                    let acc = self
                        .tool_call_accumulators
                        .entry(key.clone())
                        .or_insert_with(|| ToolCallAccumulator {
                            id: initial_id,
                            has_stable_id: !item_id.is_empty(),
                            call_id: initial_call_id,
                            name: String::new(),
                            payload: ToolCallPayloadAccumulator::FunctionArguments(String::new()),
                        });
                    acc.merge_metadata(&key, &item_id, call_id.as_ref(), name);
                    let delta_text = payload_delta.text().to_string();
                    acc.push_delta(payload_delta);
                    (acc.snapshot(), delta_text)
                };
                self.record_tool_delta(&snapshot, delta_text, event_tx);
            }

            StreamEvent::OutputItemDone(value) => {
                if let Some(func_call) = value.get("type").and_then(|t| t.as_str())
                    && func_call == "function_call"
                {
                    let mut call_id = value_string(&value, "call_id");
                    let mut id = value_string(&value, "id");
                    let lookup_id = id.as_deref().unwrap_or_default();
                    let acc = self.take_tool_call_accumulator(None, call_id.as_ref(), lookup_id);
                    if id.is_none() {
                        id = acc
                            .as_ref()
                            .map(|acc| acc.id.clone())
                            .or_else(|| call_id.clone());
                    }
                    if call_id.is_none() {
                        call_id = acc.as_ref().and_then(|acc| acc.call_id.clone());
                    }
                    let name = value_string(&value, "name")
                        .or_else(|| acc.as_ref().and_then(ToolCallAccumulator::name))
                        .unwrap_or_default();
                    let arguments = value_string(&value, "arguments")
                        .or_else(|| {
                            acc.as_ref()
                                .and_then(ToolCallAccumulator::function_arguments)
                        })
                        .unwrap_or_default();
                    if name.is_empty() {
                        return Err(PureError::LlmError(
                            "provider emitted tool call without name".to_string(),
                        ));
                    }
                    if id.as_deref().is_none_or(str::is_empty)
                        && call_id.as_deref().is_none_or(str::is_empty)
                    {
                        return Err(PureError::LlmError(
                            "provider emitted tool call without stable id".to_string(),
                        ));
                    }
                    let id = id.unwrap_or_default();
                    let call = ToolCall::function(
                        id,
                        name,
                        serde_json::from_str(&arguments)
                            .unwrap_or(serde_json::Value::String(arguments)),
                        call_id,
                    );
                    self.complete_tool_item(&call, event_tx);

                    self.tool_calls.push(call);
                } else if let Some(custom_call) = value.get("type").and_then(|t| t.as_str())
                    && custom_call == "custom_tool_call"
                {
                    let mut call_id = value_string(&value, "call_id");
                    let mut id = value_string(&value, "id");
                    let lookup_id = id.as_deref().unwrap_or_default();
                    let acc = self.take_tool_call_accumulator(None, call_id.as_ref(), lookup_id);
                    if id.is_none() {
                        id = acc
                            .as_ref()
                            .map(|acc| acc.id.clone())
                            .or_else(|| call_id.clone());
                    }
                    if call_id.is_none() {
                        call_id = acc.as_ref().and_then(|acc| acc.call_id.clone());
                    }
                    let name = value_string(&value, "name")
                        .or_else(|| acc.as_ref().and_then(ToolCallAccumulator::name))
                        .unwrap_or_default();
                    let input = value_string(&value, "input")
                        .or_else(|| acc.as_ref().and_then(ToolCallAccumulator::custom_input))
                        .unwrap_or_default();
                    if name.is_empty() {
                        return Err(PureError::LlmError(
                            "provider emitted tool call without name".to_string(),
                        ));
                    }
                    if id.as_deref().is_none_or(str::is_empty)
                        && call_id.as_deref().is_none_or(str::is_empty)
                    {
                        return Err(PureError::LlmError(
                            "provider emitted tool call without stable id".to_string(),
                        ));
                    }
                    let id = id.unwrap_or_default();
                    let call = ToolCall::custom(id, name, input, call_id);
                    self.complete_tool_item(&call, event_tx);

                    self.tool_calls.push(call);
                }
            }

            StreamEvent::Completed { usage, response_id } => {
                self.final_usage = usage;
                self.completed = true;
                let _ = response_id;
            }

            StreamEvent::Failed { message, .. } => {
                return Err(PureError::LlmError(message));
            }

            StreamEvent::Created => {}
        }

        Ok(())
    }

    fn take_tool_call_accumulator(
        &mut self,
        stream_id: Option<&String>,
        call_id: Option<&String>,
        item_id: &str,
    ) -> Option<ToolCallAccumulator> {
        let key = tool_call_accumulator_key(stream_id, call_id, item_id);
        if self.tool_call_accumulators.contains_key(&key) {
            return self.tool_call_accumulators.remove(&key);
        }

        let fallback_key = self
            .tool_call_accumulators
            .iter()
            .find_map(|(key, accumulator)| {
                let call_id_matches = call_id
                    .filter(|call_id| !call_id.is_empty())
                    .zip(accumulator.call_id.as_ref())
                    .is_some_and(|(left, right)| left == right);
                let item_id_matches = !item_id.is_empty() && accumulator.id == item_id;
                (call_id_matches || item_id_matches).then(|| key.clone())
            });
        fallback_key.and_then(|key| self.tool_call_accumulators.remove(&key))
    }

    pub(crate) fn finish(mut self, event_tx: &AgentEventSender) -> Result<CompletionResponse> {
        if !self.completed {
            return Err(PureError::LlmError(
                "provider stream ended before completion".to_string(),
            ));
        }
        let segments = self.text_parser.finish().segments;
        self.apply_visible_text_segments(None, segments, event_tx);
        if self.timeline.is_some() && !self.untagged_content.trim().is_empty() {
            return Err(PureError::LlmError(
                "provider emitted untagged assistant text; expected <commentary>, <final>, or <proposed_plan>".to_string(),
            ));
        }
        // 合并累积的工具调用（如果有 delta 但没有 output_item.done）
        let remaining_accumulators = std::mem::take(&mut self.tool_call_accumulators);
        for (_, acc) in remaining_accumulators {
            if !self.tool_calls.iter().any(|tc| tc.id == acc.id) {
                let call = acc.into_tool_call()?;
                self.complete_tool_item_without_broadcast(&call);
                self.tool_calls.push(call);
            }
        }
        if let Some(timeline) = self.timeline.as_mut() {
            for event in timeline.complete_streaming_items() {
                let _ = event_tx.send(event);
            }
        }

        let content = if self.content_parts.is_empty() {
            None
        } else {
            Some(self.content_parts.join(""))
        };
        let raw_content = if self.raw_content_parts.is_empty() {
            None
        } else {
            Some(self.raw_content_parts.join(""))
        };

        let reasoning_content = if self.reasoning_parts.is_empty() {
            None
        } else {
            Some(self.reasoning_parts.join(""))
        };

        let finish_reason = if !self.tool_calls.is_empty() {
            FinishReason::ToolCalls
        } else {
            FinishReason::Stop
        };

        let timeline_events = self
            .timeline
            .as_ref()
            .map(TimelineState::events)
            .unwrap_or_default();
        let next_sequence = self
            .timeline
            .as_ref()
            .map(TimelineState::next_sequence)
            .unwrap_or(0);

        Ok(CompletionResponse {
            content,
            raw_content,
            reasoning_content,
            tool_calls: self.tool_calls,
            timeline_events,
            next_sequence,
            usage: self.final_usage.unwrap_or_default(),
            finish_reason,
            model: String::new(),
        })
    }

    fn record_text_delta(
        &mut self,
        item_id: Option<String>,
        delta: String,
        event_tx: &AgentEventSender,
        text_channel: TimelineTextChannel,
    ) {
        let Some(timeline) = self.timeline.as_mut() else {
            return;
        };
        let saved_item_id = match text_channel {
            TimelineTextChannel::User => None,
            TimelineTextChannel::Commentary => self.commentary_item_id.clone(),
            TimelineTextChannel::Final => self.final_item_id.clone(),
        };
        let item_id = item_id
            .filter(|value| !value.is_empty())
            .or(saved_item_id)
            .unwrap_or_else(|| timeline.item_id(text_channel.as_str()));
        match text_channel {
            TimelineTextChannel::User => {}
            TimelineTextChannel::Commentary => self.commentary_item_id = Some(item_id.clone()),
            TimelineTextChannel::Final => self.final_item_id = Some(item_id.clone()),
        }
        for event in timeline.append_text_delta(&item_id, text_channel, delta) {
            let _ = event_tx.send(event);
        }
    }

    fn apply_visible_text_segments(
        &mut self,
        item_id: Option<String>,
        segments: Vec<VisibleTextSegment>,
        event_tx: &AgentEventSender,
    ) {
        for segment in segments {
            match segment {
                VisibleTextSegment::Untagged(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    self.untagged_content.push_str(&text);
                    if self.timeline.is_none() {
                        self.content_parts.push(text);
                    }
                }
                VisibleTextSegment::Commentary(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    self.saw_tagged_content = true;
                    self.record_text_delta(
                        item_id.clone(),
                        text,
                        event_tx,
                        TimelineTextChannel::Commentary,
                    );
                }
                VisibleTextSegment::Final(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    self.saw_tagged_content = true;
                    self.content_parts.push(text.clone());
                    self.record_text_delta(
                        item_id.clone(),
                        text,
                        event_tx,
                        TimelineTextChannel::Final,
                    );
                }
                VisibleTextSegment::ProposedPlan(delta) => {
                    if !delta.is_empty() {
                        self.saw_tagged_content = true;
                        self.record_plan_delta(delta, event_tx);
                    }
                }
            }
        }
    }

    fn record_plan_delta(&mut self, delta: String, event_tx: &AgentEventSender) {
        let Some(timeline) = self.timeline.as_mut() else {
            return;
        };
        for event in timeline.append_plan_delta(delta) {
            let _ = event_tx.send(event);
        }
    }

    fn record_thinking_delta(
        &mut self,
        item_id: Option<String>,
        chunk_index: u32,
        delta: String,
        event_tx: &AgentEventSender,
    ) {
        let Some(timeline) = self.timeline.as_mut() else {
            return;
        };
        let item_id = item_id
            .filter(|value| !value.is_empty())
            .or_else(|| self.thinking_item_id.clone())
            .unwrap_or_else(|| timeline.item_id("thinking"));
        self.thinking_item_id = Some(item_id.clone());
        for event in timeline.append_thinking_delta(&item_id, chunk_index, delta) {
            let _ = event_tx.send(event);
        }
    }

    fn record_tool_delta(
        &mut self,
        snapshot: &ToolCallAccumulatorSnapshot,
        delta: String,
        event_tx: &AgentEventSender,
    ) {
        let Some(timeline) = self.timeline.as_mut() else {
            return;
        };
        for event in timeline.append_tool_arguments_delta(snapshot, delta) {
            let _ = event_tx.send(event);
        }
    }

    fn complete_tool_item(&mut self, call: &ToolCall, event_tx: &AgentEventSender) {
        let Some(timeline) = self.timeline.as_mut() else {
            return;
        };
        let event = timeline.complete_tool_call(call);
        let _ = event_tx.send(event);
    }

    fn complete_tool_item_without_broadcast(&mut self, call: &ToolCall) {
        if let Some(timeline) = self.timeline.as_mut() {
            timeline.complete_tool_call_trace_only(call);
        }
    }
}

struct ToolCallAccumulator {
    id: String,
    has_stable_id: bool,
    call_id: Option<String>,
    name: String,
    payload: ToolCallPayloadAccumulator,
}

impl ToolCallAccumulator {
    fn merge_metadata(
        &mut self,
        key: &str,
        item_id: &str,
        call_id: Option<&String>,
        name: Option<String>,
    ) {
        if !item_id.is_empty() && (self.id.is_empty() || self.id == key) {
            self.id = item_id.to_string();
            self.has_stable_id = true;
        }
        if self.call_id.is_none()
            && let Some(call_id) = call_id.filter(|call_id| !call_id.is_empty())
        {
            self.call_id = Some(call_id.clone());
        }
        if let Some(name) = name
            && !name.is_empty()
        {
            self.name = name;
        }
    }

    fn push_delta(&mut self, payload_delta: ToolCallDeltaPayload) {
        match (&mut self.payload, payload_delta) {
            (
                ToolCallPayloadAccumulator::FunctionArguments(arguments),
                ToolCallDeltaPayload::FunctionArguments(delta),
            ) => arguments.push_str(&delta),
            (
                ToolCallPayloadAccumulator::CustomInput(input),
                ToolCallDeltaPayload::CustomInput(delta),
            ) => input.push_str(&delta),
            (_, ToolCallDeltaPayload::FunctionArguments(delta)) => {
                self.payload = ToolCallPayloadAccumulator::FunctionArguments(delta);
            }
            (_, ToolCallDeltaPayload::CustomInput(delta)) => {
                self.payload = ToolCallPayloadAccumulator::CustomInput(delta);
            }
        }
    }

    fn name(&self) -> Option<String> {
        (!self.name.is_empty()).then(|| self.name.clone())
    }

    fn function_arguments(&self) -> Option<String> {
        match &self.payload {
            ToolCallPayloadAccumulator::FunctionArguments(arguments) => Some(arguments.clone()),
            ToolCallPayloadAccumulator::CustomInput(_) => None,
        }
    }

    fn custom_input(&self) -> Option<String> {
        match &self.payload {
            ToolCallPayloadAccumulator::FunctionArguments(_) => None,
            ToolCallPayloadAccumulator::CustomInput(input) => Some(input.clone()),
        }
    }

    fn snapshot(&self) -> ToolCallAccumulatorSnapshot {
        ToolCallAccumulatorSnapshot {
            id: self.id.clone(),
            call_id: self.call_id.clone(),
            name: self.name.clone(),
            arguments: self.payload.text().to_string(),
        }
    }

    fn into_tool_call(self) -> Result<ToolCall> {
        if self.name.is_empty() {
            return Err(PureError::LlmError(
                "provider emitted tool call without name".to_string(),
            ));
        }
        if !self.has_stable_id && self.call_id.as_ref().is_none_or(String::is_empty) {
            return Err(PureError::LlmError(
                "provider emitted tool call without stable id".to_string(),
            ));
        }
        match self.payload {
            ToolCallPayloadAccumulator::FunctionArguments(arguments) => Ok(ToolCall::function(
                self.id,
                self.name,
                serde_json::from_str(&arguments).unwrap_or(serde_json::Value::String(arguments)),
                self.call_id,
            )),
            ToolCallPayloadAccumulator::CustomInput(input) => {
                Ok(ToolCall::custom(self.id, self.name, input, self.call_id))
            }
        }
    }
}

enum ToolCallPayloadAccumulator {
    FunctionArguments(String),
    CustomInput(String),
}

impl ToolCallPayloadAccumulator {
    fn text(&self) -> &str {
        match self {
            Self::FunctionArguments(arguments) | Self::CustomInput(arguments) => arguments,
        }
    }
}

struct ToolCallAccumulatorSnapshot {
    id: String,
    call_id: Option<String>,
    name: String,
    arguments: String,
}

struct TimelineState {
    session_id: String,
    turn_id: String,
    inference_id: String,
    sequence: u64,
    started: HashMap<String, TimelineItem>,
    events: Vec<TraceEvent>,
}

impl TimelineState {
    fn new(context: CompletionTimelineContext) -> Self {
        Self {
            session_id: context.session_id,
            turn_id: context.turn_id,
            inference_id: context.inference_id,
            sequence: context.starting_sequence,
            started: HashMap::new(),
            events: Vec::new(),
        }
    }

    fn events(&self) -> Vec<TraceEvent> {
        self.events.clone()
    }

    fn next_sequence(&self) -> u64 {
        self.sequence
    }

    fn item_id(&self, prefix: &str) -> String {
        format!("{}-{prefix}", self.inference_id)
    }

    fn plan_item_id(&self) -> String {
        format!("{}-plan", self.turn_id)
    }

    fn namespaced_item_id(&self, item_id: &str) -> String {
        if item_id.starts_with(&self.turn_id) {
            return item_id.to_string();
        }
        format!("{}-{item_id}", self.turn_id)
    }

    fn append_text_delta(
        &mut self,
        item_id: &str,
        text_channel: TimelineTextChannel,
        delta: String,
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

    fn append_plan_start(&mut self) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.plan_item_id();
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

    fn append_plan_delta(&mut self, delta: String) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let mut events = self.append_plan_start();
        let item_id = self.plan_item_id();
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

    fn append_thinking_delta(
        &mut self,
        item_id: &str,
        chunk_index: u32,
        delta: String,
    ) -> Vec<AgentEvent> {
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

    fn append_tool_arguments_delta(
        &mut self,
        snapshot: &ToolCallAccumulatorSnapshot,
        delta: String,
    ) -> Vec<AgentEvent> {
        let now = unix_seconds();
        let item_id = self.namespaced_item_id(&timeline_tool_item_id(
            snapshot.call_id.as_ref(),
            &snapshot.id,
        ));
        let mut events = Vec::new();
        if !self.started.contains_key(&item_id) {
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
                thinking_chunks: Vec::new(),
                tool: Some(TimelineToolItem {
                    tool_call_id: item_id.clone(),
                    call_id: snapshot.call_id.clone(),
                    provider_item_id: (!snapshot.id.is_empty()).then(|| snapshot.id.clone()),
                    name: snapshot.name.clone(),
                    arguments: String::new(),
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
            events.push(AgentEvent::TimelineItemStarted { item: item.clone() });
            self.started.insert(item_id.clone(), item);
        }
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

    fn complete_tool_call(&mut self, call: &ToolCall) -> AgentEvent {
        let item = self.complete_tool_call_item(call, TimelineItemStatus::Started);
        let sequence = self.sequence;
        self.record(
            TraceEventKind::TimelineItemCompleted { item: item.clone() },
            item.updated_at,
        );
        AgentEvent::TimelineItemCompleted { sequence, item }
    }

    fn complete_streaming_items(&mut self) -> Vec<AgentEvent> {
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

    fn complete_tool_call_trace_only(&mut self, call: &ToolCall) {
        let item = self.complete_tool_call_item(call, TimelineItemStatus::Started);
        self.record(
            TraceEventKind::TimelineItemCompleted { item },
            unix_seconds(),
        );
    }

    fn complete_tool_call_item(
        &mut self,
        call: &ToolCall,
        status: TimelineItemStatus,
    ) -> TimelineItem {
        let now = unix_seconds();
        let item_id =
            self.namespaced_item_id(&timeline_tool_item_id(call.call_id.as_ref(), &call.id));
        let arguments = call.payload_text();
        let tool_item = TimelineToolItem {
            tool_call_id: item_id.clone(),
            call_id: call.call_id.clone(),
            provider_item_id: Some(call.id.clone()),
            name: call.name.clone(),
            arguments,
            result: None,
            exit_code: None,
            timed_out: false,
            working_directory: None,
            denial_reason: None,
        };
        let item = self
            .started
            .entry(item_id.clone())
            .or_insert_with(|| TimelineItem {
                turn_id: self.turn_id.clone(),
                item_id: item_id.clone(),
                sequence: self.sequence,
                kind: TimelineItemKind::Tool,
                status,
                created_at: now,
                updated_at: now,
                text_channel: None,
                content: String::new(),
                thinking_chunks: Vec::new(),
                tool: Some(tool_item.clone()),
                agent: None,
                inference: None,
                usage: None,
            });
        item.status = status;
        item.updated_at = now;
        item.tool = Some(tool_item);
        item.clone()
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

fn timeline_tool_item_id(call_id: Option<&String>, id: &str) -> String {
    call_id
        .filter(|call_id| !call_id.is_empty())
        .cloned()
        .unwrap_or_else(|| id.to_string())
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn tool_call_accumulator_key(
    stream_id: Option<&String>,
    call_id: Option<&String>,
    item_id: &str,
) -> String {
    stream_id
        .filter(|stream_id| !stream_id.is_empty())
        .cloned()
        .or_else(|| {
            call_id
                .filter(|call_id| !call_id.is_empty())
                .map(ToOwned::to_owned)
        })
        .or_else(|| (!item_id.is_empty()).then(|| item_id.to_string()))
        .unwrap_or_else(|| "tool_call".to_string())
}

fn value_string(value: &serde_json::Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
