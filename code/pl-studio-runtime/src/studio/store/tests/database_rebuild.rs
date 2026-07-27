use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};

use super::StudioStore;
use crate::studio::store_support::STUDIO_DATABASE_SCHEMA_VERSION;

#[tokio::test]
async fn unversioned_legacy_schema_is_archived_and_rebuilt() {
    let db_path = unique_test_db_path("legacy-v0-rebuild");
    remove_database_files(&db_path).await;
    let reserved_backup = PathBuf::from(format!("{}.legacy-v0.bak", db_path.display()));
    let expected_backup = PathBuf::from(format!("{}.legacy-v0.1.bak", db_path.display()));
    tokio::fs::write(&reserved_backup, b"previous backup")
        .await
        .unwrap();

    let legacy = Database::connect(sqlite_url(&db_path, "rwc"))
        .await
        .unwrap();
    legacy
        .execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "CREATE TABLE agent_outcomes (
                 id TEXT PRIMARY KEY,
                 summary TEXT
             )"
            .to_string(),
        ))
        .await
        .unwrap();
    legacy
        .execute(Statement::from_string(
            DatabaseBackend::Sqlite,
            "INSERT INTO agent_outcomes (id, summary)
             VALUES ('legacy-outcome', 'preserve me')"
                .to_string(),
        ))
        .await
        .unwrap();
    legacy.close().await.unwrap();

    let store = StudioStore::open(&db_path).await.unwrap();

    assert_eq!(
        database_schema_version(store.database()).await,
        STUDIO_DATABASE_SCHEMA_VERSION
    );
    assert!(table_exists(store.database(), "agent_framework_events").await);
    assert_eq!(
        tokio::fs::read(&reserved_backup).await.unwrap(),
        b"previous backup"
    );
    drop(store);

    let archived = Database::connect(sqlite_url(&expected_backup, "ro"))
        .await
        .unwrap();
    assert_eq!(database_schema_version(&archived).await, 0);
    let preserved_summary = archived
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT summary FROM agent_outcomes WHERE id = 'legacy-outcome'".to_string(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "summary")
        .unwrap();
    assert_eq!(preserved_summary, "preserve me");
    archived.close().await.unwrap();

    remove_database_files(&db_path).await;
    remove_database_files(&reserved_backup).await;
    remove_database_files(&expected_backup).await;
}

async fn database_schema_version(db: &DatabaseConnection) -> i64 {
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

async fn remove_database_files(path: &Path) {
    let _ = tokio::fs::remove_file(path).await;
    let path_text = path.to_string_lossy();
    for suffix in ["-wal", "-shm"] {
        let _ = tokio::fs::remove_file(format!("{path_text}{suffix}")).await;
    }
}
