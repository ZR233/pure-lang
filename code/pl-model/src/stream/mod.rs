use async_openai::types::stream::StreamResponse;
use futures::Stream;
use futures::StreamExt;
use pl_protocol::{PureError, Result};
use pl_trace::{AgentEventSender, TraceTextChannel};
use std::collections::{HashMap, VecDeque};
use std::pin::Pin;

pub(crate) mod event;
mod lifecycle;
mod tagged_output;
mod tool_stream;
mod trace_projection;

use crate::protocol::openai::sse;
use crate::protocol::openai::{OpenAiProtocol, VisibleOutputProtocol};
use crate::request::{
    CompletionResponse, CompletionTraceContext, FinishReason, TokenUsage, ToolCall,
};

use event::{ModelBlockContent, ModelBlockField, ModelBlockKind, ModelStreamEvent};
use lifecycle::StreamLifecycle;
use tagged_output::{TaggedOutputDiagnostics, TaggedVisibleOutputAdapter};
use tool_stream::ToolStream;
use trace_projection::TraceProjection;

pub use event::{
    ModelBlockContent as CompletionBlockContent, ModelBlockField as CompletionBlockField,
    ModelBlockKind as CompletionBlockKind, ModelStreamEvent as CompletionStreamEvent,
    ToolInputDeltaPayload, ToolInputPayloadKind,
};

pub type CompletionEventStream = Pin<Box<dyn Stream<Item = Result<CompletionStreamEvent>> + Send>>;

pub type CompletionStreamAccumulator = StreamCompletionAccumulator;

pub(crate) fn decode_provider_stream(
    stream: StreamResponse<sse::SseStreamEvent>,
    protocol: OpenAiProtocol,
) -> CompletionEventStream {
    let state = ProviderStreamDecodeState {
        stream: Box::pin(stream),
        decoder: protocol.new_stream_decoder(),
        visible_output: VisibleOutputDecoder::new(protocol.visible_output_protocol()),
        pending: VecDeque::new(),
    };

    Box::pin(futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(event) = state.pending.pop_front() {
                return Some((Ok(event), state));
            }

            let sse_event = match state.stream.next().await {
                Some(Ok(event)) => event,
                Some(Err(error)) => {
                    return Some((
                        Err(PureError::LlmError(format!(
                            "provider stream error: {error}"
                        ))),
                        state,
                    ));
                }
                None => {
                    state.visible_output.record_diagnostics();
                    return None;
                }
            };

            for stream_event in state.decoder.decode(&sse_event) {
                state
                    .pending
                    .extend(state.visible_output.decode(stream_event));
            }
        }
    }))
}

struct ProviderStreamDecodeState {
    stream: Pin<Box<StreamResponse<sse::SseStreamEvent>>>,
    decoder: sse::OpenAiStreamDecoder,
    visible_output: VisibleOutputDecoder,
    pending: VecDeque<CompletionStreamEvent>,
}

pub async fn collect_completion_event_stream(
    mut stream: CompletionEventStream,
    event_tx: &AgentEventSender,
    trace: Option<CompletionTraceContext>,
) -> Result<CompletionResponse> {
    let mut accumulator = StreamCompletionAccumulator::new(trace);

    while let Some(event) = stream.next().await {
        accumulator.apply(event?, event_tx)?;
    }

    accumulator.finish(event_tx)
}

pub struct StreamCompletionAccumulator {
    content_parts: Vec<ContentPart>,
    content_indexes: HashMap<String, usize>,
    raw_content_parts: Vec<String>,
    reasoning_summary_parts: Vec<String>,
    raw_reasoning_parts: Vec<String>,
    tool_calls: Vec<ToolCall>,
    tool_stream: ToolStream,
    lifecycle: StreamLifecycle,
    final_usage: Option<TokenUsage>,
    response_id: Option<String>,
    completed: bool,
    trace: Option<TraceProjection>,
}

impl StreamCompletionAccumulator {
    pub fn new(trace: Option<CompletionTraceContext>) -> Self {
        Self {
            content_parts: Vec::new(),
            content_indexes: HashMap::new(),
            raw_content_parts: Vec::new(),
            reasoning_summary_parts: Vec::new(),
            raw_reasoning_parts: Vec::new(),
            tool_calls: Vec::new(),
            tool_stream: ToolStream::new(),
            lifecycle: StreamLifecycle::new(),
            final_usage: None,
            response_id: None,
            completed: false,
            trace: trace.map(TraceProjection::new),
        }
    }

    pub fn apply(
        &mut self,
        stream_event: ModelStreamEvent,
        event_tx: &AgentEventSender,
    ) -> Result<()> {
        if self.completed {
            return Err(PureError::LlmError(
                "provider stream emitted event after completion".to_string(),
            ));
        }
        if let ModelStreamEvent::BlockDelta {
            kind:
                ModelBlockKind::Text {
                    channel: TraceTextChannel::Final,
                },
            delta,
            ..
        } = &stream_event
        {
            self.raw_content_parts.push(delta.clone());
        }
        for stream_event in self.lifecycle.normalize(stream_event)? {
            self.apply_normalized(stream_event, event_tx)?;
        }

        Ok(())
    }

    fn apply_normalized(
        &mut self,
        stream_event: ModelStreamEvent,
        event_tx: &AgentEventSender,
    ) -> Result<()> {
        match stream_event {
            ModelStreamEvent::BlockOpened {
                id,
                kind: ModelBlockKind::Text { channel },
                ..
            } => {
                self.record_text_started(&id, channel, event_tx);
            }
            ModelStreamEvent::BlockOpened {
                id,
                kind: ModelBlockKind::ReasoningSummary,
                ..
            } => {
                let _ = id;
            }
            ModelStreamEvent::BlockOpened {
                kind: ModelBlockKind::Plan,
                ..
            } => {}
            ModelStreamEvent::BlockDelta {
                id,
                kind: ModelBlockKind::Text { channel },
                field: ModelBlockField::Text,
                delta,
                ..
            } => {
                if channel == TraceTextChannel::Final {
                    self.append_content_part(&id, &delta);
                }
                self.record_text_delta(&id, delta, event_tx, channel);
            }
            ModelStreamEvent::BlockDelta {
                id,
                kind: ModelBlockKind::ReasoningSummary,
                field: ModelBlockField::ReasoningSummary,
                delta,
                section_index,
            } => {
                self.reasoning_summary_parts.push(delta.clone());
                self.record_thinking_delta(&id, section_index.unwrap_or_default(), delta, event_tx);
            }
            ModelStreamEvent::BlockDelta {
                kind: ModelBlockKind::Plan,
                ..
            }
            | ModelStreamEvent::BlockDelta {
                kind: ModelBlockKind::Text { .. },
                ..
            }
            | ModelStreamEvent::BlockDelta {
                kind: ModelBlockKind::ReasoningSummary,
                ..
            } => {}
            ModelStreamEvent::BlockClosed {
                id,
                kind: ModelBlockKind::Text { channel },
                authoritative_content,
                ..
            } => {
                let authoritative_text = match authoritative_content {
                    Some(ModelBlockContent::Text(text)) => Some(text),
                    Some(ModelBlockContent::ReasoningSummary(_) | ModelBlockContent::Plan(_))
                    | None => None,
                };
                if channel == TraceTextChannel::Final
                    && let Some(text) = authoritative_text.as_deref()
                {
                    self.complete_content_part(&id, text);
                }
                self.record_text_completed(&id, channel, authoritative_text, event_tx);
            }
            ModelStreamEvent::BlockClosed {
                id,
                kind: ModelBlockKind::ReasoningSummary,
                authoritative_content,
                ..
            } => {
                let authoritative_summary =
                    if let Some(ModelBlockContent::ReasoningSummary(summary)) =
                        authoritative_content
                    {
                        self.reasoning_summary_parts = summary.clone();
                        Some(summary)
                    } else {
                        None
                    };
                self.record_thinking_completed(&id, authoritative_summary, event_tx);
            }
            ModelStreamEvent::BlockClosed {
                kind: ModelBlockKind::Plan,
                ..
            } => {}
            ModelStreamEvent::ReasoningRawDelta {
                id,
                content_index,
                delta,
            } => {
                let _ = id;
                let _ = content_index;
                self.raw_reasoning_parts.push(delta);
            }
            ModelStreamEvent::ToolInputStarted {
                stream_id,
                item_id,
                call_id,
                name,
                payload_kind,
            } => {
                let snapshot = self.tool_stream.start_input(
                    stream_id.as_ref(),
                    item_id,
                    call_id.as_ref(),
                    name,
                    lifecycle::tool_start_payload(payload_kind),
                );
                self.record_tool_started(&snapshot.tool, event_tx);
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
            ModelStreamEvent::ToolInputCompleted {
                stream_id,
                item_id,
                call_id,
                name,
                payload,
            } => {
                self.tool_stream.complete_input(
                    stream_id.as_ref(),
                    call_id.as_ref(),
                    &item_id,
                    name,
                    payload,
                );
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
                    self.update_tool_trace(&call, event_tx);
                    self.tool_calls.push(call);
                }
            }
            ModelStreamEvent::Usage(usage) => {
                self.final_usage = Some(usage);
            }
            ModelStreamEvent::Completed { response_id } => {
                for call in self.tool_stream.finish_all(&self.tool_calls)? {
                    self.update_tool_trace(&call, event_tx);
                    self.tool_calls.push(call);
                }
                if response_id.is_some() {
                    self.response_id = response_id;
                }
                self.completed = true;
            }
            ModelStreamEvent::Failed { code, message } => {
                let _ = code;
                return Err(PureError::LlmError(message));
            }
            ModelStreamEvent::ResponseStarted { response_id } => {
                if self.response_id.is_none() {
                    self.response_id = response_id;
                }
            }
        }

        Ok(())
    }

    pub fn finish(mut self, event_tx: &AgentEventSender) -> Result<CompletionResponse> {
        if !self.completed {
            return Err(PureError::LlmError(
                "provider stream ended before completion".to_string(),
            ));
        }

        for call in self.tool_stream.finish_all(&self.tool_calls)? {
            self.update_tool_trace(&call, event_tx);
            self.tool_calls.push(call);
        }
        if let Some(trace) = self.trace.as_mut() {
            for event in trace.complete_streaming_items() {
                let _ = event_tx.send(event);
            }
        }

        let content = if self.content_parts.is_empty() {
            None
        } else {
            Some(
                self.content_parts
                    .iter()
                    .map(|part| part.text.as_str())
                    .collect::<String>(),
            )
        };
        let raw_content = if self.raw_content_parts.is_empty() {
            None
        } else {
            Some(self.raw_content_parts.join(""))
        };
        let reasoning_content = if self.raw_reasoning_parts.is_empty() {
            None
        } else {
            Some(self.raw_reasoning_parts.join(""))
        };
        let finish_reason = if self.tool_calls.is_empty() {
            FinishReason::Stop
        } else {
            FinishReason::ToolCalls
        };
        let trace_events = self
            .trace
            .as_ref()
            .map(TraceProjection::events)
            .unwrap_or_default();
        let next_sequence = self
            .trace
            .as_ref()
            .map(TraceProjection::next_sequence)
            .unwrap_or_default();

        Ok(CompletionResponse {
            response_id: self.response_id,
            content,
            raw_content,
            reasoning_content,
            tool_calls: self.tool_calls,
            trace_events,
            next_sequence,
            usage: self.final_usage.unwrap_or_default(),
            finish_reason,
            model: String::new(),
        })
    }

    fn record_text_started(
        &mut self,
        item_id: &str,
        text_channel: TraceTextChannel,
        event_tx: &AgentEventSender,
    ) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        for event in trace.start_text(item_id, text_channel) {
            let _ = event_tx.send(event);
        }
    }

    fn record_text_delta(
        &mut self,
        item_id: &str,
        delta: String,
        event_tx: &AgentEventSender,
        text_channel: TraceTextChannel,
    ) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        for event in trace.append_text_delta(item_id, text_channel, delta) {
            let _ = event_tx.send(event);
        }
    }

    fn record_text_completed(
        &mut self,
        item_id: &str,
        text_channel: TraceTextChannel,
        authoritative_text: Option<String>,
        event_tx: &AgentEventSender,
    ) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        for event in trace.complete_text(item_id, text_channel, authoritative_text) {
            let _ = event_tx.send(event);
        }
    }

    fn append_content_part(&mut self, item_id: &str, delta: &str) {
        let index = *self
            .content_indexes
            .entry(item_id.to_string())
            .or_insert_with(|| {
                self.content_parts.push(ContentPart {
                    text: String::new(),
                });
                self.content_parts.len() - 1
            });
        self.content_parts[index].text.push_str(delta);
    }

    fn complete_content_part(&mut self, item_id: &str, text: &str) {
        let index = *self
            .content_indexes
            .entry(item_id.to_string())
            .or_insert_with(|| {
                self.content_parts.push(ContentPart {
                    text: String::new(),
                });
                self.content_parts.len() - 1
            });
        self.content_parts[index].text = text.to_string();
    }

    fn record_thinking_delta(
        &mut self,
        item_id: &str,
        chunk_index: u32,
        delta: String,
        event_tx: &AgentEventSender,
    ) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        for event in trace.append_thinking_delta(item_id, chunk_index, delta) {
            let _ = event_tx.send(event);
        }
    }

    fn record_thinking_completed(
        &mut self,
        item_id: &str,
        authoritative_summary: Option<Vec<String>>,
        event_tx: &AgentEventSender,
    ) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        for event in trace.complete_thinking(item_id, authoritative_summary) {
            let _ = event_tx.send(event);
        }
    }

    fn record_tool_delta(
        &mut self,
        snapshot: &tool_stream::ToolCallAccumulatorSnapshot,
        delta: String,
        event_tx: &AgentEventSender,
    ) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        for event in trace.append_tool_arguments_delta(snapshot, delta) {
            let _ = event_tx.send(event);
        }
    }

    fn record_tool_started(
        &mut self,
        snapshot: &tool_stream::ToolCallAccumulatorSnapshot,
        event_tx: &AgentEventSender,
    ) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        for event in trace.start_tool(snapshot) {
            let _ = event_tx.send(event);
        }
    }

    fn update_tool_trace(&mut self, call: &ToolCall, event_tx: &AgentEventSender) {
        if let Some(trace) = self.trace.as_mut() {
            for event in trace.update_tool_trace(call) {
                let _ = event_tx.send(event);
            }
        }
    }
}

struct ContentPart {
    text: String,
}

pub(crate) enum VisibleOutputDecoder {
    NativePhases,
    TaggedText(TaggedVisibleOutputAdapter),
}

impl VisibleOutputDecoder {
    pub(crate) fn new(protocol: VisibleOutputProtocol) -> Self {
        match protocol {
            VisibleOutputProtocol::NativePhases => Self::NativePhases,
            VisibleOutputProtocol::TaggedText => {
                Self::TaggedText(TaggedVisibleOutputAdapter::new())
            }
        }
    }

    pub(crate) fn decode(&mut self, event: ModelStreamEvent) -> Vec<ModelStreamEvent> {
        match self {
            Self::NativePhases => vec![event],
            Self::TaggedText(decoder) => decoder.adapt(event),
        }
    }

    pub(crate) fn diagnostics(&self) -> TaggedOutputDiagnostics {
        match self {
            Self::NativePhases => TaggedOutputDiagnostics::default(),
            Self::TaggedText(decoder) => decoder.diagnostics(),
        }
    }

    fn record_diagnostics(&self) {
        let diagnostics = self.diagnostics();
        if diagnostics.untagged_visible_text_segments == 0 {
            return;
        }
        eprintln!(
            "[pl-model] tagged visible output contained {} untagged visible text segment(s), {} char(s); using fallback final text",
            diagnostics.untagged_visible_text_segments, diagnostics.untagged_visible_text_chars
        );
    }
}

#[cfg(test)]
mod tests;
