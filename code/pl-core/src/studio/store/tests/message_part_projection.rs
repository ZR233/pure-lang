use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn message_part_snapshot_projection_preserves_first_order() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Conversation", CompileMode::Auto)
        .await
        .unwrap();
    let message = StudioMessage {
        message_id: "turn-1-assistant".to_string(),
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
    store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-1".to_string(),
            project_id: Some(project.id.clone()),
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 10,
            kind: StudioEventKind::MessageUpdated {
                message: Box::new(message),
            },
        })
        .await
        .unwrap();
    let part = StudioPart {
        part_id: "turn-1-final".to_string(),
        message_id: "turn-1-assistant".to_string(),
        session_id: session.id.clone(),
        turn_id: "turn-1".to_string(),
        part_type: StudioPartType::Text,
        order: 2,
        revision: 0,
        status: StudioPartStatus::Streaming,
        created_at: 10,
        updated_at: 10,
        completed_at: None,
        error: None,
        text_channel: Some(StudioTextChannel::Final),
        activity_group_id: None,
        text: String::new(),
        attachments: Vec::new(),
        tool: None,
        agent: None,
        inference: None,
        plan: None,
        file: None,
        usage: None,
        synthetic: false,
        ignored: false,
    };
    let first_part = store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-2".to_string(),
            project_id: Some(project.id.clone()),
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 10,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(part.clone()),
            },
        })
        .await
        .unwrap();
    let mut duplicate_order_part = part.clone();
    duplicate_order_part.part_id = "turn-1-other-final".to_string();
    let err = store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-duplicate-order".to_string(),
            project_id: Some(project.id.clone()),
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 10,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(duplicate_order_part),
            },
        })
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("part order already exists for message")
    );

    let mut completed_part = part;
    completed_part.order = 2;
    completed_part.revision = 1;
    completed_part.status = StudioPartStatus::Completed;
    completed_part.text = "hello".to_string();
    completed_part.updated_at = 11;
    completed_part.completed_at = Some(11);
    let second_part = store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-3".to_string(),
            project_id: Some(project.id),
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 11,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(completed_part),
            },
        })
        .await
        .unwrap();

    let StudioEventKind::MessagePartUpdated { part } = first_part.kind else {
        panic!("expected first part snapshot");
    };
    assert_eq!(part.order, 2);
    let StudioEventKind::MessagePartUpdated { part } = second_part.kind else {
        panic!("expected second part snapshot");
    };
    assert_eq!(part.order, 2);

    let parts = store.load_message_parts(&session.id).await.unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].part.order, 2);
    assert_eq!(parts[0].part.text, "hello");
    assert_eq!(parts[0].sequence, 2);
}

#[tokio::test]
async fn message_part_snapshot_round_trip_preserves_activity_group_id() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Conversation", CompileMode::Auto)
        .await
        .unwrap();
    let message = StudioMessage {
        message_id: "turn-1:assistant".to_string(),
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
    store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-tool-message".to_string(),
            project_id: Some(project.id.clone()),
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 10,
            kind: StudioEventKind::MessageUpdated {
                message: Box::new(message),
            },
        })
        .await
        .unwrap();
    let part = StudioPart {
        part_id: "turn-1:part-1".to_string(),
        message_id: "turn-1:assistant".to_string(),
        session_id: session.id.clone(),
        turn_id: "turn-1".to_string(),
        part_type: StudioPartType::Tool,
        order: 1,
        revision: 0,
        status: StudioPartStatus::Running,
        created_at: 10,
        updated_at: 10,
        completed_at: None,
        error: None,
        text_channel: None,
        activity_group_id: Some("tool-group:turn-1:1".to_string()),
        text: String::new(),
        attachments: Vec::new(),
        tool: Some(StudioToolPart {
            tool_call_id: "tool-a".to_string(),
            call_id: Some("call-a".to_string()),
            provider_item_id: Some("item-a".to_string()),
            name: "bash".to_string(),
            arguments: "{\"command\":\"cargo test -p pl-core\"}".to_string(),
            result: None,
            exit_code: None,
            timed_out: false,
            working_directory: Some("D:/work".to_string()),
            denial_reason: None,
        }),
        agent: None,
        inference: None,
        plan: None,
        file: None,
        usage: None,
        synthetic: false,
        ignored: false,
    };
    store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-tool-part".to_string(),
            project_id: Some(project.id),
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 10,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(part.clone()),
            },
        })
        .await
        .unwrap();

    let parts = store.load_message_parts(&session.id).await.unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(
        parts[0].part.activity_group_id.as_deref(),
        Some("tool-group:turn-1:1")
    );

    let events = store
        .load_studio_events(&session.id, None, None)
        .await
        .unwrap();
    let StudioEventKind::MessagePartUpdated { part } = &events[1].kind else {
        panic!("expected tool part snapshot");
    };
    assert_eq!(
        part.activity_group_id.as_deref(),
        Some("tool-group:turn-1:1")
    );
}

#[tokio::test]
async fn message_part_delta_is_not_durable() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/beta").await.unwrap();
    let session = store
        .create_session(&project.id, "Live", CompileMode::Auto)
        .await
        .unwrap();
    let err = store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-live".to_string(),
            project_id: Some(project.id),
            session_id: Some(session.id),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 10,
            kind: StudioEventKind::MessagePartDelta {
                delta: StudioPartDelta {
                    part_id: "part-1".to_string(),
                    revision: 1,
                    field: StudioPartDeltaField::Text,
                    delta: "live".to_string(),
                    chunk_index: None,
                },
            },
        })
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("messagePartDelta is live-only and must not be persisted")
    );
}

#[tokio::test]
async fn invalid_message_part_projection_updates_are_rejected() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/beta").await.unwrap();
    let session = store
        .create_session(&project.id, "Terminal", CompileMode::Auto)
        .await
        .unwrap();
    let part = StudioPart {
        part_id: "part-terminal".to_string(),
        message_id: "message-terminal".to_string(),
        session_id: session.id.clone(),
        turn_id: "turn-1".to_string(),
        part_type: StudioPartType::Text,
        order: 1,
        revision: 1,
        status: StudioPartStatus::Completed,
        created_at: 10,
        updated_at: 11,
        completed_at: Some(11),
        error: None,
        text_channel: Some(StudioTextChannel::Final),
        activity_group_id: None,
        text: "done".to_string(),
        attachments: Vec::new(),
        tool: None,
        agent: None,
        inference: None,
        plan: None,
        file: None,
        usage: None,
        synthetic: false,
        ignored: false,
    };
    store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-terminal".to_string(),
            project_id: Some(project.id.clone()),
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 11,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(part.clone()),
            },
        })
        .await
        .unwrap();

    let stored_parts = store.load_message_parts(&session.id).await.unwrap();
    assert_eq!(stored_parts.len(), 1);
    let mut stale_streaming = stored_parts[0].part.clone();
    stale_streaming.status = StudioPartStatus::Streaming;
    stale_streaming.revision = 2;
    stale_streaming.text = "partial".to_string();
    let err = store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-streaming".to_string(),
            project_id: Some(project.id),
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 12,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(stale_streaming),
            },
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("invalid part transition"));

    let parts = store.load_message_parts(&session.id).await.unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].part.status, StudioPartStatus::Completed);
    assert_eq!(parts[0].part.revision, 1);
    assert_eq!(parts[0].part.text, "done");

    let stored_terminal = parts[0].part.clone();
    let mut changed_created_at = stored_terminal.clone();
    changed_created_at.created_at = 9;
    let err = store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-created-at-change".to_string(),
            project_id: None,
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 13,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(changed_created_at),
            },
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("part createdAt cannot change"));

    let mut changed_terminal_text = stored_terminal;
    changed_terminal_text.text = "changed".to_string();
    let err = store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-terminal-content-change".to_string(),
            project_id: None,
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 14,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(changed_terminal_text),
            },
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("terminal part cannot change"));

    let mutable_part = StudioPart {
        part_id: "part-mutable".to_string(),
        message_id: "message-terminal".to_string(),
        session_id: session.id.clone(),
        turn_id: "turn-1".to_string(),
        part_type: StudioPartType::Text,
        order: 2,
        revision: 3,
        status: StudioPartStatus::Streaming,
        created_at: 12,
        updated_at: 12,
        completed_at: None,
        error: None,
        text_channel: Some(StudioTextChannel::Final),
        activity_group_id: None,
        text: "live".to_string(),
        attachments: Vec::new(),
        tool: None,
        agent: None,
        inference: None,
        plan: None,
        file: None,
        usage: None,
        synthetic: false,
        ignored: false,
    };
    store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-mutable".to_string(),
            project_id: None,
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 12,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(mutable_part.clone()),
            },
        })
        .await
        .unwrap();

    let parts = store.load_message_parts(&session.id).await.unwrap();
    let stored_mutable = parts
        .iter()
        .find(|record| record.part.part_id == "part-mutable")
        .expect("mutable part should be projected")
        .part
        .clone();

    let mut low_revision = stored_mutable.clone();
    low_revision.revision = 2;
    let err = store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-low-revision".to_string(),
            project_id: None,
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 13,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(low_revision),
            },
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("part revision cannot decrease"));

    let mut changed_order = stored_mutable;
    changed_order.revision = 4;
    changed_order.order = 9;
    let err = store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-order-change".to_string(),
            project_id: None,
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 14,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(changed_order),
            },
        })
        .await
        .unwrap_err();
    assert!(err.to_string().contains("part order cannot change"));
}
