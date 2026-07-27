use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::StudioStore;

#[tokio::test]
async fn managed_task_continuation_is_deduplicated_while_live() {
    let store = StudioStore::open_memory().await.unwrap();
    store
        .database()
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO agent_turns
             (agent_id, turn_id, session_id, status, usage_json, metadata_json)
             VALUES ('root-agent', 'turn-1', 'session-1', 'queued', '{}', ?)",
            [serde_json::json!({
                "taskRunId": "task-1",
                "historyPolicy": "normal",
            })
            .to_string()
            .into()],
        ))
        .await
        .unwrap();

    assert!(store.has_live_task_continuation("task-1").await.unwrap());

    store
        .database()
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE agent_turns
             SET status = 'running'
             WHERE agent_id = 'root-agent' AND turn_id = 'turn-1'",
            [],
        ))
        .await
        .unwrap();
    assert!(
        !store.has_live_task_continuation("task-1").await.unwrap(),
        "a normal root turn must not consume a child-terminal wakeup"
    );

    store
        .database()
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "UPDATE agent_turns
             SET metadata_json = ?
             WHERE agent_id = 'root-agent' AND turn_id = 'turn-1'",
            [serde_json::json!({
                "taskRunId": "task-1",
                "historyPolicy": "ephemeral",
            })
            .to_string()
            .into()],
        ))
        .await
        .unwrap();
    for status in ["running", "waiting_tool", "waiting_interaction"] {
        store
            .database()
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "UPDATE agent_turns
                 SET status = ?
                 WHERE agent_id = 'root-agent' AND turn_id = 'turn-1'",
                [status.to_string().into()],
            ))
            .await
            .unwrap();
        assert!(
            store.has_live_task_continuation("task-1").await.unwrap(),
            "managed continuation status {status} must suppress duplicate dispatch"
        );
    }

    store
        .database()
        .execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "UPDATE agent_turns
             SET status = 'completed'
             WHERE agent_id = 'root-agent' AND turn_id = 'turn-1'"
                .to_string(),
        ))
        .await
        .unwrap();
    assert!(!store.has_live_task_continuation("task-1").await.unwrap());
}
