use super::*;
use pretty_assertions::assert_eq;

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
