use async_openai::types::stream::StreamResponse;
use futures::Stream;
use futures::StreamExt;
use pl_protocol::{PureError, Result};
use pl_trace::{AgentEventSender, TraceTextChannel};
use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::time::Duration;

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
pub(crate) type OpenAiRawEventStream =
    Pin<Box<dyn Stream<Item = Result<sse::SseStreamEvent>> + Send>>;
const COMPLETION_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(180);

pub type CompletionStreamAccumulator = StreamCompletionAccumulator;

pub(crate) fn decode_provider_stream(
    stream: StreamResponse<sse::SseStreamEvent>,
    protocol: OpenAiProtocol,
) -> CompletionEventStream {
    let stream = stream.map(|event| {
        event.map_err(|error| PureError::LlmError(format!("provider stream error: {error}")))
    });
    decode_openai_event_stream(Box::pin(stream), protocol)
}

pub(crate) fn decode_openai_event_stream(
    stream: OpenAiRawEventStream,
    protocol: OpenAiProtocol,
) -> CompletionEventStream {
    let state = ProviderStreamDecodeState {
        stream,
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
                Some(Err(error)) => return Some((Err(error), state)),
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
    stream: OpenAiRawEventStream,
    decoder: sse::OpenAiStreamDecoder,
    visible_output: VisibleOutputDecoder,
    pending: VecDeque<CompletionStreamEvent>,
}

pub async fn collect_completion_event_stream(
    stream: CompletionEventStream,
    event_tx: &AgentEventSender,
    trace: Option<CompletionTraceContext>,
) -> Result<CompletionResponse> {
    collect_completion_event_stream_with_idle_timeout(
        stream,
        event_tx,
        trace,
        COMPLETION_STREAM_IDLE_TIMEOUT,
    )
    .await
}

async fn collect_completion_event_stream_with_idle_timeout(
    mut stream: CompletionEventStream,
    event_tx: &AgentEventSender,
    trace: Option<CompletionTraceContext>,
    idle_timeout: Duration,
) -> Result<CompletionResponse> {
    let mut accumulator = StreamCompletionAccumulator::new(trace);

    loop {
        let event = match tokio::time::timeout(idle_timeout, stream.next()).await {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(_) => {
                let error = PureError::transient_model_transport(
                    "stream error: idle timeout waiting for provider event",
                );
                accumulator.fail_attempt(&error, event_tx);
                return Err(error);
            }
        };
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                accumulator.fail_attempt(&error, event_tx);
                return Err(error);
            }
        };
        if let Err(error) = accumulator.apply(event, event_tx) {
            accumulator.fail_attempt(&error, event_tx);
            return Err(error);
        }
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
    hosted_web_search_calls: HashMap<String, crate::HostedWebSearchCall>,
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
            hosted_web_search_calls: HashMap::new(),
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
                self.raw_reasoning_parts.push(delta.clone());
                self.record_reasoning_content_delta(&id, content_index, delta, event_tx);
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
            ModelStreamEvent::WebSearchStarted { item_id, action } => {
                self.hosted_web_search_calls
                    .entry(item_id.clone())
                    .or_insert(crate::HostedWebSearchCall {
                        item_id: item_id.clone(),
                        action: action.clone(),
                        results: None,
                    });
                self.record_web_search_started(&item_id, action, event_tx);
            }
            ModelStreamEvent::WebSearchCompleted {
                item_id,
                action,
                results,
            } => {
                self.hosted_web_search_calls.insert(
                    item_id.clone(),
                    crate::HostedWebSearchCall {
                        item_id: item_id.clone(),
                        action: action.clone(),
                        results: results.clone(),
                    },
                );
                self.record_web_search_completed(&item_id, action, results, event_tx);
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
            let error =
                PureError::transient_model_transport("provider stream ended before completion");
            self.fail_attempt(&error, event_tx);
            return Err(error);
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
        let mut hosted_web_search_calls = self
            .hosted_web_search_calls
            .into_values()
            .collect::<Vec<_>>();
        hosted_web_search_calls.sort_by(|left, right| left.item_id.cmp(&right.item_id));
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
            hosted_web_search_calls,
            next_sequence,
            usage: self.final_usage.unwrap_or_default(),
            finish_reason,
            model: String::new(),
        })
    }

    fn fail_attempt(&mut self, error: &PureError, event_tx: &AgentEventSender) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        for event in trace.fail_attempt(&error.to_string()) {
            let _ = event_tx.send(event);
        }
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

    fn record_web_search_started(
        &mut self,
        item_id: &str,
        action: crate::WebSearchAction,
        event_tx: &AgentEventSender,
    ) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        for event in trace.start_web_search(item_id, action) {
            let _ = event_tx.send(event);
        }
    }

    fn record_web_search_completed(
        &mut self,
        item_id: &str,
        action: crate::WebSearchAction,
        results: Option<Vec<serde_json::Value>>,
        event_tx: &AgentEventSender,
    ) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        for event in trace.complete_web_search(item_id, action, results) {
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

    fn record_reasoning_content_delta(
        &mut self,
        item_id: &str,
        chunk_index: u32,
        delta: String,
        event_tx: &AgentEventSender,
    ) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        for event in trace.append_reasoning_content_delta(item_id, chunk_index, delta) {
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
        tracing::warn!(
            segments = diagnostics.untagged_visible_text_segments,
            chars = diagnostics.untagged_visible_text_chars,
            "tagged visible output contained untagged visible text; using fallback final text"
        );
    }
}

#[cfg(test)]
mod tests;
