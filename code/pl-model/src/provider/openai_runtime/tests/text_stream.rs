use super::*;
use pretty_assertions::assert_eq;

#[test]
fn stream_accumulator_returns_content_and_reasoning_content() {
    let (event_tx, mut event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
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
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content.as_deref(), Some("9.11 更大。"));
    assert_eq!(response.raw_content.as_deref(), Some("9.11 更大。"));
    assert_eq!(response.reasoning_content, None);
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.kind == TracePartKind::Thinking
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
            StreamEvent::ResponseStarted {
                response_id: Some("resp_started".to_string()),
            },
            &event_tx,
        )
        .unwrap();
    accumulator
        .apply(
            StreamEvent::Completed {
                response_id: Some("resp_completed".to_string()),
            },
            &event_tx,
        )
        .unwrap();

    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.response_id.as_deref(), Some("resp_completed"));
}

#[test]
fn stream_accumulator_streams_commentary_without_content() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
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
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content.as_deref(), Some("完成。"));
    assert_eq!(response.raw_content.as_deref(), Some("完成。"));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Commentary)
                && item.content == "检查配置。"
    )));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Final)
        && item.content == "完成。"
    )));
}

#[test]
fn stream_accumulator_keeps_tagged_raw_reasoning_hidden() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: true,
        trace_sequence_base: 0,
    }));
    let mut decoder = tagged_decoder();

    for delta in [
        "<commentary>正在分析日志。</commentary>",
        "<final>完成。</final>",
    ] {
        apply_tagged(
            &mut decoder,
            &mut accumulator,
            StreamEvent::ReasoningRawDelta {
                id: "thinking".to_string(),
                content_index: 0,
                delta: delta.to_string(),
            },
            &event_tx,
        );
    }

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content, None);
    assert_eq!(
        response.reasoning_content.as_deref(),
        Some("<commentary>正在分析日志。</commentary><final>完成。</final>")
    );
    assert!(!response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.kind == TracePartKind::Text
                || item.kind == TracePartKind::Plan
                || item.kind == TracePartKind::Thinking
    )));
}

#[test]
fn stream_accumulator_splits_repeated_tagged_commentary_and_final_blocks() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
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
    let response = accumulator.finish(&event_tx).unwrap();
    let completed_text = response
        .trace_events
        .iter()
        .filter_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Text => {
                Some((
                    item.item_id.as_str(),
                    item.text_channel,
                    item.content.as_str(),
                ))
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
        .collect::<Vec<_>>();

    assert_eq!(response.content.as_deref(), Some("BD"));
    assert_eq!(response.raw_content.as_deref(), Some("BD"));
    assert_eq!(
        completed_text,
        vec![
            (
                "inf-1-text-commentary-1",
                Some(pl_trace::TraceTextChannel::Commentary),
                "A",
            ),
            (
                "inf-1-text-final-1",
                Some(pl_trace::TraceTextChannel::Final),
                "B",
            ),
            (
                "inf-1-text-commentary-2",
                Some(pl_trace::TraceTextChannel::Commentary),
                "C",
            ),
            (
                "inf-1-text-final-2",
                Some(pl_trace::TraceTextChannel::Final),
                "D",
            ),
        ]
    );
}

#[test]
fn stream_accumulator_keeps_untagged_reasoning_hidden() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: true,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(
            StreamEvent::ReasoningRawDelta {
                id: "thinking".to_string(),
                content_index: 0,
                delta: "先比较整数位。".to_string(),
            },
            &event_tx,
        )
        .unwrap();

    apply_completed(&mut accumulator, &event_tx);
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content, None);
    assert_eq!(
        response.reasoning_content.as_deref(),
        Some("先比较整数位。")
    );
    assert!(!response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.kind == TracePartKind::Text
                || item.kind == TracePartKind::Plan
                || item.kind == TracePartKind::Thinking
    )));
}

#[test]
fn stream_accumulator_treats_untagged_display_text_as_final() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
    }));

    accumulator
        .apply(final_started("final"), &event_tx)
        .unwrap();
    accumulator
        .apply(final_delta("final", "plain text"), &event_tx)
        .unwrap();
    apply_completed(&mut accumulator, &event_tx);

    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content.as_deref(), Some("plain text"));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Final)
                && item.content == "plain text"
    )));
}

#[test]
fn stream_accumulator_uses_authoritative_completed_text_for_response_content() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
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

    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(response.content.as_deref(), Some("final text"));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Final)
                && item.content == "final text"
    )));
}

#[test]
fn stream_accumulator_creates_part_for_authoritative_completion_without_delta() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(8);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: false,
        trace_sequence_base: 0,
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

    let response = accumulator.finish(&event_tx).unwrap();

    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartStarted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Commentary)
                && item.content.is_empty()
    )));
    assert!(response.trace_events.iter().any(|event| matches!(
        &event.kind,
        TraceEventKind::TracePartCompleted { item }
            if item.text_channel == Some(pl_trace::TraceTextChannel::Commentary)
                && item.content == "已完成检查"
    )));
}

#[test]
fn stream_accumulator_does_not_extract_proposed_plan_item() {
    let (event_tx, _event_rx) = tokio::sync::broadcast::channel(16);
    let mut accumulator = StreamCompletionAccumulator::new(Some(CompletionTraceContext {
        session_id: "session-1".to_string(),
        turn_id: "turn-1".to_string(),
        inference_id: "inf-1".to_string(),
        plan_mode: true,
        trace_sequence_base: 0,
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
    let response = accumulator.finish(&event_tx).unwrap();

    assert_eq!(
        response.content.as_deref(),
        Some("\n<proposed_plan>\n- step\n</proposed_plan>\nOutro")
    );
    assert_eq!(
        response.raw_content.as_deref(),
        Some("\n<proposed_plan>\n- step\n</proposed_plan>\nOutro")
    );
    let completed_plan = response
        .trace_events
        .iter()
        .find_map(|event| match &event.kind {
            TraceEventKind::TracePartCompleted { item } if item.kind == TracePartKind::Plan => {
                Some(item)
            }
            TraceEventKind::TracePartStarted { .. }
            | TraceEventKind::TracePartDelta { .. }
            | TraceEventKind::TracePartCompleted { .. }
            | TraceEventKind::TracePartFailed { .. }
            | TraceEventKind::PlanLifecycleChanged { .. }
            | TraceEventKind::InteractionChanged { .. }
            | TraceEventKind::SkillActivated { .. }
            | TraceEventKind::EnabledToolsRecorded { .. } => None,
        });
    assert!(completed_plan.is_none());
}
