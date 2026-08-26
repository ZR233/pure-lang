use super::super::*;
use pretty_assertions::assert_eq;

#[test]
fn stream_trace_part_ids_are_scoped_to_turn() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
    }));

    accumulator
        .apply(
            ModelStreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "call_0".to_string(),
                call_id: Some("call_0".to_string()),
                name: Some("exec".to_string()),
                payload_delta: ToolInputDeltaPayload::FunctionArguments(
                    r#"{"command":"pwd"}"#.to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            ModelStreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "call_0".to_string(),
                call_id: Some("call_0".to_string()),
                name: Some("exec".to_string()),
                payload: Some(ToolInputDeltaPayload::FunctionArguments(
                    "{\"command\":\"pwd\"}".to_string(),
                )),
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = finish_with_trace(accumulator, &event_tx).unwrap();

    assert_eq!(response.tool_calls[0].id, "call_0");
    let item_ids = response
        .trace_events
        .iter()
        .map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item } => item.item_id(),
            TraceEventKind::TracePartDelta { event } => event.item_id.as_str(),
            TraceEventKind::TracePartFailed { item } => item.item_id(),
            TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => "",
        })
        .filter(|item_id| !item_id.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(
        item_ids,
        vec!["turn-1-call_0", "turn-1-call_0", "turn-1-call_0"]
    );
}

#[test]
fn stream_accumulator_merges_tool_call_with_late_call_id() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
    }));

    accumulator
        .apply(
            ModelStreamEvent::ToolInputStarted {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: None,
                name: Some("read_file".to_string()),
                payload_kind:
                    crate::completion::stream::event::ToolInputPayloadKind::FunctionArguments,
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            ModelStreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: None,
                payload_delta: ToolInputDeltaPayload::FunctionArguments(
                    r#"{"path":"Cargo.toml"}"#.to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            ModelStreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: None,
                payload: None,
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = finish_with_trace(accumulator, &event_tx).unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "fc_1");
    assert_eq!(response.tool_calls[0].call_id, "call_1");
    assert_eq!(response.tool_calls[0].name, "read_file");
    let item_ids = response
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
                if item.kind() == TracePartKind::Tool =>
            {
                Some(item.item_id())
            }
            TraceEventKind::TracePartDelta { event } if event.kind() == TracePartKind::Tool => {
                Some(event.item_id.as_str())
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(item_ids, vec!["turn-1-fc_1", "turn-1-fc_1", "turn-1-fc_1"]);
}

#[test]
fn stream_accumulator_keeps_tool_trace_id_when_item_id_arrives_late() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
    }));

    accumulator
        .apply(
            ModelStreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: String::new(),
                call_id: Some("call_1".to_string()),
                name: Some("read_file".to_string()),
                payload_delta: ToolInputDeltaPayload::FunctionArguments(
                    r#"{"path":"Car"#.to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            ModelStreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: None,
                payload_delta: ToolInputDeltaPayload::FunctionArguments(r#"go.toml"}"#.to_string()),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            ModelStreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "fc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: None,
                payload: None,
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = finish_with_trace(accumulator, &event_tx).unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "fc_1");
    assert_eq!(response.tool_calls[0].call_id, "call_1");
    assert_eq!(response.tool_calls[0].name, "read_file");
    let item_ids = response
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartStarted { item }
            | TraceEventKind::TracePartCompleted { item }
                if item.kind() == TracePartKind::Tool =>
            {
                Some(item.item_id())
            }
            TraceEventKind::TracePartDelta { event } if event.kind() == TracePartKind::Tool => {
                Some(event.item_id.as_str())
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        item_ids
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(["turn-1-call_1"])
    );
    assert_eq!(
        item_ids,
        vec![
            "turn-1-call_1",
            "turn-1-call_1",
            "turn-1-call_1",
            "turn-1-call_1"
        ]
    );
}

#[test]
fn stream_trace_scope_rejects_similar_turn_prefix() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "turn-1-inf-0".to_string(),
    }));

    accumulator
        .apply(
            ModelStreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "turn-10-call".to_string(),
                call_id: None,
                name: Some("exec".to_string()),
                payload_delta: ToolInputDeltaPayload::FunctionArguments(
                    r#"{"command":"pwd"}"#.to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            ModelStreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "turn-10-call".to_string(),
                call_id: None,
                name: None,
                payload: None,
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = finish_with_trace(accumulator, &event_tx).unwrap();

    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartStarted { item }
            if item.kind() == TracePartKind::Tool
                && item.item_id() == "turn-1-turn-10-call"
    )));
}

#[test]
fn stream_accumulator_uses_responses_added_item_name_when_done_omits_name() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(None);

    accumulator
        .apply(
            ModelStreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "ctc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: Some("apply_patch".to_string()),
                payload_delta: ToolInputDeltaPayload::CustomInput(String::new()),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            ModelStreamEvent::ToolInputDelta {
                stream_id: None,
                item_id: "ctc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: None,
                payload_delta: ToolInputDeltaPayload::CustomInput(
                    "*** Begin Patch\n*** End Patch".to_string(),
                ),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            ModelStreamEvent::ToolCallReady {
                stream_id: None,
                item_id: "ctc_1".to_string(),
                call_id: Some("call_1".to_string()),
                name: None,
                payload: Some(ToolInputDeltaPayload::CustomInput(
                    "*** Begin Patch\n*** End Patch".to_string(),
                )),
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = finish_with_trace(accumulator, &event_tx).unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].id, "ctc_1");
    assert_eq!(response.tool_calls[0].name, "apply_patch");
    match &response.tool_calls[0].payload {
        ToolCallPayload::Custom { input } => {
            assert_eq!(input, "*** Begin Patch\n*** End Patch");
        }
        other => panic!("unexpected payload: {other:?}"),
    }
}
