use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn append_studio_event_projects_message_part_snapshots() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Conversation", StudioMode::Simple)
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
    let message_event = store
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
        part_id: "turn-1:assistant-final".to_string(),
        message_id: "turn-1:assistant".to_string(),
        session_id: session.id.clone(),
        turn_id: "turn-1".to_string(),
        part_type: StudioPartType::Text,
        order: 999,
        revision: 0,
        status: StudioPartStatus::Completed,
        created_at: 10,
        updated_at: 11,
        completed_at: Some(11),
        error: None,
        text_channel: Some(StudioTextChannel::Final),
        activity_group_id: None,
        text: "hello".to_string(),
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
    let part_event = store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-2".to_string(),
            project_id: Some(project.id),
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 11,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(part),
            },
        })
        .await
        .unwrap();

    let StudioEventKind::MessageUpdated { message } = &message_event.kind else {
        panic!("expected message snapshot");
    };
    assert_eq!(message_event.sequence, 0);
    assert_eq!(message.message_id, "turn-1:assistant");
    let StudioEventKind::MessagePartUpdated { part } = &part_event.kind else {
        panic!("expected part snapshot");
    };
    assert_eq!(part_event.sequence, 1);
    assert_eq!(part.order, 999);

    let stored_events = store
        .load_studio_events(&session.id, None, None)
        .await
        .unwrap();
    let StudioEventKind::MessagePartUpdated { part } = &stored_events[1].kind else {
        panic!("expected stored part snapshot");
    };
    assert_eq!(stored_events[1].sequence, 1);
    assert_eq!(part.order, 999);
    assert_eq!(part.text, "hello");

    let messages = store.load_studio_messages(&session.id).await.unwrap();
    let parts = store.load_message_parts(&session.id).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].part.order, 999);
}

#[tokio::test]
async fn core_trace_user_snapshot_does_not_duplicate_canonical_user_part() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Conversation", StudioMode::Simple)
        .await
        .unwrap();
    let runtime = StudioEventRuntime::new(store.clone());
    let message = StudioMessage {
        message_id: "turn-1:user".to_string(),
        session_id: session.id.clone(),
        turn_id: "turn-1".to_string(),
        role: StudioMessageRole::User,
        status: StudioMessageStatus::Completed,
        created_at: 10,
        updated_at: 10,
        completed_at: Some(10),
        error: None,
        metadata: serde_json::json!({}),
    };
    let part = StudioPart {
        part_id: "turn-1:user-text".to_string(),
        message_id: "turn-1:user".to_string(),
        session_id: session.id.clone(),
        turn_id: "turn-1".to_string(),
        part_type: StudioPartType::Text,
        order: 0,
        revision: 0,
        status: StudioPartStatus::Completed,
        created_at: 10,
        updated_at: 10,
        completed_at: Some(10),
        error: None,
        text_channel: Some(StudioTextChannel::User),
        activity_group_id: None,
        text: "hello".to_string(),
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
    runtime
        .emit(
            Some(project.id.clone()),
            Some(session.id.clone()),
            Some("turn-1".to_string()),
            StudioEventKind::MessageUpdated {
                message: Box::new(message),
            },
        )
        .await
        .unwrap();
    runtime
        .emit(
            Some(project.id),
            Some(session.id.clone()),
            Some("turn-1".to_string()),
            StudioEventKind::MessagePartUpdated {
                part: Box::new(part),
            },
        )
        .await
        .unwrap();

    let trace_item = TracePart {
        turn_id: "turn-1".to_string(),
        item_id: "turn-1-user".to_string(),
        started_sequence: 0,
        revision: 0,
        kind: TracePartKind::Text,
        status: TracePartStatus::Completed,
        created_at: 11,
        updated_at: 11,
        source: TracePartSource::Model,
        text_channel: Some(TraceTextChannel::User),
        content: "hello".to_string(),
        attachments: Vec::new(),
        thinking_chunks: Vec::new(),
        tool: None,
        agent: None,
        inference: None,
        usage: None,
    };
    let emitted = runtime
        .emit_agent_event(
            &session.id,
            pl_trace::AgentEvent::TracePartCompleted { item: trace_item },
        )
        .await
        .unwrap();
    assert_eq!(emitted, None);

    let parts = store.load_message_parts(&session.id).await.unwrap();
    let events = store
        .load_studio_events(&session.id, None, None)
        .await
        .unwrap();
    let user_part_events = events
        .iter()
        .filter_map(|event| match &event.kind {
            StudioEventKind::MessagePartUpdated { part } if part.message_id == "turn-1:user" => {
                Some(part.part_id.as_str())
            }
            StudioEventKind::MessageUpdated { .. }
            | StudioEventKind::MessageRemoved { .. }
            | StudioEventKind::MessagePartUpdated { .. }
            | StudioEventKind::MessagePartRemoved { .. }
            | StudioEventKind::MessagePartDelta { .. }
            | StudioEventKind::TurnChanged { .. }
            | StudioEventKind::InteractionChanged { .. }
            | StudioEventKind::PlanLifecycleChanged { .. }
            | StudioEventKind::SessionRuntimeChanged { .. }
            | StudioEventKind::AgentChanged { .. }
            | StudioEventKind::AgentTimelineChanged { .. }
            | StudioEventKind::SkillActivated { .. }
            | StudioEventKind::SessionHandoffChanged { .. }
            | StudioEventKind::SessionListChanged { .. }
            | StudioEventKind::McpHealthChanged { .. }
            | StudioEventKind::LspHealthChanged { .. }
            | StudioEventKind::Stale { .. } => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].part.part_id, "turn-1:user-text");
    assert_eq!(parts[0].part.message_id, "turn-1:user");
    assert_eq!(user_part_events, vec!["turn-1:user-text"]);
}
