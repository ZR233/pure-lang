use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};

use super::*;
use crate::studio::store_support::STUDIO_DATABASE_SCHEMA_VERSION;

#[tokio::test]
async fn base_schema_contains_framework_and_task_tables() {
    let store = StudioStore::open_memory().await.unwrap();
    let names = store
        .db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master
             WHERE type = 'table'
               AND name IN (
                 'agent_runtime_states', 'agent_runtime_sessions',
                 'agent_runtime_traces', 'agent_framework_events',
                 'agent_pending_inputs', 'agent_turns',
                 'session_event_journal', 'session_view_snapshots',
                 'task_runs', 'work_units', 'agent_outcomes',
                 'merge_records', 'review_rounds', 'branch_leases'
               )
             ORDER BY name"
                .to_string(),
        ))
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.try_get::<String>("", "name").unwrap())
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec![
            "agent_framework_events",
            "agent_outcomes",
            "agent_pending_inputs",
            "agent_runtime_sessions",
            "agent_runtime_states",
            "agent_runtime_traces",
            "agent_turns",
            "branch_leases",
            "merge_records",
            "review_rounds",
            "session_event_journal",
            "session_view_snapshots",
            "task_runs",
            "work_units",
        ]
    );
    assert_eq!(
        schema_version(&store.db).await,
        STUDIO_DATABASE_SCHEMA_VERSION
    );
}

#[tokio::test]
async fn base_schema_has_only_canonical_session_projection_tables() {
    let store = StudioStore::open_memory().await.unwrap();

    for removed in [
        "agent_events",
        "agent_runtime_events",
        "agent_runtime_snapshots",
        "agents",
        "messages",
        "session_runtime_snapshots",
        "session_skills",
        "trace_events",
        "turns",
    ] {
        assert_eq!(
            table_columns(&store.db, removed).await,
            Vec::<String>::new()
        );
    }

    let work_unit_columns = table_columns(&store.db, "work_units").await;
    for required in ["base_commit", "worktree_path", "branch"] {
        assert!(work_unit_columns.iter().any(|column| column == required));
    }

    let runtime_session_columns = table_columns(&store.db, "agent_runtime_sessions").await;
    assert!(
        runtime_session_columns
            .iter()
            .any(|column| column == "trace_sequence")
    );
    assert!(
        runtime_session_columns
            .iter()
            .any(|column| column == "session_event_sequence")
    );
}

#[tokio::test]
async fn schema_mismatch_deletes_database_instead_of_migrating_it() {
    let db_path = unique_test_db_path("schema-reset");
    remove_test_db_files(&db_path).await;
    let db = Database::connect(sqlite_url_for_test(&db_path))
        .await
        .unwrap();
    execute_sql(
        &db,
        "CREATE TABLE legacy_only (id TEXT PRIMARY KEY);
         INSERT INTO legacy_only (id) VALUES ('must-disappear');
         PRAGMA user_version = 999;",
    )
    .await;
    db.close().await.unwrap();

    let store = StudioStore::open(&db_path).await.unwrap();

    let legacy_table = store
        .db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'legacy_only'"
                .to_string(),
        ))
        .await
        .unwrap();
    assert_eq!(legacy_table.is_none(), true);
    assert_eq!(
        schema_version(&store.db).await,
        STUDIO_DATABASE_SCHEMA_VERSION
    );
    assert_eq!(table_columns(&store.db, "projects").await.is_empty(), false);

    drop(store);
    remove_test_db_files(&db_path).await;
}

#[tokio::test]
async fn base_schema_has_no_migration_bookkeeping() {
    let store = StudioStore::open_memory().await.unwrap();
    let migration_table = store
        .db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name = 'studio_schema_migrations'"
                .to_string(),
        ))
        .await
        .unwrap();
    let migration_settings = store
        .db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM app_settings WHERE key LIKE 'migration:%'".to_string(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "count")
        .unwrap();

    assert_eq!(migration_table.is_none(), true);
    assert_eq!(migration_settings, 0);
}

async fn table_columns(db: &DatabaseConnection, table: &str) -> Vec<String> {
    db.query_all(Statement::from_string(
        DatabaseBackend::Sqlite,
        format!("PRAGMA table_info({table})"),
    ))
    .await
    .unwrap()
    .into_iter()
    .map(|row| row.try_get::<String>("", "name").unwrap())
    .collect()
}

async fn schema_version(db: &DatabaseConnection) -> i64 {
    db.query_one(Statement::from_string(
        DatabaseBackend::Sqlite,
        "PRAGMA user_version".to_string(),
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "user_version")
    .unwrap()
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
