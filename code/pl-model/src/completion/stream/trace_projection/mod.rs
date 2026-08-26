//! provider 流式增量到 trace part 的投影。
//!
//! 按域拆分:`text` 承载正文与 reasoning 流,`tool` 承载工具与 web search 流,
//! `ids` 承载 item id 解析与别名收敛;本模块保留投影状态、事件记录与
//! 终态收尾。

mod ids;
mod plan;
mod text;
mod tool;

use std::collections::HashMap;
use std::sync::Arc;

use pl_trace::{
    AgentEvent, TraceEvent, TraceEventDraft, TraceEventKind, TraceEventSink, TracePart,
    TracePartAction, TracePartCompletion, TracePartKind, TracePartState, TraceToolFailureKind,
};

use crate::completion::CompletionTraceContext;

pub(crate) struct TraceProjection {
    session_id: String,
    turn_id: String,
    inference_id: String,
    sequence: u64,
    started: HashMap<String, TracePart>,
    active_text_items: HashMap<String, String>,
    active_thinking_items: HashMap<String, String>,
    active_tool_items: HashMap<String, String>,
    input_trace_projections: Arc<HashMap<String, pl_trace::ToolInputTraceProjection>>,
    plan_projections: HashMap<String, plan::PlanProjectionState>,
    segment_occurrences: HashMap<String, u64>,
    events: Vec<TraceEvent>,
    sink: Option<Arc<dyn TraceEventSink>>,
    trace_error: Option<pl_trace::TraceEventSinkError>,
}

impl TraceProjection {
    #[cfg(test)]
    pub(crate) fn new(context: CompletionTraceContext) -> Self {
        Self::with_sink(context, None)
    }

    #[cfg(test)]
    pub(crate) fn with_sink(
        context: CompletionTraceContext,
        sink: Option<Arc<dyn TraceEventSink>>,
    ) -> Self {
        Self::with_sink_and_projections(context, sink, Arc::new(HashMap::new()))
    }

    pub(crate) fn with_sink_and_projections(
        context: CompletionTraceContext,
        sink: Option<Arc<dyn TraceEventSink>>,
        input_trace_projections: Arc<HashMap<String, pl_trace::ToolInputTraceProjection>>,
    ) -> Self {
        let sequence = sink.as_ref().map_or(0, |sink| sink.next_sequence());
        Self {
            session_id: context.session_id,
            turn_id: context.turn_id,
            inference_id: context.inference_id,
            sequence,
            started: HashMap::new(),
            active_text_items: HashMap::new(),
            active_thinking_items: HashMap::new(),
            active_tool_items: HashMap::new(),
            input_trace_projections,
            plan_projections: HashMap::new(),
            segment_occurrences: HashMap::new(),
            events: Vec::new(),
            sink,
            trace_error: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn events(&self) -> Vec<TraceEvent> {
        self.events.clone()
    }

    pub(crate) fn take_trace_error(&mut self) -> Option<pl_trace::TraceEventSinkError> {
        self.trace_error.take()
    }

    pub(crate) fn complete_streaming_items(&mut self) -> Vec<AgentEvent> {
        let item_ids = self
            .started
            .iter()
            .filter(|(_, item)| {
                matches!(item.kind(), TracePartKind::Text | TracePartKind::Thinking)
            })
            .map(|(item_id, _)| item_id.clone())
            .collect::<Vec<_>>();
        let mut events = Vec::new();
        for item_id in item_ids {
            let Some(item) = self.started.get_mut(&item_id) else {
                continue;
            };
            if item.is_terminal() {
                continue;
            }
            let completion = match item.state() {
                TracePartState::Text(_) => TracePartCompletion::Text {
                    authoritative_content: None,
                },
                TracePartState::Thinking(_) => TracePartCompletion::Thinking {
                    authoritative_summary: None,
                },
                TracePartState::Plan(_) => continue,
                TracePartState::Tool(_)
                | TracePartState::Agent(_)
                | TracePartState::Turn(_)
                | TracePartState::Inference(_) => continue,
            };
            let now = unix_seconds();
            if let Err(error) = item.apply(item.command(now, TracePartAction::Complete(completion)))
            {
                self.trace_error.get_or_insert_with(|| {
                    pl_trace::TraceEventSinkError::new(format!(
                        "failed to complete streaming trace item: {error}"
                    ))
                });
                continue;
            }
            let item = item.clone();
            self.record(
                TraceEventKind::TracePartCompleted { item: item.clone() },
                item.updated_at(),
            );
            events.push(AgentEvent::TracePartCompleted { item });
        }
        events
    }

    pub(crate) fn fail_attempt(&mut self, error: &str) -> Vec<AgentEvent> {
        let mut item_ids = self.started.keys().cloned().collect::<Vec<_>>();
        item_ids.sort_by_key(|item_id| {
            self.started
                .get(item_id)
                .map(TracePart::started_sequence)
                .unwrap_or_default()
        });
        let mut events = Vec::new();
        for item_id in item_ids {
            let Some(item) = self.started.get_mut(&item_id) else {
                continue;
            };
            if item.is_terminal() {
                continue;
            }
            let now = unix_seconds();
            if let Err(transition_error) = item.apply(item.command(
                now,
                TracePartAction::Fail {
                    error: error.to_string(),
                    tool_kind: TraceToolFailureKind::Execution,
                },
            )) {
                self.trace_error.get_or_insert_with(|| {
                    pl_trace::TraceEventSinkError::new(format!(
                        "failed to fail open trace item: {transition_error}"
                    ))
                });
                continue;
            }
            let item = item.clone();
            self.record(
                TraceEventKind::TracePartFailed { item: item.clone() },
                item.updated_at(),
            );
            events.push(AgentEvent::TracePartFailed { item });
        }
        events
    }

    pub(crate) fn cancel_attempt(&mut self, reason: &str) -> Vec<AgentEvent> {
        let mut item_ids = self.started.keys().cloned().collect::<Vec<_>>();
        item_ids.sort_by_key(|item_id| {
            self.started
                .get(item_id)
                .map(TracePart::started_sequence)
                .unwrap_or_default()
        });
        let mut events = Vec::new();
        for item_id in item_ids {
            let Some(item) = self.started.get_mut(&item_id) else {
                continue;
            };
            if item.is_terminal() {
                continue;
            }
            let now = unix_seconds();
            if let Err(error) = item.apply(item.command(
                now,
                TracePartAction::Cancel {
                    reason: reason.to_string(),
                },
            )) {
                self.trace_error.get_or_insert_with(|| {
                    pl_trace::TraceEventSinkError::new(format!(
                        "failed to cancel open trace item: {error}"
                    ))
                });
                continue;
            }
            let item = item.clone();
            self.record(
                TraceEventKind::TracePartFailed { item: item.clone() },
                item.updated_at(),
            );
            events.push(AgentEvent::TracePartFailed { item });
        }
        events
    }

    fn record(&mut self, kind: TraceEventKind, timestamp: i64) {
        let event = if let Some(sink) = &self.sink {
            match sink.emit(TraceEventDraft::new(timestamp, kind)) {
                Ok(event) => event,
                Err(error) => {
                    if self.trace_error.is_none() {
                        self.trace_error = Some(error);
                    }
                    return;
                }
            }
        } else {
            TraceEvent {
                session_id: self.session_id.clone(),
                sequence: self.sequence,
                timestamp,
                kind,
            }
        };
        self.sequence = event.sequence.saturating_add(1);
        self.events.push(event);
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod unit_tests;
