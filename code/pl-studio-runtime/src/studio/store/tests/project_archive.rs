use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn archive_project_removes_canonical_session_state() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store.upsert_project("C:/work/alpha").await.unwrap();
    let session = store
        .create_session(&project.id, "Build app", StudioMode::Simple)
        .await
        .unwrap();
    let agent_id = "agent-project-archive";

    for sql in [
        format!(
            "INSERT INTO agent_runtime_states
             (agent_id, revision, snapshot_json, updated_at)
             VALUES ('{agent_id}', 1, '{{}}', 1)"
        ),
        format!(
            "INSERT INTO agent_runtime_sessions
             (agent_id, session_id, metadata_json, context_json, usage_json, updated_at)
             VALUES ('{agent_id}', '{}', '{{}}', '[]', '{{}}', 1)",
            session.id
        ),
        format!(
            "INSERT INTO agent_framework_events
             (agent_id, sequence, payload_json, created_at)
             VALUES ('{agent_id}', 1, '{{}}', 1)"
        ),
        format!(
            "INSERT INTO agent_pending_inputs
             (agent_id, queue_position, input_json)
             VALUES ('{agent_id}', 0, '{{}}')"
        ),
        format!(
            "INSERT INTO agent_active_inputs
             (agent_id, input_json, updated_at)
             VALUES ('{agent_id}', '{{}}', 1)"
        ),
        format!(
            "INSERT INTO agent_wake_receipts
             (agent_id, wake_id, receipt_json, accepted_at)
             VALUES ('{agent_id}', 'wake-1', '{{}}', 1)"
        ),
        format!(
            "INSERT INTO agent_turns
             (agent_id, turn_id, session_id, status, usage_json)
             VALUES ('{agent_id}', 'turn-1', '{}', 'running', '{{}}')",
            session.id
        ),
        format!(
            "INSERT INTO agent_runtime_traces
             (agent_id, session_id, sequence, payload_json, created_at)
             VALUES ('{agent_id}', '{}', 1, '{{}}', 1)",
            session.id
        ),
        format!(
            "INSERT INTO session_event_journal
             (session_id, sequence, event_json, emitted_at)
             VALUES ('{}', 1, '{{}}', 1)",
            session.id
        ),
        format!(
            "INSERT INTO session_view_snapshots
             (session_id, through_sequence, snapshot_json, updated_at)
             VALUES ('{}', 1, '{{}}', 1)",
            session.id
        ),
    ] {
        store
            .db
            .execute(Statement::from_string(DatabaseBackend::Sqlite, sql))
            .await
            .unwrap();
    }

    store.archive_project(&project.id).await.unwrap().unwrap();

    let mut remaining = Vec::new();
    for table in [
        "agent_runtime_states",
        "agent_runtime_sessions",
        "agent_framework_events",
        "agent_pending_inputs",
        "agent_active_inputs",
        "agent_wake_receipts",
        "agent_turns",
        "agent_runtime_traces",
        "session_event_journal",
        "session_view_snapshots",
    ] {
        let count = store
            .db
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!("SELECT COUNT(*) AS count FROM {table}"),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "count")
            .unwrap();
        remaining.push((table, count));
    }

    assert_eq!(
        remaining,
        vec![
            ("agent_runtime_states", 0),
            ("agent_runtime_sessions", 0),
            ("agent_framework_events", 0),
            ("agent_pending_inputs", 0),
            ("agent_active_inputs", 0),
            ("agent_wake_receipts", 0),
            ("agent_turns", 0),
            ("agent_runtime_traces", 0),
            ("session_event_journal", 0),
            ("session_view_snapshots", 0),
        ]
    );
}
