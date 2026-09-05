//! 流事件累积器：把 canonical `ModelStreamEvent` 序列累积为 `CompletionResponse`。
//!
//! 累积器同时驱动 trace 投影（`TraceProjection`）并把投影事件发布到宿主
//! `AgentEventSender`；生命周期合法性与工具调用增量分别由 `lifecycle` 与
//! `tool_stream` 子状态机保证。

use std::collections::HashMap;
use std::sync::Arc;

use pl_protocol::{
    InferenceOrchestrationMetrics, PureError, ResponsesContextItem, ResponsesContextItemKind,
    Result, ToolCallCaller, UsageReport,
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
    final_usage: Option<UsageReport>,
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
            accounting: pl_protocol::InferenceAccounting {
                usage: self.final_usage.unwrap_or_default(),
                ..Default::default()
            },
            model: String::new(),
        })
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
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use pl_trace::{AgentEvent, TraceDelta, TraceEventKind, TracePartKind};

    use super::*;
    use crate::completion::stream::test_support::*;

    use pretty_assertions::assert_eq;

    #[test]
    fn text_delta_is_published_before_stream_completion() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 0));
        let mut accumulator = StreamCompletionAccumulator::with_trace_sink(
            Some(CompletionTraceContext {
                session_id: "session-1".to_string(),
                turn_id: "turn-1".to_string(),
                inference_id: "inf-1".to_string(),
            }),
            Some(sink.clone()),
        );

        accumulator
            .apply(
                ModelStreamEvent::text_started(
                    "assistant".to_string(),
                    pl_trace::TraceTextChannel::Final,
                ),
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(final_delta("assistant", "a"), &event_tx)
            .unwrap();

        let events = sink.events();
        assert!(events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartStarted { item }
                if item.kind() == TracePartKind::Text
        )));
        assert!(events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartDelta { event }
                if matches!(&event.delta, TraceDelta::Text { delta, .. } if delta == "a")
        )));
        assert!(!events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if item.kind() == TracePartKind::Text
        )));
    }

    #[test]
    fn stream_accumulator_returns_content_and_reasoning_content() {
        let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
        }));
        let mut decoder = tagged_decoder();

        accumulator
            .apply(summary_started("thinking"), &event_tx)
            .unwrap();
        accumulator
            .apply(summary_delta("thinking", 0, "先比较整数位。"), &event_tx)
            .unwrap();
        apply_tagged(
            &mut decoder,
            &mut accumulator,
            final_delta("final", "<final>9.11 更大。</final>"),
            &event_tx,
        );

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.content.as_deref(), Some("9.11 更大。"));
        assert_eq!(response.content.as_deref(), Some("9.11 更大。"));
        assert_eq!(response.reasoning_content, None);
        assert!(response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if item.kind() == TracePartKind::Thinking
                    && trace_part_text(item) == "先比较整数位。"
        )));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TracePartStarted { .. }
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TracePartDelta { .. }
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TracePartStarted { .. }
        ));
        assert!(matches!(
            event_rx.try_recv().unwrap(),
            AgentEvent::TracePartDelta { .. }
        ));
    }

    #[test]
    fn stream_accumulator_preserves_response_id() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(None);

        accumulator
            .apply(
                ModelStreamEvent::ResponseStarted {
                    response_id: Some("resp_started".to_string()),
                },
                &event_tx,
            )
            .unwrap();
        accumulator
            .apply(
                ModelStreamEvent::Completed {
                    response_id: Some("resp_completed".to_string()),
                },
                &event_tx,
            )
            .unwrap();

        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.response_id.as_deref(), Some("resp_completed"));
    }

    #[test]
    fn stream_accumulator_streams_commentary_without_content() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
        }));
        let mut decoder = tagged_decoder();

        for delta in [
            "<comm",
            "entary>检查配置。</commentary>",
            "<final>完成。</final>",
        ] {
            apply_tagged(
                &mut decoder,
                &mut accumulator,
                final_delta("final", delta),
                &event_tx,
            );
        }

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.content.as_deref(), Some("完成。"));
        assert_eq!(response.content.as_deref(), Some("完成。"));
        assert!(response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if trace_text_channel(item) == Some(pl_trace::TraceTextChannel::Commentary)
                    && trace_part_text(item) == "检查配置。"
        )));
        assert!(response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if trace_text_channel(item) == Some(pl_trace::TraceTextChannel::Final)
            && trace_part_text(item) == "完成。"
        )));
    }

    #[test]
    fn stream_accumulator_projects_tagged_raw_reasoning_without_visible_text() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
        }));
        let mut decoder = tagged_decoder();

        for delta in [
            "<commentary>正在分析日志。</commentary>",
            "<final>完成。</final>",
        ] {
            apply_tagged(
                &mut decoder,
                &mut accumulator,
                ModelStreamEvent::ReasoningRawDelta {
                    id: "thinking".to_string(),
                    content_index: 0,
                    delta: delta.to_string(),
                },
                &event_tx,
            );
        }

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.content, None);
        assert_eq!(
            response.reasoning_content.as_deref(),
            Some("<commentary>正在分析日志。</commentary><final>完成。</final>")
        );
        assert!(response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if item.kind() == TracePartKind::Thinking
                    && trace_part_text(item)
                        == "<commentary>正在分析日志。</commentary><final>完成。</final>"
        )));
        assert!(!response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if item.kind() == TracePartKind::Text
        )));
    }

    #[test]
    fn stream_accumulator_splits_repeated_tagged_commentary_and_final_blocks() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
        }));
        let mut decoder = tagged_decoder();

        for delta in [
            "<commentary>A</commentary><final>B</final>",
            "<commentary>C</commentary><final>D</final>",
        ] {
            apply_tagged(
                &mut decoder,
                &mut accumulator,
                final_delta("final", delta),
                &event_tx,
            );
        }

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();
        let completed_text = response
            .trace_events
            .iter()
            .filter_map(|event| match &event.kind {
                TraceEventKind::TracePartCompleted { item }
                    if item.kind() == TracePartKind::Text =>
                {
                    Some((
                        item.item_id(),
                        trace_text_channel(item),
                        trace_part_text(item),
                    ))
                }
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartDelta { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(response.content.as_deref(), Some("BD"));
        assert_eq!(response.content.as_deref(), Some("BD"));
        assert_eq!(
            completed_text,
            vec![
                (
                    "inf-1-text-commentary-1",
                    Some(pl_trace::TraceTextChannel::Commentary),
                    "A".to_string(),
                ),
                (
                    "inf-1-text-final-1",
                    Some(pl_trace::TraceTextChannel::Final),
                    "B".to_string(),
                ),
                (
                    "inf-1-text-commentary-2",
                    Some(pl_trace::TraceTextChannel::Commentary),
                    "C".to_string(),
                ),
                (
                    "inf-1-text-final-2",
                    Some(pl_trace::TraceTextChannel::Final),
                    "D".to_string(),
                ),
            ]
        );
    }

    #[test]
    fn stream_accumulator_projects_untagged_reasoning_without_visible_text() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
        }));

        accumulator
            .apply(
                ModelStreamEvent::ReasoningRawDelta {
                    id: "thinking".to_string(),
                    content_index: 0,
                    delta: "先比较整数位。".to_string(),
                },
                &event_tx,
            )
            .unwrap();

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.content, None);
        assert_eq!(
            response.reasoning_content.as_deref(),
            Some("先比较整数位。")
        );
        assert!(response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if item.kind() == TracePartKind::Thinking
                    && trace_part_text(item) == "先比较整数位。"
        )));
        assert!(!response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if item.kind() == TracePartKind::Text
        )));
    }

    #[test]
    fn stream_accumulator_treats_untagged_display_text_as_final() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
        }));

        accumulator
            .apply(final_started("final"), &event_tx)
            .unwrap();
        accumulator
            .apply(final_delta("final", "plain text"), &event_tx)
            .unwrap();
        apply_completed(&mut accumulator, &event_tx);

        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.content.as_deref(), Some("plain text"));
        assert!(response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if trace_text_channel(item) == Some(pl_trace::TraceTextChannel::Final)
                    && trace_part_text(item) == "plain text"
        )));
    }

    #[test]
    fn stream_accumulator_uses_authoritative_completed_text_for_response_content() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
        }));

        accumulator
            .apply(final_started("msg_1"), &event_tx)
            .unwrap();
        accumulator
            .apply(final_delta("msg_1", "partial"), &event_tx)
            .unwrap();
        accumulator
            .apply(
                completed_text(
                    "msg_1",
                    pl_trace::TraceTextChannel::Final,
                    Some("final text"),
                ),
                &event_tx,
            )
            .unwrap();
        apply_completed(&mut accumulator, &event_tx);

        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(response.content.as_deref(), Some("final text"));
        assert!(response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if trace_text_channel(item) == Some(pl_trace::TraceTextChannel::Final)
                    && trace_part_text(item) == "final text"
        )));
    }

    #[test]
    fn stream_accumulator_creates_part_for_authoritative_completion_without_delta() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
        }));

        accumulator
            .apply(commentary_started("msg_progress"), &event_tx)
            .unwrap();
        accumulator
            .apply(
                completed_text(
                    "msg_progress",
                    pl_trace::TraceTextChannel::Commentary,
                    Some("已完成检查"),
                ),
                &event_tx,
            )
            .unwrap();
        apply_completed(&mut accumulator, &event_tx);

        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert!(response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartStarted { item }
                if trace_text_channel(item) == Some(pl_trace::TraceTextChannel::Commentary)
                    && trace_part_text(item).is_empty()
        )));
        assert!(response.trace_events.iter().any(|event| matches!(
            &event.kind,
            TraceEventKind::TracePartCompleted { item }
                if trace_text_channel(item) == Some(pl_trace::TraceTextChannel::Commentary)
                    && trace_part_text(item) == "已完成检查"
        )));
    }

    #[test]
    fn stream_accumulator_preserves_unknown_proposed_plan_tags_as_text() {
        let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
        let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inf-1".to_string(),
        }));
        let mut decoder = tagged_decoder();

        for delta in [
            "<commentary>Intro</commentary>\n<prop",
            "osed_plan>\n- step\n",
            "</proposed_plan>\n<final>Outro</final>",
        ] {
            apply_tagged(
                &mut decoder,
                &mut accumulator,
                final_delta("final", delta),
                &event_tx,
            );
        }

        apply_completed(&mut accumulator, &event_tx);
        let response = finish_with_trace(accumulator, &event_tx).unwrap();

        assert_eq!(
            response.content.as_deref(),
            Some("\n<proposed_plan>\n- step\n</proposed_plan>\nOutro")
        );
    }
}
