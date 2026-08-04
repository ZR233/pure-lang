use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};

use super::*;
use crate::studio::store_support::{STUDIO_DATABASE_SCHEMA_VERSION, initialize_studio_schema};

#[tokio::test]
async fn creates_one_studio_database_with_thread_schema_v1() {
    let root = unique_test_root("single-database");
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
        "interactions",
        "attachments",
        "app_settings",
        "task_runs",
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
    for removed in [
        "sessions",
        "agent_runtime_states",
        "agent_runtime_sessions",
        "agent_pending_inputs",
        "agent_active_inputs",
        "agent_turns",
        "storage_metadata",
        "history_gc_jobs",
        "agent_framework_events",
        "agent_runtime_traces",
        "session_event_journal",
        "tool_approvals",
    ] {
        assert!(!table_exists(store.database(), removed).await);
    }
    assert!(tokio::fs::try_exists(&database_path).await.unwrap());
    assert!(
        !tokio::fs::try_exists(root.join("studio.history.sqlite"))
            .await
            .unwrap()
    );

    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn file_database_serializes_concurrent_writers_on_one_connection() {
    let root = unique_test_root("single-writer");
    let database_path = root.join("studio.sqlite");
    let store = StudioStore::open(&database_path).await.unwrap();
    assert_eq!(
        store
            .database()
            .get_sqlite_connection_pool()
            .options()
            .get_max_connections(),
        1
    );

    let mut writes = tokio::task::JoinSet::new();
    for index in 0..16 {
        let store = store.clone();
        writes.spawn(async move {
            store
                .upsert_project(format!("C:/work/single-writer-{index}"))
                .await
        });
    }
    while let Some(result) = writes.join_next().await {
        result.unwrap().unwrap();
    }
    assert_eq!(store.list_projects().await.unwrap().len(), 16);

    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn canonical_database_version_in_wal_survives_reopen() {
    let root = unique_test_root("canonical-wal-reopen");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let database_path = root.join("studio.sqlite");
    let database = Database::connect(sqlite_url(&database_path, "rwc"))
        .await
        .unwrap();
    execute_sql(
        &database,
        "PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;",
    )
    .await;
    initialize_studio_schema(&database).await.unwrap();

    let header = tokio::fs::read(&database_path).await.unwrap();
    assert_eq!(u32::from_be_bytes(header[60..64].try_into().unwrap()), 0);
    assert_eq!(
        schema_version(&database).await,
        STUDIO_DATABASE_SCHEMA_VERSION
    );

    let reopened = StudioStore::open(&database_path).await.unwrap();
    assert_eq!(
        schema_version(reopened.database()).await,
        STUDIO_DATABASE_SCHEMA_VERSION
    );
    assert!(!tokio::fs::try_exists(root.join("archive")).await.unwrap());

    drop(reopened);
    database.close().await.unwrap();
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn project_archive_preserves_canonical_thread_and_attachment_rows() {
    let store = StudioStore::open_memory().await.unwrap();
    let project = store
        .upsert_project("C:/work/archive-logical")
        .await
        .unwrap();
    let thread = store
        .create_thread(&project.id, "Archive logical", StudioMode::Simple)
        .await
        .unwrap();
    store
        .database()
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO attachments (
                id, thread_id, item_id, media_type, filename, storage_path,
                byte_size, width, height, created_at
             ) VALUES (?, ?, NULL, ?, NULL, ?, 1, NULL, NULL, 1)",
            [
                "attachment-logical".into(),
                thread.id.clone().into(),
                "image/png".into(),
                "C:/attachments/attachment-logical.png".into(),
            ],
        ))
        .await
        .unwrap();

    let archived = store.archive_project(&project.id).await.unwrap().unwrap();

    assert_eq!(archived.id, project.id);
    assert!(store.list_projects().await.unwrap().is_empty());
    assert_eq!(
        store
            .read_thread(&thread.id)
            .await
            .unwrap()
            .unwrap()
            .visibility,
        crate::studio::records::ThreadVisibility::Archived
    );
    let attachment_count = store
        .database()
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT COUNT(*) AS count FROM attachments".to_string(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "count")
        .unwrap();
    assert_eq!(attachment_count, 1);
}

#[tokio::test]
async fn legacy_database_and_sidecars_are_archived_without_importing_rows() {
    let root = unique_test_root("legacy-archive");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let legacy_path = root.join("studio_state.sqlite");
    let attachments = root.join("attachments");
    tokio::fs::create_dir_all(&attachments).await.unwrap();
    tokio::fs::write(attachments.join("proof.txt"), b"attachment")
        .await
        .unwrap();
    let legacy = Database::connect(sqlite_url(&legacy_path, "rwc"))
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
    tokio::fs::write(sidecar_path(&legacy_path, "-wal"), b"legacy-wal")
        .await
        .unwrap();
    tokio::fs::write(sidecar_path(&legacy_path, "-shm"), b"legacy-shm")
        .await
        .unwrap();

    let store = StudioStore::open(&legacy_path).await.unwrap();
    assert_eq!(
        schema_version(store.database()).await,
        STUDIO_DATABASE_SCHEMA_VERSION
    );
    assert!(!table_exists(store.database(), "legacy_only").await);

    let archive = only_archive(&root).await;
    let archived_db = archive.join("studio_state.sqlite");
    assert!(
        tokio::fs::try_exists(archive.join("manifest.json"))
            .await
            .unwrap()
    );
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
    assert!(
        tokio::fs::try_exists(archive.join("attachments/proof.txt"))
            .await
            .unwrap()
    );
    let archived = Database::connect(sqlite_url(&archived_db, "ro"))
        .await
        .unwrap();
    assert_eq!(schema_version(&archived).await, 10);
    assert!(table_exists(&archived, "legacy_only").await);
    archived.close().await.unwrap();

    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn orphaned_legacy_sidecars_and_attachments_are_archived_together() {
    let root = unique_test_root("orphaned-legacy-sidecars");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let canonical_path = root.join("studio.sqlite");
    let legacy_path = root.join("studio_state.sqlite");
    let legacy_wal = sidecar_path(&legacy_path, "-wal");
    let legacy_shm = sidecar_path(&legacy_path, "-shm");
    let attachments = root.join("attachments");
    tokio::fs::write(&legacy_wal, b"orphaned-wal")
        .await
        .unwrap();
    tokio::fs::write(&legacy_shm, b"orphaned-shm")
        .await
        .unwrap();
    tokio::fs::create_dir_all(&attachments).await.unwrap();
    tokio::fs::write(attachments.join("proof.txt"), b"attachment")
        .await
        .unwrap();

    let store = StudioStore::open_database(
        &canonical_path,
        vec![legacy_path],
        Some(attachments.clone()),
    )
    .await
    .unwrap();

    let archive = only_archive(&root).await;
    assert!(tokio::fs::try_exists(&canonical_path).await.unwrap());
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
    assert!(
        tokio::fs::try_exists(archive.join("attachments/proof.txt"))
            .await
            .unwrap()
    );
    let manifest: serde_json::Value = serde_json::from_slice(
        &tokio::fs::read(archive.join("manifest.json"))
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(manifest["databases"].as_array().unwrap().is_empty());
    assert!(!tokio::fs::try_exists(&legacy_wal).await.unwrap());
    assert!(!tokio::fs::try_exists(&legacy_shm).await.unwrap());
    assert!(!tokio::fs::try_exists(&attachments).await.unwrap());

    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn legacy_manifest_records_all_databases_attachments_and_git_resources() {
    let root = unique_test_root("legacy-manifest");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let repository_path = root.join("legacy-task-repository");
    let (branch, base_commit, actual_head) = create_git_repository(&repository_path).await;
    tokio::fs::write(repository_path.join("dirty.txt"), b"not committed")
        .await
        .unwrap();

    let state_path = root.join("studio_state.sqlite");
    let history_path = root.join("studio_history.sqlite");
    let legacy_path = root.join("studio_2.sqlite");
    let canonical_path = root.join("studio.sqlite");
    let attachments_path = root.join("attachments");
    tokio::fs::create_dir_all(&attachments_path).await.unwrap();
    tokio::fs::write(attachments_path.join("proof.txt"), b"attachment")
        .await
        .unwrap();

    let workspace_root = sql_text(&repository_path.to_string_lossy());
    let git_common_dir = sql_text(&repository_path.join(".git").to_string_lossy());
    let branch_sql = sql_text(&branch);
    let base_sql = sql_text(&base_commit);
    let state = Database::connect(sqlite_url(&state_path, "rwc"))
        .await
        .unwrap();
    execute_sql(
        &state,
        format!(
            "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, path TEXT NOT NULL, closed INTEGER NOT NULL);
             CREATE TABLE task_runs (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, phase TEXT NOT NULL, workspace_root TEXT NOT NULL, git_common_dir TEXT NOT NULL, branch TEXT NOT NULL, base_commit TEXT NOT NULL, expected_head TEXT NOT NULL);
             CREATE TABLE work_units (id TEXT PRIMARY KEY, task_run_id TEXT NOT NULL, status TEXT NOT NULL, worktree_path TEXT NOT NULL, branch TEXT NOT NULL, base_commit TEXT NOT NULL, worktree_disposition TEXT NOT NULL);
             CREATE TABLE branch_leases (id TEXT PRIMARY KEY, task_run_id TEXT NOT NULL, git_common_dir TEXT NOT NULL, branch TEXT NOT NULL, expected_head TEXT NOT NULL);
             INSERT INTO projects VALUES ('project-old', 'Old project', '{workspace_root}', 0);
             INSERT INTO task_runs VALUES ('task-old', 'session-old', 'executing', '{workspace_root}', '{git_common_dir}', '{branch_sql}', '{base_sql}', '{base_sql}');
             INSERT INTO work_units VALUES ('work-old', 'task-old', 'running', '{workspace_root}', '{branch_sql}', '{base_sql}', 'protect');
             INSERT INTO branch_leases VALUES ('lease-old', 'task-old', '{git_common_dir}', '{branch_sql}', '{base_sql}');
             PRAGMA user_version = 11;"
        ),
    )
    .await;
    state.close().await.unwrap();
    create_legacy_database(&history_path, "history_only", 1).await;
    create_legacy_database(&legacy_path, "studio_two_only", 10).await;
    for database_path in [&state_path, &history_path, &legacy_path] {
        tokio::fs::write(sidecar_path(database_path, "-wal"), b"legacy-wal")
            .await
            .unwrap();
        tokio::fs::write(sidecar_path(database_path, "-shm"), b"legacy-shm")
            .await
            .unwrap();
    }

    let store = StudioStore::open_database(
        &canonical_path,
        vec![
            state_path.clone(),
            history_path.clone(),
            legacy_path.clone(),
        ],
        Some(attachments_path.clone()),
    )
    .await
    .unwrap();
    let archive = only_archive(&root).await;
    for file_name in [
        "studio_state.sqlite",
        "studio_state.sqlite-wal",
        "studio_state.sqlite-shm",
        "studio_history.sqlite",
        "studio_history.sqlite-wal",
        "studio_history.sqlite-shm",
        "studio_2.sqlite",
        "studio_2.sqlite-wal",
        "studio_2.sqlite-shm",
    ] {
        assert!(
            tokio::fs::try_exists(archive.join(file_name))
                .await
                .unwrap(),
            "missing archived {file_name}"
        );
    }
    assert!(
        tokio::fs::try_exists(archive.join("attachments/proof.txt"))
            .await
            .unwrap()
    );

    let manifest: serde_json::Value = serde_json::from_slice(
        &tokio::fs::read(archive.join("manifest.json"))
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["databases"].as_array().unwrap().len(), 3);
    assert_eq!(
        manifest["legacyRecords"]["projects"][0]["id"],
        "project-old"
    );
    assert_eq!(
        manifest["legacyRecords"]["taskRuns"][0]["rootThreadId"],
        "session-old"
    );
    assert_eq!(
        manifest["legacyRecords"]["workUnits"][0]["worktreeDisposition"],
        "protect"
    );
    let workspace = manifest["externalResources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|resource| resource["kind"] == "taskWorkspace")
        .unwrap();
    assert_eq!(workspace["actualBranch"], branch);
    assert_eq!(workspace["actualHead"], actual_head);
    assert_eq!(workspace["expectedHead"], base_commit);
    assert_eq!(workspace["matchesExpectedHead"], false);
    assert_eq!(workspace["dirty"], true);
    assert_eq!(workspace["aheadBy"], 1);
    assert!(workspace["probeError"].is_null());

    assert!(tokio::fs::try_exists(&repository_path).await.unwrap());
    assert_eq!(
        git_output(&repository_path, &["rev-parse", "HEAD"]),
        actual_head
    );
    assert_eq!(
        git_output(
            &repository_path,
            &["rev-parse", "--verify", &format!("refs/heads/{branch}")]
        ),
        actual_head
    );

    drop(store);
    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn archive_move_conflict_and_midway_failure_preserve_sources() {
    let root = unique_test_root("legacy-archive-rollback");
    let conflict_archive = root.join("conflict-archive");
    tokio::fs::create_dir_all(&conflict_archive).await.unwrap();
    let conflict_source = root.join("conflict-source.sqlite");
    let conflict_destination = conflict_archive.join("conflict-source.sqlite");
    let conflict_manifest = conflict_archive.join("manifest.json");
    tokio::fs::write(&conflict_source, b"source").await.unwrap();
    tokio::fs::write(&conflict_destination, b"existing")
        .await
        .unwrap();
    tokio::fs::write(&conflict_manifest, b"manifest")
        .await
        .unwrap();
    let conflict = super::super::project::move_archive_entries(
        vec![(conflict_source.clone(), conflict_destination.clone())],
        &conflict_manifest,
        &conflict_archive,
    )
    .await;
    assert!(conflict.is_err());
    assert_eq!(tokio::fs::read(&conflict_source).await.unwrap(), b"source");
    assert_eq!(
        tokio::fs::read(&conflict_destination).await.unwrap(),
        b"existing"
    );

    let rollback_archive = root.join("rollback-archive");
    tokio::fs::create_dir_all(&rollback_archive).await.unwrap();
    let first_source = root.join("first.sqlite");
    let missing_source = root.join("missing.sqlite");
    let rollback_manifest = rollback_archive.join("manifest.json");
    tokio::fs::write(&first_source, b"first").await.unwrap();
    tokio::fs::write(&rollback_manifest, b"manifest")
        .await
        .unwrap();
    let rollback = super::super::project::move_archive_entries(
        vec![
            (first_source.clone(), rollback_archive.join("first.sqlite")),
            (missing_source, rollback_archive.join("missing.sqlite")),
        ],
        &rollback_manifest,
        &rollback_archive,
    )
    .await;
    assert!(rollback.is_err());
    assert_eq!(tokio::fs::read(&first_source).await.unwrap(), b"first");
    assert!(!tokio::fs::try_exists(&rollback_manifest).await.unwrap());
    assert!(!tokio::fs::try_exists(&rollback_archive).await.unwrap());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn incomplete_legacy_product_table_fails_closed() {
    let root = unique_test_root("legacy-incomplete");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let legacy_path = root.join("studio_2.sqlite");
    let legacy = Database::connect(sqlite_url(&legacy_path, "rwc"))
        .await
        .unwrap();
    execute_sql(
        &legacy,
        "CREATE TABLE projects (id TEXT PRIMARY KEY); PRAGMA user_version = 10;",
    )
    .await;
    legacy.close().await.unwrap();

    let result = StudioStore::open(&legacy_path).await;
    assert!(result.is_err());
    assert!(tokio::fs::try_exists(&legacy_path).await.unwrap());
    assert!(!tokio::fs::try_exists(root.join("archive")).await.unwrap());

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn future_canonical_schema_is_rejected_and_preserved() {
    let root = unique_test_root("future-schema");
    tokio::fs::create_dir_all(&root).await.unwrap();
    let database_path = root.join("studio.sqlite");
    let database = Database::connect(sqlite_url(&database_path, "rwc"))
        .await
        .unwrap();
    execute_sql(
        &database,
        "CREATE TABLE future_only (id TEXT PRIMARY KEY);
         INSERT INTO future_only (id) VALUES ('must-remain');
         PRAGMA user_version = 999;",
    )
    .await;
    database.close().await.unwrap();

    let error = match StudioStore::open(&database_path).await {
        Ok(_) => panic!("future schema must be rejected"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("高于当前支持版本"));
    let preserved = Database::connect(sqlite_url(&database_path, "ro"))
        .await
        .unwrap();
    assert_eq!(schema_version(&preserved).await, 999);
    assert!(table_exists(&preserved, "future_only").await);
    preserved.close().await.unwrap();

    let _ = tokio::fs::remove_dir_all(root).await;
}

async fn only_archive(root: &Path) -> PathBuf {
    let mut entries = tokio::fs::read_dir(root.join("archive")).await.unwrap();
    let archive = entries.next_entry().await.unwrap().unwrap().path();
    assert!(entries.next_entry().await.unwrap().is_none());
    archive
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

async fn execute_sql(db: &DatabaseConnection, sql: impl Into<String>) {
    db.execute_raw(Statement::from_string(DatabaseBackend::Sqlite, sql.into()))
        .await
        .unwrap();
}

async fn create_legacy_database(path: &Path, table: &str, version: i64) {
    let database = Database::connect(sqlite_url(path, "rwc")).await.unwrap();
    execute_sql(
        &database,
        format!(
            "CREATE TABLE {table} (id TEXT PRIMARY KEY); INSERT INTO {table} VALUES ('preserved'); PRAGMA user_version = {version};"
        ),
    )
    .await;
    database.close().await.unwrap();
}

async fn create_git_repository(path: &Path) -> (String, String, String) {
    tokio::fs::create_dir_all(path).await.unwrap();
    run_git(path, &["init"]);
    run_git(path, &["config", "user.name", "Pure Studio Test"]);
    run_git(
        path,
        &["config", "user.email", "pure-studio@example.invalid"],
    );
    run_git(path, &["checkout", "-b", "codex/legacy-manifest"]);
    tokio::fs::write(path.join("base.txt"), b"base")
        .await
        .unwrap();
    run_git(path, &["add", "base.txt"]);
    run_git(path, &["commit", "-m", "test: base"]);
    let base_commit = git_output(path, &["rev-parse", "HEAD"]);
    tokio::fs::write(path.join("ahead.txt"), b"ahead")
        .await
        .unwrap();
    run_git(path, &["add", "ahead.txt"]);
    run_git(path, &["commit", "-m", "test: ahead"]);
    let actual_head = git_output(path, &["rev-parse", "HEAD"]);
    let branch = git_output(path, &["branch", "--show-current"]);
    (branch, base_commit, actual_head)
}

fn run_git(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output(path: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn sql_text(value: &str) -> String {
    value.replace('\'', "''")
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

fn sqlite_url(path: &Path, mode: &str) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    format!("sqlite://{path}?mode={mode}")
}
