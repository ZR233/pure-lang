use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn trace_part_order_is_allocated_by_runtime_not_trace_sequence() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Part order", CompileMode::Auto)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store.clone());

    let first = TracePart::text(
        "turn-order",
        "first-final",
        999,
        TraceTextChannel::Final,
        "first",
        TracePartStatus::Completed,
        100,
    );
    runtime
        .emit_agent_event(&session.id, AgentEvent::TracePartCompleted { item: first })
        .await
        .unwrap();

    let second = TracePart::text(
        "turn-order",
        "second-final",
        10,
        TraceTextChannel::Final,
        "second",
        TracePartStatus::Completed,
        101,
    );
    runtime
        .emit_agent_event(&session.id, AgentEvent::TracePartCompleted { item: second })
        .await
        .unwrap();

    let parts = store.load_message_parts(&session.id).await.unwrap();
    let compact = parts
        .into_iter()
        .map(|record| (record.part.part_id, record.part.order, record.part.text))
        .collect::<Vec<_>>();

    assert_eq!(
        compact,
        vec![
            ("turn-order:part-0".to_string(), 0, "first".to_string()),
            ("turn-order:part-1".to_string(), 1, "second".to_string()),
        ]
    );
}

#[tokio::test]
async fn trace_part_identity_is_allocated_by_runtime_actor() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Part identity", CompileMode::Auto)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store.clone());

    let inputs = [
        ("reasoning-0", TracePartKind::Thinking, 0, "r0"),
        ("commentary-0", TracePartKind::Text, 2, "c0"),
        ("tool-0", TracePartKind::Tool, 4, ""),
        ("reasoning-1", TracePartKind::Thinking, 0, "r1"),
        ("commentary-1", TracePartKind::Text, 2, "c1"),
        ("tool-1", TracePartKind::Tool, 4, ""),
        ("reasoning-2", TracePartKind::Thinking, 0, "r2"),
        ("final-2", TracePartKind::Text, 2, "final"),
    ];

    for (trace_id, kind, sequence, text) in inputs {
        let item = match kind {
            TracePartKind::Text => TracePart::text(
                "turn-identity",
                trace_id,
                sequence,
                if trace_id.starts_with("commentary") {
                    TraceTextChannel::Commentary
                } else {
                    TraceTextChannel::Final
                },
                text,
                TracePartStatus::Completed,
                100 + sequence as i64,
            ),
            TracePartKind::Thinking => thinking_part("turn-identity", trace_id, sequence, text),
            TracePartKind::Tool => tool_part("turn-identity", trace_id, sequence),
            TracePartKind::Agent
            | TracePartKind::Turn
            | TracePartKind::Inference
            | TracePartKind::Plan => {
                unreachable!("test only creates visible assistant parts")
            }
        };
        runtime
            .emit_agent_event(&session.id, AgentEvent::TracePartCompleted { item })
            .await
            .unwrap();
    }

    let parts = store.load_message_parts(&session.id).await.unwrap();
    let compact = parts
        .iter()
        .map(|record| {
            (
                record.part.part_id.clone(),
                record.part.order,
                record.part.part_type,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        compact,
        vec![
            (
                "turn-identity:part-0".to_string(),
                0,
                StudioPartType::Reasoning,
            ),
            ("turn-identity:part-1".to_string(), 1, StudioPartType::Text,),
            ("turn-identity:part-2".to_string(), 2, StudioPartType::Tool,),
            (
                "turn-identity:part-3".to_string(),
                3,
                StudioPartType::Reasoning,
            ),
            ("turn-identity:part-4".to_string(), 4, StudioPartType::Text,),
            ("turn-identity:part-5".to_string(), 5, StudioPartType::Tool,),
            (
                "turn-identity:part-6".to_string(),
                6,
                StudioPartType::Reasoning,
            ),
            ("turn-identity:part-7".to_string(), 7, StudioPartType::Text,),
        ]
    );
}

#[tokio::test]
async fn trace_part_order_spans_inference_ids_and_tool_boundary() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Inference boundary", CompileMode::Auto)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store.clone());
    let turn_id = "turn-inference-boundary";

    let items = [
        thinking_part(turn_id, "inf-a-reasoning-1", 20, "thinking a"),
        TracePart::text(
            turn_id,
            "inf-a-text-commentary-1",
            0,
            TraceTextChannel::Commentary,
            "commentary a",
            TracePartStatus::Completed,
            101,
        ),
        tool_part(turn_id, "inf-a-tool-1", 1),
        TracePart::text(
            turn_id,
            "inf-b-text-final-1",
            0,
            TraceTextChannel::Final,
            "final b",
            TracePartStatus::Completed,
            103,
        ),
    ];

    for item in items {
        runtime
            .emit_agent_event(&session.id, AgentEvent::TracePartCompleted { item })
            .await
            .unwrap();
    }

    let parts = store.load_message_parts(&session.id).await.unwrap();
    let compact = parts
        .iter()
        .map(|record| {
            (
                record.part.part_id.clone(),
                record.part.order,
                record.part.part_type,
                record.part.text_channel,
                record.part.text.clone(),
                record.part.tool.as_ref().map(|tool| tool.name.clone()),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        compact,
        vec![
            (
                "turn-inference-boundary:part-0".to_string(),
                0,
                StudioPartType::Reasoning,
                None,
                "thinking a".to_string(),
                None,
            ),
            (
                "turn-inference-boundary:part-1".to_string(),
                1,
                StudioPartType::Text,
                Some(StudioTextChannel::Commentary),
                "commentary a".to_string(),
                None,
            ),
            (
                "turn-inference-boundary:part-2".to_string(),
                2,
                StudioPartType::Tool,
                None,
                String::new(),
                Some("bash".to_string()),
            ),
            (
                "turn-inference-boundary:part-3".to_string(),
                3,
                StudioPartType::Text,
                Some(StudioTextChannel::Final),
                "final b".to_string(),
                None,
            ),
        ]
    );
}

#[tokio::test]
async fn runtime_commentary_is_projected_as_synthetic() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Runtime commentary", CompileMode::Auto)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store.clone());

    let runtime_commentary =
        TracePart::runtime_commentary("turn-1", "progress-1", 1, "正在准备上下文。", 100);
    runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartCompleted {
                item: runtime_commentary,
            },
        )
        .await
        .unwrap();

    let model_commentary = TracePart::text(
        "turn-1",
        "commentary-1",
        2,
        TraceTextChannel::Commentary,
        "模型进展",
        TracePartStatus::Completed,
        101,
    );
    runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartCompleted {
                item: model_commentary,
            },
        )
        .await
        .unwrap();

    let parts = store.load_message_parts(&session.id).await.unwrap();
    let compact = parts
        .into_iter()
        .map(|record| {
            (
                record.part.part_id,
                record.part.text_channel,
                record.part.synthetic,
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        compact,
        vec![
            (
                "turn-1:part-0".to_string(),
                Some(StudioTextChannel::Commentary),
                true,
            ),
            (
                "turn-1:part-1".to_string(),
                Some(StudioTextChannel::Commentary),
                false,
            ),
        ]
    );
}
