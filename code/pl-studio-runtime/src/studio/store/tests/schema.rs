use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};

use super::*;
use crate::studio::store_support::{
    HISTORY_DATABASE_SCHEMA_VERSION, STATE_DATABASE_SCHEMA_VERSION,
};

#[tokio::test]
async fn empty_pair_is_created_from_entities_with_matching_generation() {
    let root = unique_test_root("entity-pair");
    let state_path = root.join("studio_state.sqlite");
    let history_path = derived_history_path(&state_path);
    let store = StudioStore::open(&state_path).await.unwrap();

    assert_eq!(
        schema_version(store.database()).await,
        STATE_DATABASE_SCHEMA_VERSION
    );
    assert_eq!(
        schema_version(store.history_database()).await,
        HISTORY_DATABASE_SCHEMA_VERSION
    );
    for table in [
        "projects",
        "sessions",
        "agent_runtime_states",
        "history_gc_jobs",
    ] {
        assert!(
            table_exists(store.database(), table).await,
            "missing {table}"
        );
    }
    for table in [
        "session_history_turns",
        "session_history_items",
        "session_history_checkpoints",
    ] {
        assert!(
            table_exists(store.history_database(), table).await,
            "missing {table}"
        );
    }
    for removed in [
        "agent_framework_events",
        "agent_runtime_traces",
        "session_event_journal",
        "tool_approvals",
    ] {
        assert!(!table_exists(store.database(), removed).await);
        assert!(!table_exists(store.history_database(), removed).await);
    }
    let state_metadata = storage_metadata(store.database()).await;
    let history_metadata = storage_metadata(store.history_database()).await;
    assert_eq!(state_metadata.0, "state");
    assert_eq!(state_metadata.1, STATE_DATABASE_SCHEMA_VERSION);
    assert_eq!(history_metadata.0, "history");
    assert_eq!(history_metadata.1, HISTORY_DATABASE_SCHEMA_VERSION);
    assert_eq!(state_metadata.2, history_metadata.2);
    assert!(tokio::fs::try_exists(&state_path).await.unwrap());
    assert!(tokio::fs::try_exists(&history_path).await.unwrap());

    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn legacy_v10_database_and_sidecars_are_archived_without_importing_rows() {
    let root = unique_test_root("legacy-archive");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let db_path = root.join("studio_state.sqlite");
    let legacy = Database::connect(sqlite_url(&db_path, "rwc"))
        .await
        .unwrap();
    execute_sql(
        &legacy,
        "CREATE TABLE legacy_only (id TEXT PRIMARY KEY);
         INSERT INTO legacy_only (id) VALUES ('preserve-in-archive');
         PRAGMA user_version = 10;",
    )
    .await;
    legacy.close().await.unwrap();
    tokio::fs::write(sidecar_path(&db_path, "-wal"), b"legacy-wal")
        .await
        .unwrap();
    tokio::fs::write(sidecar_path(&db_path, "-shm"), b"legacy-shm")
        .await
        .unwrap();

    let store = StudioStore::open(&db_path).await.unwrap();
    assert_eq!(
        schema_version(store.database()).await,
        STATE_DATABASE_SCHEMA_VERSION
    );
    assert!(!table_exists(store.database(), "legacy_only").await);

    let archive_root = root.join("archive");
    let mut entries = tokio::fs::read_dir(&archive_root).await.unwrap();
    let archive = entries.next_entry().await.unwrap().unwrap().path();
    assert!(entries.next_entry().await.unwrap().is_none());
    let archived_db = archive.join("studio_state.sqlite");
    assert!(
        tokio::fs::try_exists(archive.join("studio_state.sqlite-wal"))
            .await
            .unwrap()
    );
    assert!(
        tokio::fs::try_exists(archive.join("studio_state.sqlite-shm"))
            .await
            .unwrap()
    );
    let archived = Database::connect(sqlite_url(&archived_db, "ro"))
        .await
        .unwrap();
    assert_eq!(schema_version(&archived).await, 10);
    let preserved: String = archived
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id FROM legacy_only".to_string(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();
    assert_eq!(preserved, "preserve-in-archive");
    archived.close().await.unwrap();

    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn incomplete_pair_is_rejected_without_rebuilding_missing_database() {
    let root = unique_test_root("incomplete-pair");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let state_path = root.join("studio_state.sqlite");
    let db = Database::connect(sqlite_url(&state_path, "rwc"))
        .await
        .unwrap();
    execute_sql(&db, "PRAGMA user_version = 11;").await;
    db.close().await.unwrap();

    let error = match StudioStore::open(&state_path).await {
        Ok(_) => panic!("incomplete pair must be rejected"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("数据库不完整"));
    assert!(tokio::fs::try_exists(&state_path).await.unwrap());
    assert!(
        !tokio::fs::try_exists(derived_history_path(&state_path))
            .await
            .unwrap()
    );

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn generation_mismatch_is_rejected_without_modifying_either_database() {
    let root = unique_test_root("generation-mismatch");
    let state_path = root.join("studio_state.sqlite");
    let store = StudioStore::open(&state_path).await.unwrap();
    execute_sql(
        store.history_database(),
        "UPDATE storage_metadata SET storage_generation_id = 'different-generation';",
    )
    .await;

    let error = match StudioStore::open(&state_path).await {
        Ok(_) => panic!("generation mismatch must be rejected"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("generation 不匹配"));
    assert_eq!(
        storage_metadata(store.history_database()).await.2,
        "different-generation"
    );

    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn future_state_schema_is_rejected_before_missing_pair() {
    let root = unique_test_root("future-schema");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let db_path = root.join("studio_state.sqlite");
    let db = Database::connect(sqlite_url(&db_path, "rwc"))
        .await
        .unwrap();
    execute_sql(
        &db,
        "CREATE TABLE future_only (id TEXT PRIMARY KEY);
         INSERT INTO future_only (id) VALUES ('must-remain');
         PRAGMA user_version = 999;",
    )
    .await;
    db.close().await.unwrap();
    let error = match StudioStore::open(&db_path).await {
        Ok(_) => panic!("future schema must be rejected"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("高于当前支持版本"));
    let preserved = Database::connect(sqlite_url(&db_path, "ro")).await.unwrap();
    assert_eq!(schema_version(&preserved).await, 999);
    assert!(table_exists(&preserved, "future_only").await);
    preserved.close().await.unwrap();
    assert!(
        !tokio::fs::try_exists(derived_history_path(&db_path))
            .await
            .unwrap()
    );

    let _ = tokio::fs::remove_dir_all(root).await;
}

async fn table_exists(db: &DatabaseConnection, table: &str) -> bool {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name = ?",
        [table.into()],
    ))
    .await
    .unwrap()
    .is_some()
}

async fn schema_version(db: &DatabaseConnection) -> i64 {
    db.query_one_raw(Statement::from_string(
        DatabaseBackend::Sqlite,
        "PRAGMA user_version".to_string(),
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "user_version")
    .unwrap()
}

async fn storage_metadata(db: &DatabaseConnection) -> (String, i64, String) {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT database_kind, schema_version, storage_generation_id
             FROM storage_metadata WHERE id = 'primary'"
                .to_string(),
        ))
        .await
        .unwrap()
        .unwrap();
    (
        row.try_get("", "database_kind").unwrap(),
        row.try_get("", "schema_version").unwrap(),
        row.try_get("", "storage_generation_id").unwrap(),
    )
}

async fn execute_sql(db: &DatabaseConnection, sql: impl Into<String>) {
    db.execute_raw(Statement::from_string(DatabaseBackend::Sqlite, sql.into()))
        .await
        .unwrap();
}

fn unique_test_root(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pure-studio-{name}-{}-{stamp}", std::process::id()))
}

fn derived_history_path(state_path: &Path) -> PathBuf {
    let stem = state_path.file_stem().unwrap().to_string_lossy();
    state_path.with_file_name(format!("{stem}.history.sqlite"))
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.display()))
}

fn sqlite_url(path: &Path, mode: &str) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    format!("sqlite://{path}?mode={mode}")
}
