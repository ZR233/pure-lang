use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use pretty_assertions::assert_eq;
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Statement};

use super::*;
use crate::SessionKind;
use crate::studio::store_support::STUDIO_DATABASE_SCHEMA_VERSION;
use pl_core::{
    AgentActivityState, AgentId, AgentIdentity, AgentLifecycleState, AgentRoleId, AgentSnapshot,
};
use pl_protocol::{
    SessionEventEnvelope, SessionEventKind, SessionEventPosition, SessionMessage,
    SessionMessageRole, SessionMessageStatus,
};

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
async fn future_schema_is_rejected_without_deleting_database() {
    let db_path = unique_test_db_path("future-schema");
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

    let error = match StudioStore::open(&db_path).await {
        Ok(_) => panic!("future schema must be rejected"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("高于当前支持版本"));

    let preserved = Database::connect(sqlite_url_for_test(&db_path))
        .await
        .unwrap();
    let legacy_table = preserved
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'legacy_only'"
                .to_string(),
        ))
        .await
        .unwrap();
    assert!(legacy_table.is_some());
    assert_eq!(schema_version(&preserved).await, 999);

    preserved.close().await.unwrap();
    remove_test_db_files(&db_path).await;
}

#[tokio::test]
async fn v2_schema_is_backed_up_and_migrated_without_losing_root_session() {
    let db_path = unique_test_db_path("schema-v2-migration");
    remove_test_db_files(&db_path).await;
    let db = Database::connect(sqlite_url_for_test(&db_path))
        .await
        .unwrap();
    execute_sql(
        &db,
        "CREATE TABLE projects (
             id TEXT PRIMARY KEY,
             name TEXT NOT NULL,
             path TEXT NOT NULL UNIQUE,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             last_opened_at INTEGER,
             closed INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE sessions (
             id TEXT PRIMARY KEY,
             project_id TEXT NOT NULL,
             title TEXT NOT NULL,
             mode TEXT NOT NULL,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             archived INTEGER NOT NULL DEFAULT 0,
             instruction_snapshot_json TEXT,
             visibility TEXT NOT NULL DEFAULT 'active',
             parent_session_id TEXT
         );
         CREATE TABLE agent_runtime_states (
             agent_id TEXT PRIMARY KEY,
             revision INTEGER NOT NULL,
             snapshot_json TEXT NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE agent_runtime_sessions (
             agent_id TEXT NOT NULL,
             session_id TEXT NOT NULL,
             metadata_json TEXT NOT NULL,
             context_json TEXT NOT NULL,
             usage_json TEXT NOT NULL,
             last_context_tokens INTEGER,
             trace_sequence INTEGER NOT NULL DEFAULT 0,
             session_event_sequence INTEGER NOT NULL DEFAULT 0,
             updated_at INTEGER NOT NULL,
             PRIMARY KEY (agent_id, session_id)
         );
         CREATE TABLE agent_turns (
             agent_id TEXT NOT NULL,
             turn_id TEXT NOT NULL,
             session_id TEXT NOT NULL,
             status TEXT NOT NULL,
             reason TEXT,
             usage_json TEXT NOT NULL,
             metadata_json TEXT,
             started_at INTEGER,
             finished_at INTEGER,
             PRIMARY KEY (agent_id, turn_id)
         );
         CREATE TABLE session_event_journal (
             session_id TEXT NOT NULL,
             sequence INTEGER NOT NULL,
             event_json TEXT NOT NULL,
             emitted_at INTEGER NOT NULL,
             PRIMARY KEY (session_id, sequence)
         );
         CREATE TABLE session_view_snapshots (
             session_id TEXT PRIMARY KEY,
             through_sequence INTEGER NOT NULL,
             snapshot_json TEXT NOT NULL,
             updated_at INTEGER NOT NULL
         );
         CREATE TABLE interactions (
             id TEXT PRIMARY KEY,
             session_id TEXT NOT NULL,
             turn_id TEXT NOT NULL,
             item_id TEXT,
             tool_id TEXT,
             agent_path TEXT,
             kind TEXT NOT NULL,
             status TEXT NOT NULL,
             payload_json TEXT NOT NULL,
             resolution_json TEXT,
             created_at INTEGER NOT NULL,
             updated_at INTEGER NOT NULL,
             resolved_at INTEGER
         );
         INSERT INTO projects (
             id, name, path, created_at, updated_at, closed
         ) VALUES (
             'project-v2', 'V2 project', 'C:/fixture', 10, 11, 0
         );
         INSERT INTO sessions (
             id, project_id, title, mode, created_at, updated_at,
             archived, visibility
         ) VALUES (
             'session-v2', 'project-v2', 'Preserved session', 'task',
             12, 13, 0, 'active'
         );
         PRAGMA user_version = 2;",
    )
    .await;
    let child_snapshot = AgentSnapshot {
        identity: AgentIdentity {
            id: AgentId::new("child-v2").unwrap(),
            parent_id: Some(AgentId::new("studio:session-v2").unwrap()),
            role: AgentRoleId::new("executor").unwrap(),
            depth: 1,
        },
        lifecycle: AgentLifecycleState::Active,
        activity: AgentActivityState::Idle,
        active_turn_id: None,
        active_session_id: None,
        pending_inputs: 0,
        last_turn: None,
        revision: 1,
        event_sequence: 1,
        updated_at: 20,
    };
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO agent_runtime_states
         (agent_id, revision, snapshot_json, updated_at) VALUES (?, ?, ?, ?)",
        [
            "child-v2".into(),
            1_i64.into(),
            serde_json::to_string(&child_snapshot).unwrap().into(),
            20_i64.into(),
        ],
    ))
    .await
    .unwrap();
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO agent_runtime_sessions (
             agent_id, session_id, metadata_json, context_json, usage_json,
             trace_sequence, session_event_sequence, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            "child-v2".into(),
            "child-session-v2".into(),
            r#"{"taskName":"V2 executor"}"#.into(),
            "[]".into(),
            "{}".into(),
            0_i64.into(),
            1_i64.into(),
            20_i64.into(),
        ],
    ))
    .await
    .unwrap();
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO agent_turns (
             agent_id, turn_id, session_id, status, usage_json, started_at, finished_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?)",
        [
            "child-v2".into(),
            "turn-child-v2".into(),
            "child-session-v2".into(),
            "completed".into(),
            "{}".into(),
            20_i64.into(),
            21_i64.into(),
        ],
    ))
    .await
    .unwrap();
    let planner_event = message_event(
        "root-event-v2",
        "session-v2",
        "studio:session-v2",
        "turn-root-v2",
        1,
        "root-message-v2",
    );
    let child_event = message_event(
        "child-event-v2",
        "session-v2",
        "child-v2",
        "turn-child-v2",
        2,
        "child-message-v2",
    );
    for event in [planner_event, child_event] {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            "INSERT INTO session_event_journal
             (session_id, sequence, event_json, emitted_at) VALUES (?, ?, ?, ?)",
            [
                "session-v2".into(),
                i64::try_from(event.position.durable_sequence().unwrap())
                    .unwrap()
                    .into(),
                serde_json::to_string(&event).unwrap().into(),
                event.emitted_at.into(),
            ],
        ))
        .await
        .unwrap();
    }
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "INSERT INTO interactions (
             id, session_id, turn_id, kind, status, payload_json, created_at, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        [
            "interaction-child-v2".into(),
            "session-v2".into(),
            "turn-child-v2".into(),
            "userInput".into(),
            "pending".into(),
            "{}".into(),
            20_i64.into(),
            20_i64.into(),
        ],
    ))
    .await
    .unwrap();
    db.close().await.unwrap();

    let store = StudioStore::open(&db_path).await.unwrap();
    let session = store.read_session("session-v2").await.unwrap().unwrap();
    assert_eq!(session.root_session_id, "session-v2");
    assert_eq!(session.owner_agent_id, "studio:session-v2");
    assert_eq!(session.owner_role, "planner");
    assert_eq!(session.session_kind, SessionKind::Root);
    let child = store
        .read_session("child-session-v2")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(child.root_session_id, "session-v2");
    assert_eq!(child.parent_session_id.as_deref(), Some("session-v2"));
    assert_eq!(child.owner_agent_id, "child-v2");
    assert_eq!(child.owner_role, "executor");
    let root_journal = journal_events(&store.db, "session-v2").await;
    assert_eq!(root_journal.len(), 1);
    assert_eq!(
        root_journal[0].source_agent_id.as_deref(),
        Some("studio:session-v2")
    );
    let child_journal = journal_events(&store.db, "child-session-v2").await;
    assert_eq!(child_journal.len(), 1);
    assert_eq!(child_journal[0].session_id, "child-session-v2");
    assert_eq!(
        child_journal[0].source_agent_id.as_deref(),
        Some("child-v2")
    );
    let child_message = match &child_journal[0].kind {
        SessionEventKind::MessageChanged { message } => message,
        other => panic!("expected migrated child message, got {other:?}"),
    };
    assert_eq!(child_message.session_id, "child-session-v2");
    let interaction_session = store
        .db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT session_id FROM interactions WHERE id = 'interaction-child-v2'".to_string(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "session_id")
        .unwrap();
    assert_eq!(interaction_session, "child-session-v2");
    assert_eq!(
        schema_version(&store.db).await,
        STUDIO_DATABASE_SCHEMA_VERSION
    );
    assert!(
        PathBuf::from(format!("{}.v2.bak", db_path.display())).is_file(),
        "v2 migration must preserve a recoverable backup"
    );

    drop(store);
    remove_test_db_files(&db_path).await;
    let _ = tokio::fs::remove_file(format!("{}.v2.bak", db_path.display())).await;
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

fn message_event(
    event_id: &str,
    session_id: &str,
    source_agent_id: &str,
    turn_id: &str,
    sequence: u64,
    message_id: &str,
) -> SessionEventEnvelope {
    SessionEventEnvelope {
        event_id: event_id.to_string(),
        session_id: session_id.to_string(),
        source_agent_id: Some(source_agent_id.to_string()),
        turn_id: Some(turn_id.to_string()),
        emitted_at: i64::try_from(sequence).unwrap() + 20,
        position: SessionEventPosition::Durable { sequence },
        kind: SessionEventKind::MessageChanged {
            message: Box::new(SessionMessage {
                message_id: message_id.to_string(),
                session_id: session_id.to_string(),
                turn_id: turn_id.to_string(),
                role: SessionMessageRole::Assistant,
                status: SessionMessageStatus::Completed,
                created_at: 20,
                updated_at: 21,
                completed_at: Some(21),
                error: None,
                metadata: serde_json::json!({}),
            }),
        },
    }
}

async fn journal_events(db: &DatabaseConnection, session_id: &str) -> Vec<SessionEventEnvelope> {
    db.query_all(Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "SELECT event_json FROM session_event_journal
         WHERE session_id = ? ORDER BY sequence",
        [session_id.into()],
    ))
    .await
    .unwrap()
    .into_iter()
    .map(|row| {
        serde_json::from_str(
            &row.try_get::<String>("", "event_json")
                .expect("journal event json"),
        )
        .expect("valid journal event")
    })
    .collect()
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
