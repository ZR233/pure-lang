use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn trace_part_delta_requires_existing_part() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/studio").await.unwrap();
    let session = store
        .create_session(&project.id, "Delta guard", StudioMode::Simple)
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
        .create_session(&project.id, "Delta revision", StudioMode::Simple)
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
        .create_session(&project.id, "Delta identity guard", StudioMode::Simple)
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
        .create_session(&project.id, "Scoped delta", StudioMode::Simple)
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
        .create_session(&project.id, "Terminal delta", StudioMode::Simple)
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
