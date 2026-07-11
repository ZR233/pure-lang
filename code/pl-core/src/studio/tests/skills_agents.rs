use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn session_skills_persist_from_skill_activation_trace_events_and_dedupe() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Skills", CompileMode::Simple)
        .await
        .unwrap();
    let first = SkillActivation {
        name: "skill-creator".to_string(),
        source: "user".to_string(),
        path: "C:/skills/skill-creator".to_string(),
        turn_id: "turn-1".to_string(),
        tool_call_id: "call-1".to_string(),
        activated_at: 10,
    };
    let repeated = SkillActivation {
        name: "Skill-Creator".to_string(),
        source: "user".to_string(),
        path: "C:/skills/skill-creator".to_string(),
        turn_id: "turn-2".to_string(),
        tool_call_id: "call-2".to_string(),
        activated_at: 20,
    };

    store
        .append_turn_records(
            &session.id,
            &[
                TraceEvent {
                    session_id: session.id.clone(),
                    sequence: 0,
                    timestamp: 10,
                    kind: TraceEventKind::SkillActivated { activation: first },
                },
                TraceEvent {
                    session_id: session.id.clone(),
                    sequence: 1,
                    timestamp: 20,
                    kind: TraceEventKind::SkillActivated {
                        activation: repeated,
                    },
                },
            ],
            &[],
        )
        .await
        .unwrap();

    let skills = store.list_session_skills(&session.id).await.unwrap();
    let names = store.list_session_skill_names(&session.id).await.unwrap();

    assert_eq!(names, vec!["Skill-Creator".to_string()]);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].first_turn_id, "turn-1");
    assert_eq!(skills[0].last_turn_id, "turn-2");
    assert_eq!(skills[0].last_tool_call_id, "call-2");
    assert_eq!(skills[0].activated_at, 10);
    assert_eq!(skills[0].updated_at, 20);
}

#[tokio::test]
async fn agent_trace_events_are_append_only_and_agents_are_snapshots() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Agent work", CompileMode::Simple)
        .await
        .unwrap();

    let base_snapshot = AgentSnapshotRecord {
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
    };
    store
        .upsert_agent_snapshot(base_snapshot.clone())
        .await
        .unwrap();
    store
        .upsert_agent_snapshot(AgentSnapshotRecord {
            status: AgentStatus::Completed,
            summary: Some("done".to_string()),
            updated_at: 20,
            ..base_snapshot.clone()
        })
        .await
        .unwrap();

    for sequence in [1, 2, 3] {
        store
            .record_agent_event(AgentTimelineEventRecord {
                event_id: format!("event-{sequence}"),
                session_id: session.id.clone(),
                sequence,
                kind: "agentStatus".to_string(),
                agent_id: Some("agent-1".to_string()),
                path: Some("/root/research".to_string()),
                parent_path: None,
                payload_json: format!(r#"{{"sequence":{sequence}}}"#),
                created_at: sequence,
            })
            .await
            .unwrap();
    }

    let agents = store.list_agents(&session.id).await.unwrap();
    let events = store.list_agent_events(&session.id).await.unwrap();

    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].status, AgentStatus::Completed);
    assert_eq!(agents[0].summary.as_deref(), Some("done"));
    assert_eq!(events.len(), 3);
    assert_eq!(
        events
            .iter()
            .map(|event| event.event_id.as_str())
            .collect::<Vec<_>>(),
        vec!["event-1", "event-2", "event-3"],
    );
}
