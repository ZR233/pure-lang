use super::*;
use pretty_assertions::assert_eq;

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
