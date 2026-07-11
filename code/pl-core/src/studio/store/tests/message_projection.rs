use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn invalid_message_projection_updates_are_rejected() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/gamma").await.unwrap();
    let session = store
        .create_session(&project.id, "Messages", CompileMode::Simple)
        .await
        .unwrap();
    let message = StudioMessage {
        message_id: "message-1".to_string(),
        session_id: session.id.clone(),
        turn_id: "turn-1".to_string(),
        role: StudioMessageRole::Assistant,
        status: StudioMessageStatus::Streaming,
        created_at: 10,
        updated_at: 10,
        completed_at: None,
        error: None,
        metadata: serde_json::json!({}),
    };
    let event = |event_id: &str, sequence: u64, message: StudioMessage| StudioEventEnvelope {
        event_id: event_id.to_string(),
        project_id: Some(project.id.clone()),
        session_id: Some(session.id.clone()),
        turn_id: Some("turn-1".to_string()),
        sequence,
        created_at: sequence as i64,
        kind: StudioEventKind::MessageUpdated {
            message: Box::new(message),
        },
    };

    store
        .append_studio_event(event("studio-message-1", 1, message.clone()))
        .await
        .unwrap();

    let mut changed_turn = message.clone();
    changed_turn.turn_id = "turn-2".to_string();
    changed_turn.updated_at = 12;
    let err = store
        .append_studio_event(event("studio-message-turn-change", 2, changed_turn))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("message turnId cannot change"));
    assert_eq!(
        store
            .load_studio_events(&session.id, None, None)
            .await
            .unwrap()
            .len(),
        1
    );
    let stored = store
        .read_studio_message("message-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.message.turn_id, "turn-1");
    assert_eq!(stored.message.updated_at, 10);

    let mut completed = message.clone();
    completed.status = StudioMessageStatus::Completed;
    completed.updated_at = 13;
    completed.completed_at = Some(13);
    store
        .append_studio_event(event("studio-message-completed", 3, completed.clone()))
        .await
        .unwrap();

    let mut changed_terminal = completed;
    changed_terminal.updated_at = 14;
    let err = store
        .append_studio_event(event("studio-message-terminal-change", 4, changed_terminal))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("terminal message cannot change"));
    assert_eq!(
        store
            .load_studio_events(&session.id, None, None)
            .await
            .unwrap()
            .len(),
        2
    );
    let stored = store
        .read_studio_message("message-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.message.status, StudioMessageStatus::Completed);
    assert_eq!(stored.message.updated_at, 13);
}
