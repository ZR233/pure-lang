use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn project_crud_orders_by_recent_open() {
    let store = StudioStore::open_memory().await.unwrap();

    let first = store.upsert_project("C:/work/alpha").await.unwrap();
    let second = store.upsert_project("C:/work/beta").await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    store.mark_project_opened(&first.id).await.unwrap();

    let projects = store.list_projects().await.unwrap();

    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].id, first.id);
    assert_eq!(projects[1].id, second.id);
}

#[tokio::test]
async fn archive_project_hides_project_and_clears_studio_history() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Build app", StudioMode::Simple)
        .await
        .unwrap();
    let message = Message {
        role: MessageRole::User,
        content: MessageContent::Text("hello".to_string()),
        reasoning_content: None,
        metadata: HashMap::new(),
    };
    store.append_message(&session.id, &message).await.unwrap();
    store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-1".to_string(),
            project_id: Some(project.id.clone()),
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 1,
            kind: StudioEventKind::MessageUpdated {
                message: Box::new(StudioMessage {
                    message_id: "turn-1:user".to_string(),
                    session_id: session.id.clone(),
                    turn_id: "turn-1".to_string(),
                    role: StudioMessageRole::User,
                    status: StudioMessageStatus::Completed,
                    created_at: 1,
                    updated_at: 1,
                    completed_at: Some(1),
                    error: None,
                    metadata: serde_json::json!({}),
                }),
            },
        })
        .await
        .unwrap();
    store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-2".to_string(),
            project_id: Some(project.id.clone()),
            session_id: Some(session.id.clone()),
            turn_id: Some("turn-1".to_string()),
            sequence: 0,
            created_at: 1,
            kind: StudioEventKind::MessagePartUpdated {
                part: Box::new(StudioPart {
                    part_id: "turn-1:user-text".to_string(),
                    message_id: "turn-1:user".to_string(),
                    session_id: session.id.clone(),
                    turn_id: "turn-1".to_string(),
                    part_type: StudioPartType::Text,
                    order: 1,
                    revision: 0,
                    status: StudioPartStatus::Completed,
                    created_at: 1,
                    updated_at: 1,
                    completed_at: Some(1),
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
                }),
            },
        })
        .await
        .unwrap();
    store
        .create_turn(&session.id, "turn-1", crate::StudioTurnStatus::Queued, 1)
        .await
        .unwrap();
    store
        .upsert_agent_snapshot(AgentSnapshotRecord {
            id: "agent-1".to_string(),
            session_id: session.id.clone(),
            path: "/root/research".to_string(),
            parent_path: None,
            role: "executor".to_string(),
            task: "research".to_string(),
            status: AgentStatus::Running,
            summary: None,
            depth: 1,
            error: None,
            reason: None,
            budget_limit_kind: None,
            budget_usage: None,
            runtime_usage: None,
            updated_at: 10,
        })
        .await
        .unwrap();
    store
        .record_agent_event(AgentTimelineEventRecord {
            event_id: "event-1".to_string(),
            session_id: session.id.clone(),
            sequence: 0,
            kind: "agentStatus".to_string(),
            agent_id: Some("agent-1".to_string()),
            path: Some("/root/research".to_string()),
            parent_path: None,
            payload_json: "{}".to_string(),
            created_at: 1,
        })
        .await
        .unwrap();
    let runtime_delta = AgentRuntimeDelta {
        inference_id: "root-1".to_string(),
        agent_id: "agent-root".to_string(),
        path: "/root".to_string(),
        parent_path: None,
        role: "root".to_string(),
        model: "model".to_string(),
        context_window: Some(128_000),
        usage: TokenUsageSnapshot {
            prompt_tokens: 10,
            completion_tokens: 2,
            total_tokens: 12,
            cached_prompt_tokens: 0,
        },
        estimated_costs: Vec::new(),
        has_unpriced_usage: true,
        updated_at: 20,
    };
    store
        .record_agent_runtime_delta(&session.id, &runtime_delta)
        .await
        .unwrap();

    let archived = store.archive_project(&project.id).await.unwrap().unwrap();
    let hidden_projects = store.list_projects().await.unwrap();
    let sessions = store.list_sessions(&project.id).await.unwrap();
    let messages = store.load_messages(&session.id).await.unwrap();
    let studio_events = store
        .load_studio_events(&session.id, None, None)
        .await
        .unwrap();
    let studio_messages = store.load_studio_messages(&session.id).await.unwrap();
    let message_parts = store.load_message_parts(&session.id).await.unwrap();
    let turn = store
        .set_turn_status("turn-1", crate::StudioTurnStatus::Completed, None, 2)
        .await
        .unwrap();
    let agents = store.list_agents(&session.id).await.unwrap();
    let agent_events = store.list_agent_events(&session.id).await.unwrap();
    let runtime = store.load_session_runtime(&session.id).await.unwrap();
    let skills = store.list_session_skills(&session.id).await.unwrap();
    let reopened = store.upsert_project("C:/work/alpha").await.unwrap();
    let visible_projects = store.list_projects().await.unwrap();
    let reopened_sessions = store.list_sessions(&project.id).await.unwrap();

    assert_eq!(archived.id, project.id);
    assert_eq!(hidden_projects, Vec::<ProjectRecord>::new());
    assert_eq!(sessions, Vec::<SessionRecord>::new());
    assert_eq!(messages, Vec::<Message>::new());
    assert_eq!(studio_events, Vec::<StudioEventEnvelope>::new());
    assert_eq!(studio_messages, Vec::new());
    assert_eq!(message_parts, Vec::new());
    assert_eq!(turn, None);
    assert_eq!(agents, Vec::<AgentSnapshotRecord>::new());
    assert_eq!(agent_events, Vec::<AgentTimelineEventRecord>::new());
    assert_eq!(runtime, None);
    assert_eq!(skills, Vec::<SessionSkillRecord>::new());
    assert_eq!(reopened.id, project.id);
    assert_eq!(visible_projects[0].id, project.id);
    assert_eq!(reopened_sessions, Vec::<SessionRecord>::new());
}
