use pl_trace::{
    AgentEvent, TraceEvent, TraceEventDraft, TraceEventKind, TraceEventSink, TraceEventSinkError,
    TracePart, TracePartKind, TraceTextChannel, TraceTextState, TraceToolState,
};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::super::tool_stream::ToolCallAccumulatorSnapshot;
use super::*;
use crate::completion::{ToolCall, ToolCallPayload};

fn trace() -> TraceProjection {
    TraceProjection::new(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inference-1".to_string(),
    })
}

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
    let mut trace = TraceProjection::with_sink(
        CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inference-1".to_string(),
        },
        Some(sink.clone()),
    );

    let events = trace.append_text_delta(
        "provider-text",
        TraceTextChannel::Final,
        "must not escape".to_string(),
    );

    assert!(matches!(
        events.as_slice(),
        [AgentEvent::TracePartStarted { .. }]
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
fn repeated_provider_thinking_id_after_completion_gets_new_part_id() {
    let mut trace = trace();

    let first = trace.append_thinking_delta("thinking", 0, "first".to_string());
    let first_completed = trace.complete_thinking("thinking", None);
    let second = trace.append_thinking_delta("thinking", 0, "second".to_string());
    let second_completed = trace.complete_thinking("thinking", None);

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
fn reasoning_summary_sections_get_distinct_part_ids() {
    let mut trace = trace();

    let first = trace.append_thinking_delta("thinking", 0, "first".to_string());
    let second = trace.append_thinking_delta("thinking", 1, "second".to_string());
    let completed = trace.complete_thinking(
        "thinking",
        Some(vec!["first done".to_string(), "second done".to_string()]),
    );

    let first_delta = first
        .into_iter()
        .find_map(delta_item_id)
        .expect("first delta");
    let second_delta = second
        .into_iter()
        .find_map(delta_item_id)
        .expect("second delta");
    let completed = completed
        .into_iter()
        .filter_map(completed_thinking_item)
        .map(|item| (item.item_id().to_string(), trace_part_text(&item)))
        .collect::<Vec<_>>();

    assert_eq!(first_delta, "inference-1-reasoning-1");
    assert_eq!(second_delta, "inference-1-reasoning-2");
    assert_eq!(
        completed,
        vec![
            (
                "inference-1-reasoning-1".to_string(),
                "first done".to_string(),
            ),
            (
                "inference-1-reasoning-2".to_string(),
                "second done".to_string(),
            ),
        ]
    );
}

#[test]
fn raw_reasoning_starts_the_part_and_later_summary_updates_the_same_part() {
    let mut trace = trace();

    let raw = trace.append_reasoning_content_delta("thinking", 0, "raw".to_string());
    let summary = trace.append_thinking_delta("thinking", 0, "summary".to_string());
    let completed = trace
        .complete_thinking("thinking", Some(vec!["summary done".to_string()]))
        .into_iter()
        .find_map(completed_thinking_item)
        .expect("completed reasoning part");

    assert_eq!(
        raw.into_iter().find_map(delta_item_id),
        Some("inference-1-reasoning-1".to_string())
    );
    assert_eq!(
        summary.into_iter().find_map(delta_item_id),
        Some("inference-1-reasoning-1".to_string())
    );
    assert_eq!(trace_part_text(&completed), "summary done");
    assert_eq!(
        completed
            .thinking()
            .expect("thinking part")
            .content()
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<String>(),
        "raw"
    );
}

#[test]
fn empty_reasoning_delta_does_not_create_a_revision_gap() {
    let sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 0));
    let mut trace = TraceProjection::with_sink(
        CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inference-1".to_string(),
        },
        Some(sink.clone()),
    );

    let started = trace.append_reasoning_content_delta("thinking", 0, String::new());
    let first = trace.append_reasoning_content_delta("thinking", 0, "first".to_string());
    let ignored = trace.append_reasoning_content_delta("thinking", 0, String::new());
    let second = trace.append_reasoning_content_delta("thinking", 0, " second".to_string());

    assert!(matches!(
        started.as_slice(),
        [AgentEvent::TracePartStarted { .. }]
    ));
    assert!(ignored.is_empty());
    assert!(first.iter().any(|event| matches!(
        event,
        AgentEvent::TracePartDelta { event } if event.revision == 1
    )));
    assert!(second.iter().any(|event| matches!(
        event,
        AgentEvent::TracePartDelta { event } if event.revision == 2
    )));
    assert!(trace.take_trace_error().is_none());
    assert_eq!(
        sink.events()
            .into_iter()
            .filter_map(|event| match event.kind {
                TraceEventKind::TracePartDelta { event } => Some(event.revision),
                TraceEventKind::TracePartStarted { .. }
                | TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn provider_reasoning_chunk_indices_are_local_to_distinct_parts() {
    let mut trace = trace();

    let first = trace.append_reasoning_content_delta("thinking", 0, "first raw".to_string());
    let second = trace.append_reasoning_content_delta("thinking", 1, "second raw".to_string());
    let completed = trace
        .complete_thinking(
            "thinking",
            Some(vec![
                "first summary".to_string(),
                "second summary".to_string(),
            ]),
        )
        .into_iter()
        .filter_map(completed_thinking_item)
        .collect::<Vec<_>>();

    assert_eq!(
        first.into_iter().find_map(delta_item_id),
        Some("inference-1-reasoning-1".to_string())
    );
    assert_eq!(
        second.into_iter().find_map(delta_item_id),
        Some("inference-1-reasoning-2".to_string())
    );
    assert_eq!(completed.len(), 2);
    assert_eq!(trace_part_text(&completed[0]), "first summary");
    assert_eq!(trace_part_text(&completed[1]), "second summary");
    assert_eq!(
        completed[0]
            .thinking()
            .expect("first thinking part")
            .content()
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<String>(),
        "first raw"
    );
    assert_eq!(
        completed[1]
            .thinking()
            .expect("second thinking part")
            .content()
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<String>(),
        "second raw"
    );
}

#[test]
fn raw_only_reasoning_is_preserved_in_the_authoritative_part() {
    let mut trace = trace();

    let _ = trace.append_reasoning_content_delta("thinking", 0, "raw only".to_string());
    let completed = trace
        .complete_thinking("thinking", None)
        .into_iter()
        .find_map(completed_thinking_item)
        .expect("completed reasoning part");

    assert!(
        completed
            .thinking()
            .expect("thinking part")
            .summary()
            .is_empty()
    );
    assert_eq!(
        completed
            .thinking()
            .expect("thinking part")
            .content()
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<String>(),
        "raw only"
    );
}

#[test]
fn generated_part_ids_are_scoped_to_inference() {
    let mut first = trace();
    let mut second = TraceProjection::new(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inference-2".to_string(),
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
fn trace_sink_sequence_offsets_started_sequence() {
    let first_sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 10));
    let second_sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 20));
    let mut first = TraceProjection::with_sink(
        CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-0".to_string(),
        },
        Some(first_sink),
    );
    let mut second = TraceProjection::with_sink(
        CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "turn-1-inf-1".to_string(),
        },
        Some(second_sink),
    );

    let first_sequence = first
        .start_thinking("thinking", 0)
        .into_iter()
        .find_map(started_sequence)
        .expect("first started sequence");
    let second_sequence = second
        .start_thinking("thinking", 0)
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

    assert_eq!(completed.text().expect("text part").content(), "final text");
    assert_eq!(completed.revision(), 2);
}

#[test]
fn failed_sampling_attempt_invalidates_completed_and_streaming_parts() {
    let mut trace = trace();
    let _ = trace.append_text_delta("msg_1", TraceTextChannel::Final, "partial".to_string());
    let _ = trace.complete_text(
        "msg_1",
        TraceTextChannel::Final,
        Some("partial".to_string()),
    );
    let _ = trace.append_thinking_delta("thinking", 0, "reasoning".to_string());

    let failed = trace
        .fail_attempt("connection lost")
        .into_iter()
        .filter_map(|event| match event {
            AgentEvent::TracePartFailed { item } => Some((
                item.item_id().to_string(),
                item.failure().map(str::to_string),
            )),
            AgentEvent::TracePartStarted { .. }
            | AgentEvent::TracePartDelta { .. }
            | AgentEvent::TracePartCompleted { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::TodoListUpdated { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::Done
            | AgentEvent::Error { .. } => None,
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

#[test]
fn update_tool_trace_keeps_streaming_tool_status_after_arguments_delta() {
    let mut trace = trace();
    let snapshot = ToolCallAccumulatorSnapshot {
        id: "provider-tool-1".to_string(),
        trace_id: "provider-tool-1".to_string(),
        call_id: Some("call-1".to_string()),
        name: "exec".to_string(),
    };
    let _ = trace.append_tool_arguments_delta(&snapshot, "{\"cmd\":\"ec".to_string());
    let updated = trace
        .update_tool_trace(&ToolCall {
            id: "provider-tool-1".to_string(),
            call_id: "call-1".to_string(),
            name: "exec".to_string(),
            payload: ToolCallPayload::Function {
                arguments: serde_json::json!({"cmd": "echo hi"}),
            },
            invalid_arguments: None,
            caller: None,
        })
        .into_iter()
        .find_map(started_tool_item)
        .expect("updated tool snapshot");

    assert_eq!(updated.item_id(), "turn-1-provider-tool-1");
    assert!(matches!(
        updated.tool().map(|tool| tool.state()),
        Some(TraceToolState::Streaming(_))
    ));
    assert_eq!(updated.revision(), 2);
    let tool = updated.tool().expect("tool metadata");
    assert_eq!(tool.invocation().arguments(), "{\"cmd\":\"echo hi\"}");
}

#[test]
fn late_provider_tool_id_keeps_original_trace_part_id() {
    let mut trace = trace();
    let early = ToolCallAccumulatorSnapshot {
        id: "call-1".to_string(),
        trace_id: "call-1".to_string(),
        call_id: Some("call-1".to_string()),
        name: "exec".to_string(),
    };
    let late = ToolCallAccumulatorSnapshot {
        id: "provider-tool-1".to_string(),
        trace_id: "call-1".to_string(),
        call_id: Some("call-1".to_string()),
        name: "exec".to_string(),
    };

    let first_delta = trace
        .append_tool_arguments_delta(&early, "{\"cmd\":\"ec".to_string())
        .into_iter()
        .find_map(tool_delta_item_id)
        .expect("first tool delta");
    let second_delta = trace
        .append_tool_arguments_delta(&late, "ho hi\"}".to_string())
        .into_iter()
        .find_map(tool_delta_item_id)
        .expect("second tool delta");
    let updated = trace
        .update_tool_trace(&ToolCall {
            id: "provider-tool-1".to_string(),
            call_id: "call-1".to_string(),
            name: "exec".to_string(),
            payload: ToolCallPayload::Function {
                arguments: serde_json::json!({"cmd": "echo hi"}),
            },
            invalid_arguments: None,
            caller: None,
        })
        .into_iter()
        .find_map(started_tool_item)
        .expect("updated tool snapshot");

    assert_eq!(first_delta, "turn-1-call-1");
    assert_eq!(second_delta, "turn-1-call-1");
    assert_eq!(updated.item_id(), "turn-1-call-1");
    assert_eq!(updated.revision(), 3);
    let tool = updated.tool().expect("tool metadata");
    assert_eq!(
        tool.invocation().provider_item_id(),
        Some("provider-tool-1")
    );
    assert_eq!(tool.invocation().call_id(), Some("call-1"));
}

#[test]
fn tool_metadata_and_argument_deltas_share_one_revision_chain() {
    let sink = Arc::new(pl_trace::InMemoryTraceEventSink::new("session-1", 0));
    let mut trace = TraceProjection::with_sink(
        CompletionTraceContext {
            session_id: "session-1".to_string(),
            turn_id: "turn-1".to_string(),
            inference_id: "inference-1".to_string(),
        },
        Some(sink.clone()),
    );
    let early = ToolCallAccumulatorSnapshot {
        id: "call-1".to_string(),
        trace_id: "call-1".to_string(),
        call_id: Some("call-1".to_string()),
        name: "exec".to_string(),
    };
    let late = ToolCallAccumulatorSnapshot {
        id: "provider-tool-1".to_string(),
        trace_id: "call-1".to_string(),
        call_id: Some("call-1".to_string()),
        name: "exec".to_string(),
    };

    let first = trace.append_tool_arguments_delta(&early, "{\"cmd\":\"ec".to_string());
    let ignored = trace.append_tool_arguments_delta(&early, String::new());
    let second = trace.append_tool_arguments_delta(&late, "ho hi\"}".to_string());
    let canonical = trace.update_tool_trace(&ToolCall {
        id: "provider-tool-1".to_string(),
        call_id: "call-1".to_string(),
        name: "exec".to_string(),
        payload: ToolCallPayload::Function {
            arguments: serde_json::json!({"cmd": "echo hi"}),
        },
        invalid_arguments: None,
        caller: None,
    });

    assert!(ignored.is_empty());
    assert!(first.iter().any(|event| matches!(
        event,
        AgentEvent::TracePartDelta { event } if event.revision == 1
    )));
    assert!(second.iter().any(|event| matches!(
        event,
        AgentEvent::TracePartStarted { item } if item.revision() == 2
    )));
    assert!(second.iter().any(|event| matches!(
        event,
        AgentEvent::TracePartDelta { event } if event.revision == 3
    )));
    assert!(canonical.iter().any(|event| matches!(
        event,
        AgentEvent::TracePartStarted { item } if item.revision() == 3
    )));
    assert!(trace.take_trace_error().is_none());
    assert_eq!(
        sink.events()
            .into_iter()
            .filter_map(|event| match event.kind {
                TraceEventKind::TracePartStarted { item } => Some(("snapshot", item.revision())),
                TraceEventKind::TracePartDelta { event } => Some(("delta", event.revision)),
                TraceEventKind::TracePartCompleted { .. }
                | TraceEventKind::TracePartFailed { .. }
                | TraceEventKind::InteractionChanged { .. }
                | TraceEventKind::SkillActivated { .. }
                | TraceEventKind::EnabledToolsRecorded { .. } => None,
            })
            .collect::<Vec<_>>(),
        vec![
            ("snapshot", 0),
            ("delta", 1),
            ("snapshot", 2),
            ("delta", 3),
            ("snapshot", 3),
        ]
    );
}

fn started_sequence(event: AgentEvent) -> Option<u64> {
    match event {
        AgentEvent::TracePartStarted { item } => Some(item.started_sequence()),
        AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::TodoListUpdated { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. } => None,
    }
}

fn delta_item_id(event: AgentEvent) -> Option<String> {
    match event {
        AgentEvent::TracePartDelta { event } if event.kind() == TracePartKind::Thinking => {
            Some(event.item_id)
        }
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::TodoListUpdated { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. }
        | AgentEvent::TracePartDelta { .. } => None,
    }
}

fn tool_delta_item_id(event: AgentEvent) -> Option<String> {
    match event {
        AgentEvent::TracePartDelta { event } if event.kind() == TracePartKind::Tool => {
            Some(event.item_id)
        }
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::TodoListUpdated { .. }
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
        AgentEvent::TracePartCompleted { item } if item.kind() == TracePartKind::Thinking => {
            Some(item.item_id().to_string())
        }
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::TodoListUpdated { .. }
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
            if matches!(
                item.text(),
                Some(text) if text.channel() == TraceTextChannel::Final
                    && matches!(text.state(), TraceTextState::Completed(_))
            ) =>
        {
            Some(item)
        }
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::TodoListUpdated { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. }
        | AgentEvent::TracePartCompleted { .. } => None,
    }
}

fn completed_thinking_item(event: AgentEvent) -> Option<TracePart> {
    match event {
        AgentEvent::TracePartCompleted { item } if item.kind() == TracePartKind::Thinking => {
            Some(item)
        }
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::TodoListUpdated { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. }
        | AgentEvent::TracePartCompleted { .. } => None,
    }
}

fn trace_part_text(item: &TracePart) -> String {
    item.thinking()
        .expect("thinking part")
        .summary()
        .iter()
        .map(|chunk| chunk.content.as_str())
        .collect::<Vec<_>>()
        .join("")
}

fn started_tool_item(event: AgentEvent) -> Option<TracePart> {
    match event {
        AgentEvent::TracePartStarted { item } if item.kind() == TracePartKind::Tool => Some(item),
        AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::TodoListUpdated { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. }
        | AgentEvent::TracePartStarted { .. } => None,
    }
}
