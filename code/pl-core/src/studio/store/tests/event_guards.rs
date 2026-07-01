use super::*;

#[tokio::test]
async fn stale_event_is_not_durable() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/beta").await.unwrap();
    let session = store
        .create_session(&project.id, "Live", CompileMode::Auto)
        .await
        .unwrap();

    let err = store
        .append_studio_event(StudioEventEnvelope {
            event_id: "studio-event-stale".to_string(),
            project_id: Some(project.id),
            session_id: Some(session.id),
            turn_id: None,
            sequence: 0,
            created_at: 10,
            kind: StudioEventKind::Stale { lagged_events: 2 },
        })
        .await
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("stale is live-only and must not be persisted")
    );
}
