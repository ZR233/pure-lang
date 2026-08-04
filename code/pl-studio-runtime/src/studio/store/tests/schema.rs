use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};

use super::*;
use crate::studio::store_support::STUDIO_DATABASE_SCHEMA_VERSION;

#[tokio::test]
async fn schema_v9_is_archived_and_rebuilt_without_importing_rows() {
    let db_path = unique_test_db_path("schema-v9-rebuild");
    remove_test_db_files(&db_path).await;
    let legacy = Database::connect(sqlite_url(&db_path, "rwc"))
        .await
        .unwrap();
    execute_sql(
        &legacy,
        "CREATE TABLE legacy_only (id TEXT PRIMARY KEY);
         INSERT INTO legacy_only (id) VALUES ('preserve-in-backup');
         PRAGMA user_version = 9;",
    )
    .await;
    legacy.close().await.unwrap();

    let store = StudioStore::open(&db_path).await.unwrap();
    assert_eq!(
        schema_version(store.database()).await,
        STUDIO_DATABASE_SCHEMA_VERSION
    );
    assert!(!table_exists(store.database(), "legacy_only").await);
    drop(store);

    let backup = PathBuf::from(format!("{}.legacy-v9.bak", db_path.display()));
    let archived = Database::connect(sqlite_url(&backup, "ro")).await.unwrap();
    assert_eq!(schema_version(&archived).await, 9);
    let preserved = archived
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id FROM legacy_only".to_string(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "id")
        .unwrap();
    assert_eq!(preserved, "preserve-in-backup");
    archived.close().await.unwrap();

    remove_test_db_files(&db_path).await;
    remove_test_db_files(&backup).await;
}

#[tokio::test]
async fn future_schema_is_rejected_without_modifying_database() {
    let db_path = unique_test_db_path("future-schema");
    remove_test_db_files(&db_path).await;
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
    remove_test_db_files(&db_path).await;
}

async fn table_exists(db: &DatabaseConnection, table: &str) -> bool {
    db.query_one(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name = ?",
        [table.into()],
    ))
    .await
    .unwrap()
    .is_some()
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

fn sqlite_url(path: &Path, mode: &str) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    format!("sqlite://{path}?mode={mode}")
}

async fn remove_test_db_files(path: &Path) {
    let _ = tokio::fs::remove_file(path).await;
    let path_text = path.to_string_lossy();
    for suffix in ["-wal", "-shm"] {
        let _ = tokio::fs::remove_file(format!("{path_text}{suffix}")).await;
    }
}
