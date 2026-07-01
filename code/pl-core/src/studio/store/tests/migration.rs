use super::*;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn migrations_drop_legacy_agent_and_handoff_tables() {
    let store = StudioStore::open_memory().await.unwrap();
    let rows = store
        .db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master
             WHERE type = 'table'
               AND name IN ('agent_messages', 'agent_turns', 'session_handoffs')"
                .to_string(),
        ))
        .await
        .unwrap();

    assert_eq!(rows.len(), 0);
}
