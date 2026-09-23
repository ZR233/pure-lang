//! 流事件累积器：把 canonical `ModelStreamEvent` 序列累积为 `CompletionResponse`。
//!
//! 累积器同时驱动 trace 投影（`TraceProjection`）并把投影事件发布到宿主
//! `AgentEventSender`；生命周期合法性与工具调用增量分别由 `lifecycle` 与
//! `tool_stream` 子状态机保证。

use std::collections::HashMap;
use std::sync::Arc;

use pl_protocol::trace::{AgentEvent, AgentEventSender, TraceEventSink, TraceTextChannel};
use pl_protocol::{
    InferenceModelObservation, InferenceOrchestrationMetrics, PureError, ResponsesContextItem,
    ResponsesContextItemKind, Result, ToolCallCaller, UsageReport,
};

use crate::completion::{
    CompletionPresentationItem, CompletionPresentationPart, CompletionResponse,
    CompletionTraceContext, ToolCall,
};

use super::event::{ModelBlockContent, ModelBlockField, ModelBlockKind, ModelStreamEvent};
use super::lifecycle::{self, StreamLifecycle};
use super::state::{CompletedStream, FailedStream, StreamAccumulatorState};
use super::tool_stream::{self, ToolStream};
use super::trace_projection::TraceProjection;

const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

pub(crate) struct StreamCompletionAccumulator {
    content_parts: Vec<ContentPart>,
    content_indexes: HashMap<String, usize>,
    other_text_lengths: HashMap<(String, &'static str), usize>,
    reasoning_summary_parts: Vec<String>,
    raw_reasoning_parts: Vec<String>,
    tool_calls: Vec<ToolCall>,
    tool_call_callers: HashMap<String, ToolCallCaller>,
    responses_context_items: Vec<ResponsesContextItem>,
    presentation_items: Vec<CompletionPresentationItem>,
    presentation_indexes: HashMap<String, usize>,
    presentation_sizes: Vec<usize>,
    presentation_bytes: usize,
    text_output_bytes: usize,
    tool_stream: ToolStream,
    lifecycle: StreamLifecycle,
    final_usage: Option<UsageReport>,
    response_id: Option<String>,
    model_observation: Option<InferenceModelObservation>,
    reported_model_is_terminal: bool,
    state: StreamAccumulatorState,
    trace: Option<TraceProjection>,
}

impl StreamCompletionAccumulator {
    pub(crate) fn with_model_observation(
        trace: Option<CompletionTraceContext>,
        trace_sink: Option<Arc<dyn TraceEventSink>>,
        model_observation: Option<InferenceModelObservation>,
    ) -> Self {
        Self {
            content_parts: Vec::new(),
            content_indexes: HashMap::new(),
            other_text_lengths: HashMap::new(),
            reasoning_summary_parts: Vec::new(),
            raw_reasoning_parts: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_callers: HashMap::new(),
            responses_context_items: Vec::new(),
            presentation_items: Vec::new(),
            presentation_indexes: HashMap::new(),
            presentation_sizes: Vec::new(),
            presentation_bytes: 0,
            text_output_bytes: 0,
            tool_stream: ToolStream::new(),
            lifecycle: StreamLifecycle::new(),
            final_usage: None,
            response_id: None,
            model_observation,
            reported_model_is_terminal: false,
            state: StreamAccumulatorState::open(),
            trace: trace.map(|trace| TraceProjection::with_sink(trace, trace_sink)),
        }
    }

    pub(crate) fn apply(
        &mut self,
        stream_event: ModelStreamEvent,
        event_tx: &AgentEventSender,
    ) -> Result<()> {
        match &self.state {
            StreamAccumulatorState::Open(_) => {}
            StreamAccumulatorState::Completed(_) => {
                return Err(PureError::LlmError(
                    "provider stream emitted event after completion".to_string(),
                ));
            }
            StreamAccumulatorState::Failed(failed) => {
                return Err(PureError::LlmError(format!(
                    "provider stream emitted event after failure: {}",
                    failed.message()
                )));
            }
        }
        for stream_event in self.lifecycle.normalize(stream_event)? {
            self.apply_normalized(stream_event, event_tx)?;
            self.ensure_trace_sink()?;
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
            ModelStreamEvent::BlockDelta {
                id,
                kind: ModelBlockKind::Text { channel },
                field: ModelBlockField::Text,
                delta,
                ..
            } => {
                if channel == TraceTextChannel::Final {
                    self.append_content_part(&id, &delta)?;
                } else {
                    self.charge_text_output(0, delta.len())?;
                    *self
                        .other_text_lengths
                        .entry((id.clone(), channel.as_str()))
                        .or_default() += delta.len();
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
                self.charge_text_output(0, delta.len())?;
                self.reasoning_summary_parts.push(delta.clone());
                self.record_thinking_delta(&id, section_index.unwrap_or_default(), delta, event_tx);
            }
            ModelStreamEvent::BlockDelta {
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
                    Some(ModelBlockContent::ReasoningSummary(_)) | None => None,
                };
                if channel == TraceTextChannel::Final
                    && let Some(text) = authoritative_text.as_deref()
                {
                    self.complete_content_part(&id, text)?;
                } else if let Some(text) = authoritative_text.as_deref() {
                    let key = (id.clone(), channel.as_str());
                    let old = self
                        .other_text_lengths
                        .get(&key)
                        .copied()
                        .unwrap_or_default();
                    self.charge_text_output(old, text.len())?;
                    self.other_text_lengths.insert(key, text.len());
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
                        let old = self.reasoning_summary_parts.iter().map(String::len).sum();
                        let new = summary.iter().map(String::len).sum();
                        self.charge_text_output(old, new)?;
                        self.reasoning_summary_parts = summary.clone();
                        Some(summary)
                    } else {
                        None
                    };
                self.record_thinking_completed(&id, authoritative_summary, event_tx);
            }
            ModelStreamEvent::ReasoningRawDelta {
                id,
                content_index,
                delta,
            } => {
                self.charge_text_output(0, delta.len())?;
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
                if let Some(mut call) = self.tool_stream.finish_ready(
                    stream_id.as_ref(),
                    call_id.as_ref(),
                    &item_id,
                    name,
                    payload,
                )? {
                    self.attach_tool_caller(&mut call);
                    self.update_tool_trace(&call, event_tx);
                    self.tool_calls.push(call);
                }
            }
            ModelStreamEvent::ToolCallCaller { item_id, caller } => {
                if let Some(call) = self
                    .tool_calls
                    .iter_mut()
                    .find(|call| call.id == item_id || call.call_id == item_id)
                {
                    call.caller = Some(caller);
                } else {
                    self.tool_call_callers.insert(item_id, caller);
                }
            }
            ModelStreamEvent::ResponsesContextItem { item } => {
                self.responses_context_items.push(item);
            }
            ModelStreamEvent::PresentationItem { item } => {
                self.record_presentation_item(item)?;
            }
            ModelStreamEvent::WebSearchStarted { item_id, action } => {
                self.record_web_search_started(&item_id, action, event_tx);
            }
            ModelStreamEvent::WebSearchCompleted {
                item_id,
                action,
                results,
            } => {
                self.record_web_search_completed(&item_id, action, results, event_tx);
            }
            ModelStreamEvent::Usage(usage) => {
                self.final_usage = Some(usage);
            }
            ModelStreamEvent::Completed { response_id } => {
                for mut call in self.tool_stream.finish_all(&self.tool_calls)? {
                    self.attach_tool_caller(&mut call);
                    self.update_tool_trace(&call, event_tx);
                    self.tool_calls.push(call);
                }
                if response_id.is_some() {
                    self.response_id = response_id;
                }
                self.state = StreamAccumulatorState::Completed(CompletedStream::new());
            }
            ModelStreamEvent::Failed {
                code,
                http_status,
                retry_after_ms,
                message,
            } => {
                let error = crate::runtime::provider_stream_failure(
                    code.as_deref(),
                    http_status,
                    retry_after_ms,
                    message,
                );
                self.state = StreamAccumulatorState::Failed(FailedStream::new(error.to_string()));
                return Err(error);
            }
            ModelStreamEvent::ResponseStarted { response_id } => {
                if self.response_id.is_none() {
                    self.response_id = response_id;
                }
            }
            ModelStreamEvent::ResponseModelObserved { model, terminal } => {
                self.observe_reported_model(&model, terminal);
            }
        }

        Ok(())
    }

    pub(crate) fn take_presentation_items(&mut self) -> Vec<CompletionPresentationItem> {
        std::mem::take(&mut self.presentation_items)
    }

    fn charge_text_output(&mut self, old: usize, new: usize) -> Result<()> {
        let next = self.text_output_bytes - old;
        let next = next
            .checked_add(new)
            .filter(|&bytes| bytes <= MAX_OUTPUT_BYTES)
            .ok_or_else(|| PureError::MemoryError("provider text output exceeds 16 MiB".into()))?;
        self.text_output_bytes = next;
        Ok(())
    }

    fn record_presentation_item(&mut self, item: CompletionPresentationItem) -> Result<()> {
        let key = item.provider_item_id.clone();
        let previous = self
            .presentation_indexes
            .get(&key)
            .map(|&index| self.presentation_sizes[index])
            .unwrap_or(0);
        let retained = self.presentation_bytes - previous;
        let size = presentation_item_bytes(&item).ok_or_else(|| {
            PureError::MemoryError("provider presentation output exceeds 16 MiB".into())
        })?;
        let next = retained
            .checked_add(size)
            .filter(|&bytes| bytes <= MAX_OUTPUT_BYTES)
            .ok_or_else(|| {
                PureError::MemoryError("provider presentation output exceeds 16 MiB".into())
            })?;
        if let Some(&index) = self.presentation_indexes.get(&key) {
            self.presentation_items[index] = item;
            self.presentation_sizes[index] = size;
        } else {
            self.presentation_indexes
                .insert(key, self.presentation_items.len());
            self.presentation_items.push(item);
            self.presentation_sizes.push(size);
        }
        self.presentation_bytes = next;
        Ok(())
    }

    pub(crate) fn finish(&mut self, event_tx: &AgentEventSender) -> Result<CompletionResponse> {
        self.finish_inner(event_tx)
    }

    fn finish_inner(&mut self, event_tx: &AgentEventSender) -> Result<CompletionResponse> {
        let terminal_error = match &self.state {
            StreamAccumulatorState::Open(_) => Some(PureError::transient_model_transport(
                "provider stream ended before completion",
            )),
            StreamAccumulatorState::Completed(_) => None,
            StreamAccumulatorState::Failed(failed) => {
                Some(PureError::LlmError(failed.message().to_string()))
            }
        };
        if let Some(error) = terminal_error {
            self.fail_attempt(&error, event_tx);
            return Err(error);
        }

        for mut call in self.tool_stream.finish_all(&self.tool_calls)? {
            self.attach_tool_caller(&mut call);
            self.update_tool_trace(&call, event_tx);
            self.tool_calls.push(call);
        }
        if let Some(trace) = self.trace.as_mut() {
            for event in trace.complete_streaming_items() {
                let _ = event_tx.send(event);
            }
        }
        self.ensure_trace_sink()?;

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
        let reasoning_content = if self.raw_reasoning_parts.is_empty() {
            None
        } else {
            Some(self.raw_reasoning_parts.join(""))
        };
        let orchestration =
            stream_orchestration_metrics(&self.responses_context_items, &self.tool_calls);
        let model = self
            .model_observation
            .as_ref()
            .and_then(|observation| observation.reported_model.clone())
            .unwrap_or_default();
        Ok(CompletionResponse {
            response_id: self.response_id.take(),
            content,
            reasoning_content,
            tool_calls: std::mem::take(&mut self.tool_calls),
            responses_context_items: std::mem::take(&mut self.responses_context_items),
            presentation_items: self.take_presentation_items(),
            orchestration,
            timing: None,
            accounting: pl_protocol::InferenceAccounting {
                usage: self.final_usage.take().unwrap_or_default(),
                ..Default::default()
            },
            model,
            model_observation: self.model_observation.take(),
        })
    }

    fn observe_reported_model(&mut self, model: &str, terminal: bool) {
        if self.reported_model_is_terminal {
            return;
        }
        let Some(model) = valid_reported_model(model) else {
            return;
        };
        let Some(observation) = self.model_observation.as_mut() else {
            return;
        };
        if terminal || observation.reported_model.is_none() {
            observation.reported_model = Some(model);
        }
        if terminal {
            self.reported_model_is_terminal = true;
        }
    }

    pub(super) fn model_observation(&self) -> Option<InferenceModelObservation> {
        self.model_observation.clone()
    }

    fn attach_tool_caller(&mut self, call: &mut ToolCall) {
        call.caller = self
            .tool_call_callers
            .remove(&call.id)
            .or_else(|| self.tool_call_callers.remove(&call.call_id));
    }

    pub(super) fn accounting(&self) -> pl_protocol::InferenceAccounting {
        pl_protocol::InferenceAccounting {
            usage: self.final_usage.clone().unwrap_or_default(),
            ..Default::default()
        }
    }

    pub(super) fn fail_attempt(&mut self, error: &PureError, event_tx: &AgentEventSender) {
        self.state = StreamAccumulatorState::Failed(FailedStream::new(error.to_string()));
        self.publish_trace(event_tx, |trace| trace.fail_attempt(&error.to_string()));
    }

    pub(super) fn cancel_attempt(&mut self, reason: &str, event_tx: &AgentEventSender) {
        self.publish_trace(event_tx, |trace| trace.cancel_attempt(reason));
    }

    fn ensure_trace_sink(&mut self) -> Result<()> {
        let Some(trace) = self.trace.as_mut() else {
            return Ok(());
        };
        match trace.take_trace_error() {
            Some(error) => Err(PureError::Protocol(format!(
                "canonical trace publication failed: {error}"
            ))),
            None => Ok(()),
        }
    }

    fn record_text_started(
        &mut self,
        item_id: &str,
        text_channel: TraceTextChannel,
        event_tx: &AgentEventSender,
    ) {
        self.publish_trace(event_tx, |trace| trace.start_text(item_id, text_channel));
    }

    fn record_web_search_started(
        &mut self,
        item_id: &str,
        action: pl_protocol::search::WebSearchAction,
        event_tx: &AgentEventSender,
    ) {
        self.publish_trace(event_tx, |trace| trace.start_web_search(item_id, action));
    }

    fn record_web_search_completed(
        &mut self,
        item_id: &str,
        action: pl_protocol::search::WebSearchAction,
        results: Option<Vec<serde_json::Value>>,
        event_tx: &AgentEventSender,
    ) {
        self.publish_trace(event_tx, |trace| {
            trace.complete_web_search(item_id, action, results)
        });
    }

    fn record_text_delta(
        &mut self,
        item_id: &str,
        delta: String,
        event_tx: &AgentEventSender,
        text_channel: TraceTextChannel,
    ) {
        self.publish_trace(event_tx, |trace| {
            trace.append_text_delta(item_id, text_channel, delta)
        });
    }

    fn record_text_completed(
        &mut self,
        item_id: &str,
        text_channel: TraceTextChannel,
        authoritative_text: Option<String>,
        event_tx: &AgentEventSender,
    ) {
        self.publish_trace(event_tx, |trace| {
            trace.complete_text(item_id, text_channel, authoritative_text)
        });
    }

    fn append_content_part(&mut self, item_id: &str, delta: &str) -> Result<()> {
        let index = self.content_part_slot(item_id);
        self.charge_text_output(0, delta.len())?;
        self.content_parts[index].text.push_str(delta);
        Ok(())
    }

    fn complete_content_part(&mut self, item_id: &str, text: &str) -> Result<()> {
        let index = self.content_part_slot(item_id);
        let old = self.content_parts[index].text.len();
        self.charge_text_output(old, text.len())?;
        self.content_parts[index].text = text.to_string();
        Ok(())
    }

    /// 返回 item 对应的 content part 下标，缺失时追加空 part。
    fn content_part_slot(&mut self, item_id: &str) -> usize {
        *self
            .content_indexes
            .entry(item_id.to_string())
            .or_insert_with(|| {
                self.content_parts.push(ContentPart {
                    text: String::new(),
                });
                self.content_parts.len() - 1
            })
    }

    fn record_thinking_delta(
        &mut self,
        item_id: &str,
        chunk_index: u32,
        delta: String,
        event_tx: &AgentEventSender,
    ) {
        self.publish_trace(event_tx, |trace| {
            trace.append_thinking_delta(item_id, chunk_index, delta)
        });
    }

    fn record_reasoning_content_delta(
        &mut self,
        item_id: &str,
        chunk_index: u32,
        delta: String,
        event_tx: &AgentEventSender,
    ) {
        self.publish_trace(event_tx, |trace| {
            trace.append_reasoning_content_delta(item_id, chunk_index, delta)
        });
    }

    fn record_thinking_completed(
        &mut self,
        item_id: &str,
        authoritative_summary: Option<Vec<String>>,
        event_tx: &AgentEventSender,
    ) {
        self.publish_trace(event_tx, |trace| {
            trace.complete_thinking(item_id, authoritative_summary)
        });
    }

    fn record_tool_delta(
        &mut self,
        snapshot: &tool_stream::ToolCallAccumulatorSnapshot,
        delta: String,
        event_tx: &AgentEventSender,
    ) {
        self.publish_trace(event_tx, |trace| {
            trace.append_tool_arguments_delta(snapshot, delta)
        });
    }

    fn record_tool_started(
        &mut self,
        snapshot: &tool_stream::ToolCallAccumulatorSnapshot,
        event_tx: &AgentEventSender,
    ) {
        self.publish_trace(event_tx, |trace| trace.start_tool(snapshot));
    }

    fn update_tool_trace(&mut self, call: &ToolCall, event_tx: &AgentEventSender) {
        self.publish_trace(event_tx, |trace| trace.update_tool_trace(call));
    }

    /// 投影 trace 事件并发布到宿主事件通道；未挂载 trace 时静默跳过。
    fn publish_trace(
        &mut self,
        event_tx: &AgentEventSender,
        project: impl FnOnce(&mut TraceProjection) -> Vec<AgentEvent>,
    ) {
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        for event in project(trace) {
            let _ = event_tx.send(event);
        }
    }
}

fn presentation_item_bytes(item: &CompletionPresentationItem) -> Option<usize> {
    // Include fixed record and index costs so zero-length items cannot grow without a bound.
    let base = std::mem::size_of::<CompletionPresentationItem>()
        .checked_add(std::mem::size_of::<(String, Option<u32>)>())?
        .checked_add(std::mem::size_of::<usize>())?
        .checked_add(item.provider_item_id.len().checked_mul(2)?)?;
    item.parts.iter().try_fold(base, |bytes, part| {
        bytes
            .checked_add(std::mem::size_of::<CompletionPresentationPart>())?
            .checked_add(part.provider_part_id.as_ref().map_or(0, String::len))?
            .checked_add(part.text.len())
    })
}

fn valid_reported_model(model: &str) -> Option<String> {
    let model = model.trim();
    (!model.is_empty() && model.len() <= 256 && !model.chars().any(char::is_control))
        .then(|| model.to_string())
}

fn stream_orchestration_metrics(
    context_items: &[ResponsesContextItem],
    tool_calls: &[ToolCall],
) -> InferenceOrchestrationMetrics {
    let program_count = context_items
        .iter()
        .filter(|item| item.kind == ResponsesContextItemKind::Program)
        .count() as u64;
    let program_tool_calls = tool_calls
        .iter()
        .filter(|call| call.caller.is_some())
        .count() as u64;

    InferenceOrchestrationMetrics {
        tool_calls: tool_calls.len() as u64,
        program_count,
        program_tool_calls,
        transport_attempts: 1,
        ..InferenceOrchestrationMetrics::default()
    }
}

struct ContentPart {
    text: String,
}
