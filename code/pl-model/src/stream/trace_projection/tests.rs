use pl_trace::{AgentEvent, TracePart, TracePartKind};

use super::*;
use crate::{ToolCall, ToolCallPayload};

fn trace() -> TraceProjection {
    TraceProjection::new(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inference-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    })
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
        .map(|item| (item.item_id.clone(), trace_part_text(&item)))
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
fn generated_part_ids_are_scoped_to_inference() {
    let mut first = trace();
    let mut second = TraceProjection::new(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inference-2".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
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
fn trace_sequence_base_offsets_started_sequence() {
    let mut first = TraceProjection::new(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 10,
    });
    let mut second = TraceProjection::new(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 20,
    });

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

    assert_eq!(completed.content, "final text");
    assert_eq!(completed.revision, 1);
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
            AgentEvent::TracePartFailed { item, error } => Some((item.item_id, item.status, error)),
            AgentEvent::TracePartStarted { .. }
            | AgentEvent::TracePartDelta { .. }
            | AgentEvent::TracePartCompleted { .. }
            | AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::AgentStateChanged { .. }
            | AgentEvent::SubAgentActivity { .. }
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
        vec![
            (
                "inference-1-text-final-1".to_string(),
                TracePartStatus::Failed,
                "connection lost".to_string(),
            ),
            (
                "inference-1-reasoning-1".to_string(),
                TracePartStatus::Failed,
                "connection lost".to_string(),
            ),
        ]
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
        arguments: "{\"cmd\":\"ec".to_string(),
    };
    let _ = trace.append_tool_arguments_delta(&snapshot, "{\"cmd\":\"ec".to_string());
    let updated = trace
        .update_tool_trace(&ToolCall {
            id: "provider-tool-1".to_string(),
            call_id: Some("call-1".to_string()),
            name: "exec".to_string(),
            payload: ToolCallPayload::Function {
                arguments: serde_json::json!({"cmd": "echo hi"}),
            },
            invalid_arguments: None,
        })
        .into_iter()
        .find_map(started_tool_item)
        .expect("updated tool snapshot");

    assert_eq!(updated.item_id, "turn-1-provider-tool-1");
    assert_eq!(updated.status, TracePartStatus::Streaming);
    assert_eq!(updated.revision, 1);
    let tool = updated.tool.expect("tool metadata");
    assert_eq!(tool.arguments, "{\"cmd\":\"echo hi\"}");
}

#[test]
fn late_provider_tool_id_keeps_original_trace_part_id() {
    let mut trace = trace();
    let early = ToolCallAccumulatorSnapshot {
        id: "call-1".to_string(),
        trace_id: "call-1".to_string(),
        call_id: Some("call-1".to_string()),
        name: "exec".to_string(),
        arguments: "{\"cmd\":\"ec".to_string(),
    };
    let late = ToolCallAccumulatorSnapshot {
        id: "provider-tool-1".to_string(),
        trace_id: "call-1".to_string(),
        call_id: Some("call-1".to_string()),
        name: "exec".to_string(),
        arguments: "{\"cmd\":\"echo hi\"}".to_string(),
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
            call_id: Some("call-1".to_string()),
            name: "exec".to_string(),
            payload: ToolCallPayload::Function {
                arguments: serde_json::json!({"cmd": "echo hi"}),
            },
            invalid_arguments: None,
        })
        .into_iter()
        .find_map(started_tool_item)
        .expect("updated tool snapshot");

    assert_eq!(first_delta, "turn-1-call-1");
    assert_eq!(second_delta, "turn-1-call-1");
    assert_eq!(updated.item_id, "turn-1-call-1");
    assert_eq!(updated.revision, 2);
    let tool = updated.tool.expect("tool metadata");
    assert_eq!(tool.provider_item_id.as_deref(), Some("provider-tool-1"));
    assert_eq!(tool.call_id.as_deref(), Some("call-1"));
}

fn started_sequence(event: AgentEvent) -> Option<u64> {
    match event {
        AgentEvent::TracePartStarted { item } => Some(item.started_sequence),
        AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::AgentStateChanged { .. }
        | AgentEvent::SubAgentActivity { .. }
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
        AgentEvent::TracePartDelta { event } if event.kind == TracePartKind::Thinking => {
            Some(event.item_id)
        }
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::AgentStateChanged { .. }
        | AgentEvent::SubAgentActivity { .. }
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
        AgentEvent::TracePartDelta { event } if event.kind == TracePartKind::Tool => {
            Some(event.item_id)
        }
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::AgentStateChanged { .. }
        | AgentEvent::SubAgentActivity { .. }
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
        AgentEvent::TracePartCompleted { item } if item.kind == TracePartKind::Thinking => {
            Some(item.item_id)
        }
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::AgentStateChanged { .. }
        | AgentEvent::SubAgentActivity { .. }
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
            if item.kind == TracePartKind::Text
                && item.text_channel == Some(TraceTextChannel::Final) =>
        {
            Some(item)
        }
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::AgentStateChanged { .. }
        | AgentEvent::SubAgentActivity { .. }
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
        AgentEvent::TracePartCompleted { item } if item.kind == TracePartKind::Thinking => {
            Some(item)
        }
        AgentEvent::TracePartStarted { .. }
        | AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::AgentStateChanged { .. }
        | AgentEvent::SubAgentActivity { .. }
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
    item.thinking_chunks
        .iter()
        .map(|chunk| chunk.content.as_str())
        .collect::<Vec<_>>()
        .join("")
}

fn started_tool_item(event: AgentEvent) -> Option<TracePart> {
    match event {
        AgentEvent::TracePartStarted { item } if item.kind == TracePartKind::Tool => Some(item),
        AgentEvent::TracePartDelta { .. }
        | AgentEvent::TracePartCompleted { .. }
        | AgentEvent::TracePartFailed { .. }
        | AgentEvent::InteractionChanged { .. }
        | AgentEvent::AgentRuntimeUpdated { .. }
        | AgentEvent::AgentStateChanged { .. }
        | AgentEvent::SubAgentActivity { .. }
        | AgentEvent::TodoListUpdated { .. }
        | AgentEvent::TurnInterrupted { .. }
        | AgentEvent::TurnBudgetLimited { .. }
        | AgentEvent::SkillActivated { .. }
        | AgentEvent::Done
        | AgentEvent::Error { .. }
        | AgentEvent::TracePartStarted { .. } => None,
    }
}
