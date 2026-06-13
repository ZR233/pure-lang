use async_openai::types::stream::StreamResponse;
use futures::StreamExt;
use pl_protocol::{AgentEventSender, PureError, Result, TimelineTextChannel};

pub(crate) mod event;
mod timeline_projection;
mod tool_stream;

use crate::proposed_plan::{VisibleTextParser, VisibleTextSegment};
use crate::protocol::openai::OpenAiProtocol;
use crate::protocol::openai::sse;
use crate::request::{
    CompletionResponse, CompletionTimelineContext, FinishReason, TokenUsage, ToolCall,
};

use event::ModelStreamEvent;
use timeline_projection::TimelineProjection;
use tool_stream::ToolStream;

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
        for stream_event in protocol.parse_stream_events(&sse_event)? {
            accumulator.apply(stream_event, event_tx)?;
        }
    }

    accumulator.finish(event_tx)
}

pub(crate) struct StreamCompletionAccumulator {
    content_parts: Vec<String>,
    raw_content_parts: Vec<String>,
    reasoning_parts: Vec<String>,
    tool_calls: Vec<ToolCall>,
    tool_stream: ToolStream,
    final_usage: Option<TokenUsage>,
    completed: bool,
    timeline: Option<TimelineProjection>,
    commentary_item_id: Option<String>,
    final_item_id: Option<String>,
    thinking_item_id: Option<String>,
    text_parser: VisibleTextParser,
}

impl StreamCompletionAccumulator {
    pub(crate) fn new(timeline: Option<CompletionTimelineContext>) -> Self {
        let plan_mode = timeline.as_ref().is_some_and(|context| context.plan_mode);
        Self {
            content_parts: Vec::new(),
            raw_content_parts: Vec::new(),
            reasoning_parts: Vec::new(),
            tool_calls: Vec::new(),
            tool_stream: ToolStream::new(),
            final_usage: None,
            completed: false,
            timeline: timeline.map(TimelineProjection::new),
            commentary_item_id: None,
            final_item_id: None,
            thinking_item_id: None,
            text_parser: VisibleTextParser::new(plan_mode),
        }
    }

    pub(crate) fn apply(
        &mut self,
        stream_event: ModelStreamEvent,
        event_tx: &AgentEventSender,
    ) -> Result<()> {
        match stream_event {
            ModelStreamEvent::TextDelta {
                item_id,
                channel,
                delta,
            } => {
                self.raw_content_parts.push(delta.clone());
                if let Some(channel) = channel {
                    match channel {
                        TimelineTextChannel::User => {}
                        TimelineTextChannel::Commentary => {
                            self.record_text_delta(item_id, delta, event_tx, channel);
                        }
                        TimelineTextChannel::Final => {
                            self.content_parts.push(delta.clone());
                            self.record_text_delta(item_id, delta, event_tx, channel);
                        }
                    }
                } else {
                    let segments = self.text_parser.push_str(&delta).segments;
                    self.apply_visible_text_segments(item_id, segments, event_tx);
                }
            }
            ModelStreamEvent::ReasoningDelta {
                item_id,
                chunk_index,
                delta,
            } => {
                self.reasoning_parts.push(delta.clone());
                self.record_thinking_delta(item_id, chunk_index, delta, event_tx);
            }
            ModelStreamEvent::ToolInputDelta {
                stream_id,
                item_id,
                call_id,
                name,
                payload_delta,
            } => {
                let snapshot = self.tool_stream.append_delta(
                    stream_id.as_ref(),
                    item_id,
                    call_id.as_ref(),
                    name,
                    payload_delta,
                );
                self.record_tool_delta(&snapshot.tool, snapshot.delta, event_tx);
            }
            ModelStreamEvent::ToolCallReady {
                stream_id,
                item_id,
                call_id,
                name,
                payload,
            } => {
                if let Some(call) = self.tool_stream.finish_ready(
                    stream_id.as_ref(),
                    call_id.as_ref(),
                    &item_id,
                    name,
                    payload,
                )? {
                    self.update_tool_trace_only(&call);
                    self.tool_calls.push(call);
                }
            }
            ModelStreamEvent::Usage(usage) => {
                self.final_usage = Some(usage);
            }
            ModelStreamEvent::Completed { response_id } => {
                for call in self.tool_stream.finish_all(&self.tool_calls)? {
                    self.update_tool_trace_only(&call);
                    self.tool_calls.push(call);
                }
                self.completed = true;
                let _ = response_id;
            }
            ModelStreamEvent::Failed { code, message } => {
                let _ = code;
                return Err(PureError::LlmError(message));
            }
            ModelStreamEvent::StepStarted { response_id } => {
                let _ = response_id;
            }
        }

        Ok(())
    }

    pub(crate) fn finish(mut self, event_tx: &AgentEventSender) -> Result<CompletionResponse> {
        if !self.completed {
            return Err(PureError::LlmError(
                "provider stream ended before completion".to_string(),
            ));
        }

        let segments = self.text_parser.finish().segments;
        self.apply_visible_text_segments(None, segments, event_tx);

        for call in self.tool_stream.finish_all(&self.tool_calls)? {
            self.update_tool_trace_only(&call);
            self.tool_calls.push(call);
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
        let finish_reason = if self.tool_calls.is_empty() {
            FinishReason::Stop
        } else {
            FinishReason::ToolCalls
        };
        let timeline_events = self
            .timeline
            .as_ref()
            .map(TimelineProjection::events)
            .unwrap_or_default();
        let next_sequence = self
            .timeline
            .as_ref()
            .map(TimelineProjection::next_sequence)
            .unwrap_or_default();

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

    fn apply_visible_text_segments(
        &mut self,
        item_id: Option<String>,
        segments: Vec<VisibleTextSegment>,
        event_tx: &AgentEventSender,
    ) {
        for segment in segments {
            match segment {
                VisibleTextSegment::Untagged(text) => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    self.content_parts.push(text.clone());
                    self.record_text_delta(
                        item_id.clone(),
                        text,
                        event_tx,
                        TimelineTextChannel::Final,
                    );
                }
                VisibleTextSegment::Final(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    self.content_parts.push(text.clone());
                    self.record_text_delta(
                        item_id.clone(),
                        text,
                        event_tx,
                        TimelineTextChannel::Final,
                    );
                }
                VisibleTextSegment::Commentary(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    self.record_text_delta(
                        item_id.clone(),
                        text,
                        event_tx,
                        TimelineTextChannel::Commentary,
                    );
                }
                VisibleTextSegment::ProposedPlan(delta) => {
                    if !delta.is_empty() {
                        self.record_plan_delta(delta, event_tx);
                    }
                }
            }
        }
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
        snapshot: &tool_stream::ToolCallAccumulatorSnapshot,
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

    fn update_tool_trace_only(&mut self, call: &ToolCall) {
        if let Some(timeline) = self.timeline.as_mut() {
            timeline.update_tool_trace_only(call);
        }
    }
}
