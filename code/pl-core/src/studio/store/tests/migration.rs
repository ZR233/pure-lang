use super::*;
use pretty_assertions::assert_eq;
use sea_orm::{Database, DatabaseConnection};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

#[tokio::test]
async fn migrations_prune_legacy_agent_timeline_events() {
    let db_path = unique_test_db_path("legacy-agent-timeline");
    remove_test_db_files(&db_path).await;
    seed_agent_events_db_at_migration_22(&db_path).await;

    let store = StudioStore::open(&db_path).await.unwrap();

    let events = store.list_agent_events("session-1").await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_id, "canonical-event");
    assert_eq!(events[0].kind, "spawned");

    let migration_marker = store
        .db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT key FROM app_settings WHERE key = 'migration:22'".to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(migration_marker.len(), 1);

    drop(store);
    remove_test_db_files(&db_path).await;
}

async fn seed_agent_events_db_at_migration_22(path: &Path) {
    let db = Database::connect(sqlite_url_for_test(path)).await.unwrap();
    execute_sql(
        &db,
        "CREATE TABLE app_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at INTEGER NOT NULL
        );",
    )
    .await;
    execute_sql(
        &db,
        "CREATE TABLE studio_schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );",
    )
    .await;
    execute_sql(
        &db,
        "CREATE TABLE agent_events (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            sequence INTEGER NOT NULL,
            kind TEXT NOT NULL,
            agent_id TEXT,
            path TEXT,
            parent_path TEXT,
            payload_json TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );",
    )
    .await;

    for version in 1..=21 {
        execute_sql(
            &db,
            format!(
                "INSERT INTO studio_schema_migrations (version, applied_at)
                 VALUES ({version}, 1);"
            ),
        )
        .await;
        execute_sql(
            &db,
            format!(
                "INSERT INTO app_settings (key, value, updated_at)
                 VALUES ('migration:{version}', 'applied', 1);"
            ),
        )
        .await;
    }

    let legacy_payload = serde_json::json!({
        "eventId": "legacy-event",
        "sessionId": "session-1",
        "sequence": 1,
        "createdAt": 1,
        "kind": {
            "type": "spawnBegin",
            "callId": "legacy-call",
            "senderPath": "root",
            "taskName": "reviewer",
            "prompt": "Review timeline state",
            "role": "reviewer"
        }
    })
    .to_string();
    let canonical_payload = serde_json::json!({
        "eventId": "canonical-event",
        "sessionId": "session-1",
        "sequence": 2,
        "createdAt": 2,
        "kind": {
            "type": "subAgentActivity",
            "callId": "canonical-call",
            "path": "root/reviewer",
            "parentPath": "root",
            "kind": "spawned",
            "status": "running",
            "message": "Review timeline state"
        }
    })
    .to_string();
    execute_sql(
        &db,
        format!(
            "INSERT INTO agent_events
             (id, session_id, sequence, kind, agent_id, path, parent_path, payload_json, created_at)
             VALUES
             ('legacy-event', 'session-1', 1, 'spawnBegin', NULL, NULL, 'root', '{}', 1),
             ('canonical-event', 'session-1', 2, 'spawned', NULL, 'root/reviewer', 'root', '{}', 2);",
            legacy_payload, canonical_payload
        ),
    )
    .await;
    db.close().await.unwrap();
}

async fn execute_sql(db: &DatabaseConnection, sql: impl Into<String>) {
    db.execute(Statement::from_string(DatabaseBackend::Sqlite, sql.into()))
        .await
        .unwrap();
}

fn unique_test_db_path(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "pure-studio-{name}-{}-{stamp}.sqlite",
        std::process::id()
    ))
}

fn sqlite_url_for_test(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    format!("sqlite://{path}?mode=rwc")
}

async fn remove_test_db_files(path: &Path) {
    let _ = tokio::fs::remove_file(path).await;
    let path_text = path.to_string_lossy();
    for suffix in ["-wal", "-shm"] {
        let _ = tokio::fs::remove_file(format!("{path_text}{suffix}")).await;
    }
}
