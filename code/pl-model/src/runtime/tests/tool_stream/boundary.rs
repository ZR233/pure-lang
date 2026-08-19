use super::super::*;
use pretty_assertions::assert_eq;

#[test]
fn stream_accumulator_merges_chat_tool_call_chunks_by_index() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(None);

    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: Some("chat_tool_call:0".to_string()),
                item_id: "call_1".to_string(),
                call_id: None,
                name: Some("read_file".to_string()),
                payload_delta: ToolCallDeltaPayload::FunctionArguments(String::new()),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: Some("chat_tool_call:0".to_string()),
                item_id: String::new(),
                call_id: None,
                name: None,
                payload_delta: ToolCallDeltaPayload::FunctionArguments(
                    "{\"path\":\"Cargo.toml\"}".to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = finish_with_trace(accumulator, &event_tx).unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "call_1");
    assert_eq!(response.tool_calls[0].name, "read_file");
    match &response.tool_calls[0].payload {
        ToolCallPayload::Function { arguments } => {
            assert_eq!(arguments, &serde_json::json!({"path": "Cargo.toml"}));
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}

#[test]
fn stream_accumulator_splits_reasoning_and_text_across_tool_boundary() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(summary_started("thinking"), &event_tx)
        .unwrap();
    accumulator
        .apply(summary_delta("thinking", 0, "before"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_started("msg_1"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_delta("msg_1", "prelude"), &event_tx)
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolInputStarted {
                stream_id: None,
                item_id: "call_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("exec".to_string()),
                payload_kind:
                    crate::completion::stream::event::ToolInputPayloadKind::FunctionArguments,
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "call_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("exec".to_string()),
                payload: Some(ToolCallDeltaPayload::FunctionArguments(
                    "{\"command\":\"pwd\"}".to_string(),
                )),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(summary_started("thinking#2"), &event_tx)
        .unwrap();
    accumulator
        .apply(summary_delta("thinking#2", 0, "after"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_started("msg_1#2"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_delta("msg_1#2", "done"), &event_tx)
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = finish_with_trace(accumulator, &event_tx).unwrap();
    let completed = response
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } => {
                Some((item.item_id.as_str(), item.kind, item.content.as_str()))
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<Vec<_>>();
    let tool_seen = response.trace_events.iter().any(|event| match &event.kind {
        TraceEventKind::TracePartStarted { item }
        | TraceEventKind::TracePartCompleted { item }
        | TraceEventKind::TracePartFailed { item, .. } => {
            item.item_id == "turn-1-call_1" && item.kind == TracePartKind::Tool
        }
        TraceEventKind::TracePartDelta { event } => {
            event.item_id == "turn-1-call_1" && event.kind == TracePartKind::Tool
        }
        TraceEventKind::PlanLifecycleChanged { .. }
        | TraceEventKind::InteractionChanged { .. }
        | TraceEventKind::SkillActivated { .. }
        | TraceEventKind::EnabledToolsRecorded { .. } => false,
    });

    assert!(completed.contains(&("turn-1-inf-0-reasoning-1", TracePartKind::Thinking, "")));
    assert!(completed.contains(&("turn-1-inf-0-text-final-1", TracePartKind::Text, "prelude")));
    assert!(tool_seen);
    assert!(completed.contains(&("turn-1-inf-0-reasoning-2", TracePartKind::Thinking, "")));
    assert!(completed.contains(&("turn-1-inf-0-text-final-2", TracePartKind::Text, "done")));
}

#[test]
fn tagged_stream_flushes_visible_text_before_tool_call() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(32);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));
    let mut decoder = tagged_decoder();

    apply_tagged(
        &mut decoder,
        &mut accumulator,
        final_delta("chat-final", "我先检查项目结构。"),
        &event_tx,
    );
    apply_tagged(
        &mut decoder,
        &mut accumulator,
        StreamEvent::ToolInputStarted {
            stream_id: Some("chat_tool_call:0".to_string()),
            item_id: "call_1".to_string(),
            call_id: None,
            name: Some("read_file".to_string()),
            payload_kind: crate::completion::stream::event::ToolInputPayloadKind::FunctionArguments,
        },
        &event_tx,
    );
    apply_tagged(
        &mut decoder,
        &mut accumulator,
        StreamEvent::ToolCallReady {
            stream_id: Some("chat_tool_call:0".to_string()),
            item_id: "call_1".to_string(),
            call_id: None,
            name: Some("read_file".to_string()),
            payload: Some(ToolCallDeltaPayload::FunctionArguments(
                r#"{"path":"Cargo.toml"}"#.to_string(),
            )),
        },
        &event_tx,
    );

    apply_completed(&mut accumulator, &event_tx);
    let response = finish_with_trace(accumulator, &event_tx).unwrap();
    let ordered_trace = response
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } => {
                Some((item.kind, item.item_id.as_str(), trace_part_text(item)))
            }
            TraceEventKind::TracePartStarted { item } => {
                Some((item.kind, item.item_id.as_str(), trace_part_text(item)))
            }
            TraceEventKind::TracePartDelta { event } => Some((
                event.kind,
                event.item_id.as_str(),
                trace_delta_text(&event.delta),
            )),
            TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(response.content.as_deref(), Some("我先检查项目结构。"));
    assert_eq!(response.tool_calls.len(), 1);
    assert!(ordered_trace.iter().any(|(kind, _, text)| {
        *kind == TracePartKind::Text && text == "我先检查项目结构。"
    }));
    let text_index = ordered_trace
        .iter()
        .position(|(kind, _, text)| *kind == TracePartKind::Text && text == "我先检查项目结构。")
        .expect("text part should complete before tool");
    let tool_index = ordered_trace
        .iter()
        .position(|(kind, _, _)| *kind == TracePartKind::Tool)
        .expect("tool part should start");
    assert!(text_index < tool_index);
}

#[test]
fn stream_accumulator_terminal_snapshots_converge_with_live_deltas() {
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(32);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(summary_started("thinking"), &event_tx)
        .unwrap();
    accumulator
        .apply(summary_delta("thinking", 0, "think"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_started("msg_1"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_delta("msg_1", "hello"), &event_tx)
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("exec".to_string()),
                payload_delta: ToolCallDeltaPayload::FunctionArguments(
                    "{\"command\":\"pwd\"}".to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("exec".to_string()),
                payload: Some(ToolCallDeltaPayload::FunctionArguments(
                    "{\"command\":\"pwd\"}".to_string(),
                )),
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = finish_with_trace(accumulator, &event_tx).unwrap();
    let live_events = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();

    let started = live_events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::TracePartStarted { item } => {
                Some((item.item_id.as_str(), item.kind, item.content.as_str()))
            }
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
        })
        .collect::<Vec<_>>();
    assert!(started.contains(&("turn-1-inf-0-reasoning-1", TracePartKind::Thinking, "")));
    assert!(started.contains(&("turn-1-inf-0-text-final-1", TracePartKind::Text, "")));
    assert!(started.contains(&("turn-1-fc_1", TracePartKind::Tool, "")));

    let mut live = std::collections::HashMap::new();
    for event in &live_events {
        match event {
            AgentEvent::TracePartStarted { item } | AgentEvent::TracePartCompleted { item } => {
                live.insert(item.item_id.clone(), trace_part_text(item));
            }
            AgentEvent::TracePartDelta { event } => {
                live.entry(event.item_id.clone())
                    .or_insert_with(String::new)
                    .push_str(&trace_delta_text(&event.delta));
            }
            AgentEvent::TracePartFailed { item, .. } => {
                live.insert(item.item_id.clone(), trace_part_text(item));
            }
            AgentEvent::InteractionChanged { .. }
            | AgentEvent::AgentRuntimeUpdated { .. }
            | AgentEvent::AgentStateChanged { .. }
            | AgentEvent::SubAgentActivity { .. }
            | AgentEvent::TodoListUpdated { .. }
            | AgentEvent::TurnInterrupted { .. }
            | AgentEvent::TurnBudgetLimited { .. }
            | AgentEvent::SkillActivated { .. }
            | AgentEvent::Done
            | AgentEvent::Error { .. } => {}
        }
    }
    let replay = response
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item }
            | TraceEventKind::TracePartStarted { item }
                if matches!(
                    item.kind,
                    TracePartKind::Text | TracePartKind::Thinking | TracePartKind::Plan
                ) && item.status == pl_trace::TracePartStatus::Completed =>
            {
                Some((item.item_id.clone(), trace_part_text(item)))
            }
            TraceEventKind::TracePartStarted { item }
                if item.kind == TracePartKind::Tool
                    && item.item_id == "turn-1-fc_1"
                    && item
                        .tool
                        .as_ref()
                        .is_some_and(|tool| !tool.arguments.is_empty()) =>
            {
                Some((item.item_id.clone(), trace_part_text(item)))
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<std::collections::HashMap<_, _>>();

    assert_eq!(
        live.get("turn-1-inf-0-reasoning-1"),
        replay.get("turn-1-inf-0-reasoning-1")
    );
    assert_eq!(
        live.get("turn-1-inf-0-text-final-1"),
        replay.get("turn-1-inf-0-text-final-1")
    );
    assert_eq!(live.get("turn-1-fc_1"), replay.get("turn-1-fc_1"));
}
