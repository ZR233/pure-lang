//! provider 流式增量到 trace part 的投影。
//!
//! 按域拆分:`text` 承载正文与 reasoning 流,`tool` 承载工具与 web search 流,
//! `ids` 承载 item id 解析与别名收敛;本模块保留投影状态、事件记录与
//! 终态收尾。

mod ids;
mod text;
mod tool;

use std::collections::HashMap;
use std::sync::Arc;

use pl_trace::{
    AgentEvent, TraceEvent, TraceEventDraft, TraceEventKind, TraceEventSink, TracePart,
    TracePartAction, TracePartCompletion, TracePartState, TraceToolFailureKind,
};

use crate::completion::CompletionTraceContext;

pub(crate) struct TraceProjection {
    turn_id: String,
    inference_id: String,
    started: HashMap<String, TracePart>,
    active_text_items: HashMap<String, String>,
    active_thinking_items: HashMap<String, String>,
    active_tool_items: HashMap<String, String>,
    segment_occurrences: HashMap<String, u64>,
    events: Vec<TraceEvent>,
    sink: Arc<dyn TraceEventSink>,
    trace_error: Option<pl_trace::TraceEventSinkError>,
}

impl TraceProjection {
    #[cfg(test)]
    pub(crate) fn new(context: CompletionTraceContext) -> Self {
        Self::with_sink(context, None)
    }

    pub(crate) fn with_sink(
        context: CompletionTraceContext,
        sink: Option<Arc<dyn TraceEventSink>>,
    ) -> Self {
        let sink = sink.unwrap_or_else(|| {
            Arc::new(pl_trace::InMemoryTraceEventSink::new(
                context.session_id.clone(),
                0,
            ))
        });
        Self {
            turn_id: context.turn_id,
            inference_id: context.inference_id,
            started: HashMap::new(),
            active_text_items: HashMap::new(),
            active_thinking_items: HashMap::new(),
            active_tool_items: HashMap::new(),
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
        let items = self
            .started
            .values()
            .filter(|item| !item.is_terminal())
            .cloned()
            .collect::<Vec<_>>();
        let mut events = Vec::new();
        for item in items {
            let completion = match item.state() {
                TracePartState::Text(_) => TracePartCompletion::Text {
                    authoritative_content: None,
                },
                TracePartState::Thinking(_) => TracePartCompletion::Thinking {
                    authoritative_summary: None,
                },
                TracePartState::Tool(_)
                | TracePartState::Agent(_)
                | TracePartState::Turn(_)
                | TracePartState::Inference(_) => continue,
            };
            events.extend(self.apply_item(item.item_id(), TracePartAction::Complete(completion)));
        }
        events
    }

    pub(crate) fn fail_attempt(&mut self, error: &str) -> Vec<AgentEvent> {
        self.settle_open(TracePartAction::Fail {
            error: error.to_owned(),
            tool_kind: TraceToolFailureKind::Execution,
        })
    }

    pub(crate) fn cancel_attempt(&mut self, reason: &str) -> Vec<AgentEvent> {
        self.settle_open(TracePartAction::Cancel {
            reason: reason.to_owned(),
        })
    }

    fn settle_open(&mut self, action: TracePartAction) -> Vec<AgentEvent> {
        let mut items = self
            .started
            .values()
            .filter(|item| !item.is_terminal())
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by_key(TracePart::started_sequence);
        let mut events = Vec::new();
        for item in items {
            events.extend(self.apply_item(item.item_id(), action.clone()));
        }
        events
    }

    fn start_item(&mut self, item_id: String, state: TracePartState) -> Vec<AgentEvent> {
        self.record(TraceEventDraft::start(
            unix_seconds(),
            self.turn_id.clone(),
            item_id,
            pl_trace::TracePartSource::Model,
            state,
        ))
        .into_iter()
        .collect()
    }

    fn apply_item(&mut self, item_id: &str, action: TracePartAction) -> Vec<AgentEvent> {
        self.record(TraceEventDraft::apply(
            unix_seconds(),
            self.turn_id.clone(),
            item_id.to_owned(),
            action,
        ))
        .into_iter()
        .collect()
    }

    /// Only returned canonical events enter the producer's read projection and live stream.
    fn record(&mut self, draft: TraceEventDraft) -> Option<AgentEvent> {
        let event = match self.sink.emit(draft) {
            Ok(event) => event,
            Err(error) => {
                self.trace_error.get_or_insert(error);
                return None;
            }
        };
        let live = match &event.kind {
            TraceEventKind::TracePartStarted { item } => {
                self.started.insert(item.item_id().to_owned(), item.clone());
                AgentEvent::TracePartStarted { item: item.clone() }
            }
            TraceEventKind::TracePartCompleted { item } => {
                self.started.insert(item.item_id().to_owned(), item.clone());
                AgentEvent::TracePartCompleted { item: item.clone() }
            }
            TraceEventKind::TracePartFailed { item } => {
                self.started.insert(item.item_id().to_owned(), item.clone());
                AgentEvent::TracePartFailed { item: item.clone() }
            }
            TraceEventKind::TracePartDelta { event } => {
                let item = self.started.get_mut(&event.item_id)?;
                if let Err(error) = item.apply(item.command(
                    event.updated_at,
                    TracePartAction::Append(event.delta.clone()),
                )) {
                    self.trace_error.get_or_insert_with(|| {
                        pl_trace::TraceEventSinkError::new(error.to_string())
                    });
                    return None;
                }
                AgentEvent::TracePartDelta {
                    event: event.clone(),
                }
            }
            TraceEventKind::InteractionChanged { event } => AgentEvent::InteractionChanged {
                event: event.clone(),
            },
            TraceEventKind::SkillActivated { activation } => AgentEvent::SkillActivated {
                activation: activation.clone(),
            },
            TraceEventKind::EnabledToolsRecorded { .. } => return None,
        };
        self.events.push(event);
        Some(live)
    }
}

fn unix_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod test_support {
    use std::sync::Arc;

    use pl_trace::{AgentEvent, TraceEventSink, TracePart, TracePartKind};

    use super::{CompletionTraceContext, TraceProjection};

    pub(super) fn trace() -> TraceProjection {
        TraceProjection::new(test_trace_context("inference-1"))
    }

    pub(super) fn test_trace_context(inference_id: &str) -> CompletionTraceContext {
        CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: inference_id.to_string(),
        }
    }

    pub(super) fn trace_with_sink(sink: Arc<dyn TraceEventSink>) -> TraceProjection {
        TraceProjection::with_sink(test_trace_context("inference-1"), Some(sink))
    }

    /// AgentEvent 中 trace part 相关载荷的统一提取视图，供投影测试按阶段与 kind 过滤。
    pub(super) enum TracePartEvent<'a> {
        Started(&'a TracePart),
        Delta {
            item_id: &'a str,
            kind: TracePartKind,
        },
        Completed(&'a TracePart),
        Failed(&'a TracePart),
    }

    pub(super) fn trace_part_event(event: &AgentEvent) -> Option<TracePartEvent<'_>> {
        match event {
            AgentEvent::TracePartStarted { item } => Some(TracePartEvent::Started(item)),
            AgentEvent::TracePartDelta { event } => Some(TracePartEvent::Delta {
                item_id: event.item_id.as_str(),
                kind: event.kind(),
            }),
            AgentEvent::TracePartCompleted { item } => Some(TracePartEvent::Completed(item)),
            AgentEvent::TracePartFailed { item } => Some(TracePartEvent::Failed(item)),
            AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::TodoListUpdated { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::Done
            | AgentEvent::Error { .. } => None,
        }
    }

    pub(super) fn delta_item_id(event: &AgentEvent) -> Option<String> {
        match trace_part_event(event)? {
            TracePartEvent::Delta {
                item_id,
                kind: TracePartKind::Thinking,
            } => Some(item_id.to_string()),
            _ => None,
        }
    }

    pub(super) fn completed_thinking_item(event: &AgentEvent) -> Option<&TracePart> {
        match trace_part_event(event)? {
            TracePartEvent::Completed(item) if item.kind() == TracePartKind::Thinking => Some(item),
            _ => None,
        }
    }

    pub(super) fn started_tool_item(event: &AgentEvent) -> Option<&TracePart> {
        match trace_part_event(event)? {
            TracePartEvent::Started(item) if item.kind() == TracePartKind::Tool => Some(item),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use pl_trace::{TraceEvent, TraceEventDraft, TraceEventSink, TraceEventSinkError};

    use super::TraceProjection;
    use super::test_support::{test_trace_context, trace, trace_part_event, trace_with_sink};

    #[derive(Debug)]
    struct RejectAfterFirstTraceSink {
        inner: pl_trace::InMemoryTraceEventSink,
        attempts: AtomicUsize,
    }

    impl RejectAfterFirstTraceSink {
        fn new() -> Self {
            Self {
                inner: pl_trace::InMemoryTraceEventSink::new("session-1", 0),
                attempts: AtomicUsize::new(0),
            }
        }
    }

    impl TraceEventSink for RejectAfterFirstTraceSink {
        fn emit(&self, draft: TraceEventDraft) -> Result<TraceEvent, TraceEventSinkError> {
            if self.attempts.fetch_add(1, Ordering::SeqCst) > 0 {
                return Err(TraceEventSinkError::new("injected trace sink rejection"));
            }
            self.inner.emit(draft)
        }

        fn next_sequence(&self) -> u64 {
            self.inner.next_sequence()
        }
    }

    #[test]
    fn rejected_trace_event_is_not_broadcast() {
        let sink = Arc::new(RejectAfterFirstTraceSink::new());
        let mut trace = trace_with_sink(sink.clone());

        let events = trace.append_text_delta(
            "provider-text",
            pl_trace::TraceTextChannel::Final,
            "must not escape".to_string(),
        );

        assert!(matches!(
            events.as_slice(),
            [pl_trace::AgentEvent::TracePartStarted { .. }]
        ));
        assert_eq!(sink.inner.events().len(), 1);
        assert_eq!(trace.events().len(), 1);
        assert_eq!(
            trace
                .take_trace_error()
                .expect("sink rejection must remain visible")
                .message(),
            "injected trace sink rejection"
        );
    }

    #[test]
    fn trace_sink_sequence_offsets_started_sequence() {
        let first_sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 10));
        let second_sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 20));
        let mut first =
            TraceProjection::with_sink(test_trace_context("turn-1-inf-0"), Some(first_sink));
        let mut second =
            TraceProjection::with_sink(test_trace_context("turn-1-inf-1"), Some(second_sink));

        let first_sequence = first
            .start_thinking("thinking", 0)
            .iter()
            .find_map(|event| match trace_part_event(event)? {
                super::test_support::TracePartEvent::Started(item) => Some(item.started_sequence()),
                _ => None,
            })
            .expect("first started sequence");
        let second_sequence = second
            .start_thinking("thinking", 0)
            .iter()
            .find_map(|event| match trace_part_event(event)? {
                super::test_support::TracePartEvent::Started(item) => Some(item.started_sequence()),
                _ => None,
            })
            .expect("second started sequence");

        assert_eq!(first_sequence, 10);
        assert_eq!(second_sequence, 20);
    }

    #[test]
    fn failed_sampling_attempt_invalidates_completed_and_streaming_parts() {
        let mut trace = trace();
        let _ = trace.append_text_delta(
            "msg_1",
            pl_trace::TraceTextChannel::Final,
            "partial".to_string(),
        );
        let _ = trace.complete_text(
            "msg_1",
            pl_trace::TraceTextChannel::Final,
            Some("partial".to_string()),
        );
        let _ = trace.append_thinking_delta("thinking", 0, "reasoning".to_string());

        let failed = trace
            .fail_attempt("connection lost")
            .iter()
            .filter_map(|event| match trace_part_event(event)? {
                super::test_support::TracePartEvent::Failed(item) => Some((
                    item.item_id().to_string(),
                    item.failure().map(str::to_string),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(
            failed,
            vec![(
                "inference-1-reasoning-1".to_string(),
                Some("connection lost".to_string()),
            )]
        );
    }
}
