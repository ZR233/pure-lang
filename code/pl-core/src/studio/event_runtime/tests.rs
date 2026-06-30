use pl_protocol::{StudioPartStatus, StudioPartType, StudioTextChannel};
use pl_trace::{
    AgentEvent, TraceDelta, TracePart, TracePartDeltaEvent, TracePartKind, TracePartSource,
    TracePartStatus, TraceTextChannel, TraceToolPart,
};
use pretty_assertions::assert_eq;

use super::*;
use crate::CompileMode;

#[tokio::test]
async fn assistant_message_lifecycle_follows_turn_not_part_status() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Visible progress", CompileMode::Auto)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store.clone());

    let commentary = TracePart::text(
        "turn-1",
        "commentary-1",
        10,
        TraceTextChannel::Commentary,
        "working",
        TracePartStatus::Completed,
        100,
    );
    runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartCompleted { item: commentary },
        )
        .await
        .unwrap();

    let messages = store.load_studio_messages(&session.id).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message.status, StudioMessageStatus::Streaming);
    assert_eq!(messages[0].message.created_at, 100);
    assert_eq!(messages[0].message.completed_at, None);

    let final_answer = TracePart::text(
        "turn-1",
        "final-1",
        11,
        TraceTextChannel::Final,
        "done",
        TracePartStatus::Started,
        200,
    );
    runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartStarted { item: final_answer },
        )
        .await
        .unwrap();

    let messages = store.load_studio_messages(&session.id).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message.status, StudioMessageStatus::Streaming);
    assert_eq!(messages[0].message.created_at, 100);
    assert_eq!(messages[0].message.completed_at, None);

    runtime
        .emit_turn(&session.id, "turn-1", StudioTurnStatus::Completed, None)
        .await
        .unwrap();

    let messages = store.load_studio_messages(&session.id).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message.status, StudioMessageStatus::Completed);
    assert_eq!(messages[0].message.created_at, 100);
    assert!(messages[0].message.completed_at.is_some());
}

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

#[tokio::test]
async fn trace_part_delta_requires_existing_part() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Delta guard", CompileMode::Auto)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store);
    let mut rx = runtime.subscribe_session(session.id.clone());

    let result = runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartDelta {
                event: text_delta_event("turn-delta", "missing-part", 1, "hello"),
            },
        )
        .await
        .unwrap();

    assert!(result.is_none());
    let event = rx.recv().await.unwrap();
    assert!(matches!(
        event.kind,
        StudioEventKind::Stale { lagged_events: 1 }
    ));
}

#[tokio::test]
async fn trace_part_delta_requires_contiguous_revision() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Delta revision", CompileMode::Auto)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store);
    runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartStarted {
                item: streaming_text_part("turn-delta", "part-delta"),
            },
        )
        .await
        .unwrap();

    let first = runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartDelta {
                event: text_delta_event("turn-delta", "part-delta", 1, "hel"),
            },
        )
        .await
        .unwrap()
        .unwrap();
    let StudioEventKind::MessagePartDelta { delta } = first.kind else {
        panic!("expected messagePartDelta");
    };
    assert_eq!(delta.revision, 1);
    assert_eq!(delta.delta, "hel");

    let mut rx = runtime.subscribe_session(session.id.clone());
    let duplicate = runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartDelta {
                event: text_delta_event("turn-delta", "part-delta", 1, "lo"),
            },
        )
        .await
        .unwrap();

    assert!(duplicate.is_none());
    let event = rx.recv().await.unwrap();
    assert!(matches!(
        event.kind,
        StudioEventKind::Stale { lagged_events: 1 }
    ));
}

#[tokio::test]
async fn trace_part_delta_rejects_mismatched_message_or_field() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Delta identity guard", CompileMode::Auto)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store.clone());
    runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartStarted {
                item: streaming_text_part("turn-delta-identity", "part-delta"),
            },
        )
        .await
        .unwrap();

    let mut rx = runtime.subscribe_session(session.id.clone());
    let mismatched = runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartDelta {
                event: commentary_delta_event(
                    "turn-delta-identity",
                    "part-delta",
                    1,
                    "wrong channel",
                ),
            },
        )
        .await
        .unwrap();

    assert!(mismatched.is_none());
    let event = rx.recv().await.unwrap();
    assert!(matches!(
        event.kind,
        StudioEventKind::Stale { lagged_events: 1 }
    ));
    let part = store
        .read_message_part("turn-delta-identity:part-0")
        .await
        .unwrap()
        .unwrap()
        .part;
    assert_eq!(part.revision, 0);
    assert_eq!(part.text_channel, Some(StudioTextChannel::Final));
}

#[tokio::test]
async fn trace_part_delta_routes_by_session_turn_scoped_identity() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Scoped delta", CompileMode::Auto)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store);
    for turn_id in ["turn-a", "turn-b"] {
        runtime
            .emit_agent_event(
                &session.id,
                AgentEvent::TracePartStarted {
                    item: streaming_text_part(turn_id, "provider-text"),
                },
            )
            .await
            .unwrap();
    }

    let first = runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartDelta {
                event: text_delta_event("turn-a", "provider-text", 1, "a"),
            },
        )
        .await
        .unwrap()
        .unwrap();
    let second = runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartDelta {
                event: text_delta_event("turn-b", "provider-text", 1, "b"),
            },
        )
        .await
        .unwrap()
        .unwrap();

    let StudioEventKind::MessagePartDelta { delta: first } = first.kind else {
        panic!("expected first delta");
    };
    let StudioEventKind::MessagePartDelta { delta: second } = second.kind else {
        panic!("expected second delta");
    };
    assert_eq!(first.part_id, "turn-a:part-0");
    assert_eq!(second.part_id, "turn-b:part-0");
    assert_ne!(first.part_id, second.part_id);
    assert_eq!(first.delta, "a");
    assert_eq!(second.delta, "b");
}

#[tokio::test]
async fn trace_part_delta_after_terminal_snapshot_emits_stale() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Terminal delta", CompileMode::Auto)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store);
    runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartCompleted {
                item: TracePart::text(
                    "turn-terminal",
                    "part-terminal",
                    0,
                    TraceTextChannel::Final,
                    "done",
                    TracePartStatus::Completed,
                    100,
                ),
            },
        )
        .await
        .unwrap();
    let mut rx = runtime.subscribe_session(session.id.clone());

    let result = runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartDelta {
                event: text_delta_event("turn-terminal", "part-terminal", 1, "late"),
            },
        )
        .await
        .unwrap();

    assert!(result.is_none());
    let event = rx.recv().await.unwrap();
    assert!(matches!(
        event.kind,
        StudioEventKind::Stale { lagged_events: 1 }
    ));
}

#[tokio::test]
async fn emit_live_rejects_durable_event_kinds() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Live guard", CompileMode::Auto)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store);
    let mut rx = runtime.subscribe();

    let error = runtime
        .emit_live(
            None,
            Some(session.id.clone()),
            Some("turn-live-guard".to_string()),
            StudioEventKind::TurnChanged {
                turn: StudioTurn {
                    turn_id: "turn-live-guard".to_string(),
                    session_id: session.id.clone(),
                    status: StudioTurnStatus::Completed,
                    reason: None,
                    updated_at: 100,
                },
            },
        )
        .await
        .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("emit_live only accepts live-only studio events")
    );
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn terminal_snapshot_carries_latest_live_revision() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Terminal revision", CompileMode::Auto)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store.clone());
    runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartStarted {
                item: streaming_text_part("turn-terminal-revision", "part-terminal-revision"),
            },
        )
        .await
        .unwrap();
    runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartDelta {
                event: text_delta_event(
                    "turn-terminal-revision",
                    "part-terminal-revision",
                    1,
                    "done",
                ),
            },
        )
        .await
        .unwrap();
    let mut completed = TracePart::text(
        "turn-terminal-revision",
        "part-terminal-revision",
        0,
        TraceTextChannel::Final,
        "done",
        TracePartStatus::Completed,
        100,
    );
    completed.updated_at = 101;

    runtime
        .emit_agent_event(
            &session.id,
            AgentEvent::TracePartCompleted { item: completed },
        )
        .await
        .unwrap();

    let part = store
        .read_message_part("turn-terminal-revision:part-0")
        .await
        .unwrap()
        .unwrap()
        .part;
    assert_eq!(part.status, StudioPartStatus::Completed);
    assert_eq!(part.revision, 1);
}

fn streaming_text_part(turn_id: &str, item_id: &str) -> TracePart {
    TracePart::text(
        turn_id,
        item_id,
        0,
        TraceTextChannel::Final,
        "",
        TracePartStatus::Streaming,
        100,
    )
}

fn thinking_part(turn_id: &str, item_id: &str, sequence: u64, text: &str) -> TracePart {
    TracePart {
        turn_id: turn_id.to_string(),
        item_id: item_id.to_string(),
        started_sequence: sequence,
        revision: 0,
        kind: TracePartKind::Thinking,
        status: TracePartStatus::Completed,
        created_at: 100,
        updated_at: 100,
        source: TracePartSource::Model,
        text_channel: None,
        content: String::new(),
        attachments: Vec::new(),
        thinking_chunks: vec![pl_trace::TraceThinkingChunk {
            chunk_index: 0,
            content: text.to_string(),
        }],
        tool: None,
        agent: None,
        inference: None,
        usage: None,
    }
}

fn tool_part(turn_id: &str, item_id: &str, sequence: u64) -> TracePart {
    TracePart {
        turn_id: turn_id.to_string(),
        item_id: item_id.to_string(),
        started_sequence: sequence,
        revision: 0,
        kind: TracePartKind::Tool,
        status: TracePartStatus::Completed,
        created_at: 100,
        updated_at: 100,
        source: TracePartSource::Model,
        text_channel: None,
        content: String::new(),
        attachments: Vec::new(),
        thinking_chunks: Vec::new(),
        tool: Some(TraceToolPart {
            tool_call_id: item_id.to_string(),
            call_id: Some(format!("{item_id}-call")),
            provider_item_id: Some(item_id.to_string()),
            name: "bash".to_string(),
            arguments: "{}".to_string(),
            result: Some("ok".to_string()),
            exit_code: Some(0),
            timed_out: false,
            working_directory: None,
            denial_reason: None,
        }),
        agent: None,
        inference: None,
        usage: None,
    }
}

fn text_delta_event(
    turn_id: &str,
    item_id: &str,
    revision: u64,
    delta: &str,
) -> TracePartDeltaEvent {
    TracePartDeltaEvent {
        turn_id: turn_id.to_string(),
        item_id: item_id.to_string(),
        started_sequence: 0,
        revision,
        kind: TracePartKind::Text,
        status: TracePartStatus::Streaming,
        created_at: 100,
        updated_at: 100,
        delta: TraceDelta::Text {
            text_channel: TraceTextChannel::Final,
            delta: delta.to_string(),
        },
    }
}

fn commentary_delta_event(
    turn_id: &str,
    item_id: &str,
    revision: u64,
    delta: &str,
) -> TracePartDeltaEvent {
    TracePartDeltaEvent {
        turn_id: turn_id.to_string(),
        item_id: item_id.to_string(),
        started_sequence: 0,
        revision,
        kind: TracePartKind::Text,
        status: TracePartStatus::Streaming,
        created_at: 100,
        updated_at: 100,
        delta: TraceDelta::Text {
            text_channel: TraceTextChannel::Commentary,
            delta: delta.to_string(),
        },
    }
}
