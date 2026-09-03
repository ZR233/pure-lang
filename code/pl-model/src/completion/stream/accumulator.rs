//! 流事件累积器：把 canonical `ModelStreamEvent` 序列累积为 `CompletionResponse`。
//!
//! 累积器同时驱动 trace 投影（`TraceProjection`）并把投影事件发布到宿主
//! `AgentEventSender`；生命周期合法性与工具调用增量分别由 `lifecycle` 与
//! `tool_stream` 子状态机保证。

use std::collections::HashMap;
use std::sync::Arc;

use pl_protocol::{
    InferenceOrchestrationMetrics, PureError, ResponsesContextItem, ResponsesContextItemKind,
    Result, TokenUsage, ToolCallCaller,
};
use pl_trace::{AgentEvent, AgentEventSender, TraceEventSink, TraceTextChannel};

use crate::completion::{CompletionResponse, CompletionTraceContext, ToolCall};

use super::event::{ModelBlockContent, ModelBlockField, ModelBlockKind, ModelStreamEvent};
use super::lifecycle::{self, StreamLifecycle};
use super::state::{CompletedStream, FailedStream, StreamAccumulatorState};
use super::tool_stream::{self, ToolStream};
use super::trace_projection::TraceProjection;

pub(crate) struct StreamCompletionAccumulator {
    content_parts: Vec<ContentPart>,
    content_indexes: HashMap<String, usize>,
    reasoning_summary_parts: Vec<String>,
    raw_reasoning_parts: Vec<String>,
    tool_calls: Vec<ToolCall>,
    tool_call_callers: HashMap<String, ToolCallCaller>,
    responses_context_items: Vec<ResponsesContextItem>,
    tool_stream: ToolStream,
    lifecycle: StreamLifecycle,
    final_usage: Option<TokenUsage>,
    response_id: Option<String>,
    state: StreamAccumulatorState,
    trace: Option<TraceProjection>,
}

impl StreamCompletionAccumulator {
    #[cfg(test)]
    pub(crate) fn new(trace: Option<CompletionTraceContext>) -> Self {
        Self::with_trace_sink(trace, None)
    }

    pub(crate) fn with_trace_sink(
        trace: Option<CompletionTraceContext>,
        trace_sink: Option<Arc<dyn TraceEventSink>>,
    ) -> Self {
        Self {
            content_parts: Vec::new(),
            content_indexes: HashMap::new(),
            reasoning_summary_parts: Vec::new(),
            raw_reasoning_parts: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_callers: HashMap::new(),
            responses_context_items: Vec::new(),
            tool_stream: ToolStream::new(),
            lifecycle: StreamLifecycle::new(),
            final_usage: None,
            response_id: None,
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
        }

        Ok(())
    }

    pub(crate) fn finish(self, event_tx: &AgentEventSender) -> Result<CompletionResponse> {
        self.finish_inner(event_tx, None)
    }

    #[cfg(test)]
    pub(crate) fn finish_with_trace_events(
        self,
        event_tx: &AgentEventSender,
        trace_events: &Arc<std::sync::Mutex<Vec<pl_trace::TraceEvent>>>,
    ) -> Result<CompletionResponse> {
        self.finish_inner(event_tx, Some(trace_events))
    }

    fn finish_inner(
        mut self,
        event_tx: &AgentEventSender,
        #[cfg_attr(not(test), allow(unused_variables))] trace_events: Option<
            &Arc<std::sync::Mutex<Vec<pl_trace::TraceEvent>>>,
        >,
    ) -> Result<CompletionResponse> {
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
        #[cfg(test)]
        if let Some(target) = trace_events
            && let Some(trace) = self.trace.as_ref()
            && let Ok(mut events) = target.lock()
        {
            events.extend(trace.events());
        }
        let orchestration =
            stream_orchestration_metrics(&self.responses_context_items, &self.tool_calls);
        Ok(CompletionResponse {
            response_id: self.response_id,
            content,
            reasoning_content,
            tool_calls: self.tool_calls,
            responses_context_items: self.responses_context_items,
            orchestration,
            timing: None,
            usage: self.final_usage.unwrap_or_default(),
            model: String::new(),
        })
    }

    fn attach_tool_caller(&mut self, call: &mut ToolCall) {
        call.caller = self
            .tool_call_callers
            .remove(&call.id)
            .or_else(|| self.tool_call_callers.remove(&call.call_id));
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
        action: crate::completion::WebSearchAction,
        event_tx: &AgentEventSender,
    ) {
        self.publish_trace(event_tx, |trace| trace.start_web_search(item_id, action));
    }

    fn record_web_search_completed(
        &mut self,
        item_id: &str,
        action: crate::completion::WebSearchAction,
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

    fn append_content_part(&mut self, item_id: &str, delta: &str) {
        let index = self.content_part_slot(item_id);
        self.content_parts[index].text.push_str(delta);
    }

    fn complete_content_part(&mut self, item_id: &str, text: &str) {
        let index = self.content_part_slot(item_id);
        self.content_parts[index].text = text.to_string();
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
