use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};

use super::*;
use crate::studio::paths::{sqlite_read_only_url, sqlite_url};
use crate::studio::store_support::STUDIO_DATABASE_SCHEMA_VERSION;

#[tokio::test]
async fn creates_canonical_schema_v5_with_typed_task_failures() {
    let root = unique_test_root("schema-v5");
    let database_path = root.join("studio.sqlite");
    let store = StudioStore::open(&database_path).await.unwrap();

    assert_eq!(
        schema_version(store.database()).await,
        STUDIO_DATABASE_SCHEMA_VERSION
    );
    for table in [
        "projects",
        "threads",
        "thread_inputs",
        "turns",
        "items",
        "thread_context_segments",
        "thread_session_state",
        "interactions",
        "attachments",
        "app_settings",
        "task_runs",
        "task_failures",
        "work_units",
        "work_completions",
        "review_rounds",
        "merge_records",
        "branch_leases",
    ] {
        assert!(
            table_exists(store.database(), table).await,
            "missing {table}"
        );
    }
    let work_unit_columns = table_columns(store.database(), "work_units").await;
    assert!(work_unit_columns.contains(&"scope_hints_json".to_string()));
    assert!(!work_unit_columns.contains(&"owned_paths_json".to_string()));
    let item_columns = table_columns(store.database(), "items").await;
    assert!(!item_columns.contains(&"provider_private_payload".to_string()));
    let task_run_columns = table_columns(store.database(), "task_runs").await;
    assert!(task_run_columns.contains(&"terminal_failure_id".to_string()));
    let task_failure_columns = table_columns(store.database(), "task_failures").await;
    for column in [
        "task_run_id",
        "source_thread_id",
        "source_turn_id",
        "source_agent_id",
        "source_role",
        "work_unit_id",
        "review_round_id",
        "disposition",
        "failure_json",
        "resolved_at",
    ] {
        assert!(
            task_failure_columns.contains(&column.to_string()),
            "missing task_failures.{column}"
        );
    }
    let turn_columns = table_columns(store.database(), "turns").await;
    for column in [
        "budget_limit_json",
        "rollover_compacted",
        "rollover_compaction_error",
    ] {
        assert!(
            turn_columns.contains(&column.to_string()),
            "missing turns.{column}"
        );
    }
    let segment_columns = table_columns(store.database(), "thread_context_segments").await;
    for column in [
        "thread_id",
        "ordinal",
        "revision",
        "kind",
        "payload_json",
        "payload_hash",
        "resulting_hash",
    ] {
        assert!(
            segment_columns.contains(&column.to_string()),
            "missing thread_context_segments.{column}"
        );
    }
    let merge_columns = table_columns(store.database(), "merge_records").await;
    for column in [
        "work_unit_id",
        "completion_id",
        "completion_revision",
        "executor_agent_id",
        "expected_previous_head",
        "resulting_head",
        "delivery_head",
        "method",
        "summary",
        "cleanup_status",
        "cleanup_detail",
    ] {
        assert!(
            merge_columns.contains(&column.to_string()),
            "missing merge_records.{column}"
        );
    }
    for legacy_column in [
        "status",
        "conflict_files_json",
        "resolution_summary",
        "verification_json",
        "attempt",
    ] {
        assert!(!merge_columns.contains(&legacy_column.to_string()));
    }

    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn migrates_schema_v4_waiting_interaction_turns_without_rebuild() {
    let root = unique_test_root("schema-v4-turn-migration");
    let database_path = root.join("studio.sqlite");
    let store = StudioStore::open(&database_path).await.unwrap();
    store
        .database()
        .execute_unprepared(
            "INSERT INTO projects \
             (id, name, path, created_at, updated_at, last_opened_at, closed) \
             VALUES ('project-1', 'Project', 'C:/project', 1, 1, 1, 0); \
             INSERT INTO threads \
             (id, project_id, title, mode, root_thread_id, parent_thread_id, role, agent_path, \
              status, revision, runtime_revision, event_sequence, metadata_json, usage_json, \
              last_context_tokens, trace_sequence, created_at, updated_at, archived) \
             VALUES ('thread-1', 'project-1', 'Thread', 'chat', 'thread-1', NULL, 'main', \
                     'thread-1', 'running', 1, 1, 1, '{}', '{}', NULL, 0, 1, 7, 0); \
             INSERT INTO turns \
             (id, thread_id, ordinal, revision, status, phase, reason, model_json, usage_json, \
              failure_json, budget_limit_json, rollover_compacted, rollover_compaction_error, \
              metadata_json, started_at, updated_at, completed_at) \
             VALUES ('turn-1', 'thread-1', 1, 1, 'inProgress', 'waitingInteraction', NULL, \
                     NULL, '{}', NULL, NULL, 0, NULL, NULL, 2, 7, NULL);",
        )
        .await
        .unwrap();
    drop(store);
    create_database(&database_path, "PRAGMA user_version = 4;").await;

    let migrated = StudioStore::open(&database_path).await.unwrap();
    assert_eq!(
        schema_version(migrated.database()).await,
        STUDIO_DATABASE_SCHEMA_VERSION
    );
    let turn = migrated
        .database()
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT status, phase, completed_at FROM turns WHERE id = 'turn-1'".to_string(),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(turn.try_get::<String>("", "status").unwrap(), "completed");
    assert_eq!(turn.try_get::<Option<String>>("", "phase").unwrap(), None);
    assert_eq!(
        turn.try_get::<Option<i64>>("", "completed_at").unwrap(),
        Some(7)
    );
    assert_eq!(migrated.list_projects().await.unwrap().len(), 1);

    drop(migrated);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn incompatible_version_is_deleted_and_rebuilt_without_archive_or_import() {
    let root = unique_test_root("version-rebuild");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let database_path = root.join("studio.sqlite");
    create_database(
        &database_path,
        "CREATE TABLE legacy_only (id TEXT PRIMARY KEY);
         INSERT INTO legacy_only VALUES ('must-disappear');
         PRAGMA user_version = 3;",
    )
    .await;
    tokio::fs::write(sidecar_path(&database_path, "-wal"), b"old-wal")
        .await
        .unwrap();
    tokio::fs::write(sidecar_path(&database_path, "-shm"), b"old-shm")
        .await
        .unwrap();

    let store = StudioStore::open(&database_path).await.unwrap();

    assert_eq!(
        schema_version(store.database()).await,
        STUDIO_DATABASE_SCHEMA_VERSION
    );
    assert!(!table_exists(store.database(), "legacy_only").await);
    assert!(!tokio::fs::try_exists(root.join("archive")).await.unwrap());
    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn matching_version_with_wrong_fingerprint_is_rebuilt() {
    let root = unique_test_root("fingerprint-rebuild");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let database_path = root.join("studio.sqlite");
    create_database(
        &database_path,
        format!(
            "CREATE TABLE projects (id TEXT PRIMARY KEY);
             INSERT INTO projects VALUES ('legacy');
             PRAGMA user_version = {STUDIO_DATABASE_SCHEMA_VERSION};"
        ),
    )
    .await;

    let store = StudioStore::open(&database_path).await.unwrap();

    assert!(table_exists(store.database(), "threads").await);
    assert_eq!(store.list_projects().await.unwrap().len(), 0);
    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn corrupt_database_is_rebuilt_to_an_empty_schema() {
    let root = unique_test_root("corrupt-rebuild");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let database_path = root.join("studio.sqlite");
    tokio::fs::write(&database_path, b"not a sqlite database")
        .await
        .unwrap();

    let store = StudioStore::open(&database_path).await.unwrap();

    assert_eq!(
        schema_version(store.database()).await,
        STUDIO_DATABASE_SCHEMA_VERSION
    );
    assert!(store.list_projects().await.unwrap().is_empty());
    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn rebuild_does_not_touch_legacy_files_attachments_or_project_resources() {
    let root = unique_test_root("resource-preservation");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let database_path = root.join("studio.sqlite");
    create_database(&database_path, "PRAGMA user_version = 1;").await;
    let legacy_database = root.join("studio_2.sqlite");
    let attachments = root.join("attachments");
    let project = root.join("project");
    let worktree = project.join(".pure/worktrees/task/executor");
    tokio::fs::write(&legacy_database, b"legacy-database-sentinel")
        .await
        .unwrap();
    tokio::fs::create_dir_all(&attachments).await.unwrap();
    tokio::fs::write(attachments.join("attachment.txt"), b"attachment")
        .await
        .unwrap();
    tokio::fs::create_dir_all(&worktree).await.unwrap();
    tokio::fs::write(worktree.join("delivery.txt"), b"delivery")
        .await
        .unwrap();

    let store = StudioStore::open(&database_path).await.unwrap();

    assert_eq!(
        tokio::fs::read(&legacy_database).await.unwrap(),
        b"legacy-database-sentinel"
    );
    assert_eq!(
        tokio::fs::read(attachments.join("attachment.txt"))
            .await
            .unwrap(),
        b"attachment"
    );
    assert_eq!(
        tokio::fs::read(worktree.join("delivery.txt"))
            .await
            .unwrap(),
        b"delivery"
    );
    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn unsafe_sidecar_target_fails_before_deleting_the_database() {
    let root = unique_test_root("unsafe-sidecar");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let database_path = root.join("studio.sqlite");
    create_database(
        &database_path,
        "CREATE TABLE legacy_only (id TEXT PRIMARY KEY); PRAGMA user_version = 1;",
    )
    .await;
    tokio::fs::create_dir_all(sidecar_path(&database_path, "-wal"))
        .await
        .unwrap();

    let result = StudioStore::open(&database_path).await;

    assert!(result.is_err());
    let preserved = Database::connect(sqlite_read_only_url(&database_path))
        .await
        .unwrap();
    assert!(table_exists(&preserved, "legacy_only").await);
    preserved.close().await.unwrap();
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[cfg(windows)]
#[tokio::test]
async fn locked_database_family_member_never_produces_a_half_initialized_schema() {
    use std::os::windows::fs::OpenOptionsExt;

    for suffix in ["", "-wal", "-shm"] {
        let root = unique_test_root(&format!("locked-{}", suffix.trim_start_matches('-')));
        tokio::fs::create_dir_all(&root).await.unwrap();
        let database_path = root.join("studio.sqlite");
        create_database(
            &database_path,
            "CREATE TABLE legacy_only (id TEXT PRIMARY KEY); PRAGMA user_version = 1;",
        )
        .await;
        let locked_path = if suffix.is_empty() {
            database_path.clone()
        } else {
            let sidecar = sidecar_path(&database_path, suffix);
            tokio::fs::write(&sidecar, b"locked-sidecar").await.unwrap();
            sidecar
        };
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&locked_path)
            .unwrap();

        let result = StudioStore::open(&database_path).await;

        assert!(result.is_err(), "locked {suffix} must fail rebuild");
        if tokio::fs::try_exists(&database_path).await.unwrap() {
            let probe = Database::connect(sqlite_read_only_url(&database_path)).await;
            if let Ok(probe) = probe {
                assert_ne!(schema_version(&probe).await, STUDIO_DATABASE_SCHEMA_VERSION);
                probe.close().await.unwrap();
            }
        }
        drop(lock);
        let _ = tokio::fs::remove_dir_all(root).await;
    }
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

async fn table_columns(db: &DatabaseConnection, table: &str) -> Vec<String> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Sqlite,
        format!("PRAGMA table_info(\"{table}\")"),
    ))
    .await
    .unwrap()
    .into_iter()
    .map(|row| row.try_get("", "name").unwrap())
    .collect()
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

async fn create_database(path: &Path, sql: impl Into<String>) {
    let database = Database::connect(sqlite_url(path)).await.unwrap();
    database
        .execute_raw(Statement::from_string(DatabaseBackend::Sqlite, sql.into()))
        .await
        .unwrap();
    database.close().await.unwrap();
}

fn unique_test_root(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pure-studio-{name}-{}-{stamp}", std::process::id()))
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", path.display()))
}
