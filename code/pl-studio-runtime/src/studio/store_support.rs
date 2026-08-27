use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use sea_orm::sea_query::{Index, IndexCreateStatement, IndexOrder};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, Schema, Statement, TransactionTrait,
};
use sha2::{Digest, Sha256};

use crate::studio::entity;

/// 唯一支持的 Studio schema 版本；仅 v13 附件表有一次性迁移。
pub(super) const STUDIO_DATABASE_SCHEMA_VERSION: i64 = 14;

pub(super) async fn migrate_studio_schema_v13_to_v14(
    db: &DatabaseConnection,
    attachments_dir: &Path,
) -> Result<()> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT id, storage_path, byte_size FROM attachments ORDER BY id".to_string(),
        ))
        .await?;
    tokio::fs::create_dir_all(attachments_dir).await?;
    let canonical_attachments_dir = std::fs::canonicalize(attachments_dir)
        .context("failed to resolve Studio attachments directory for v13 migration")?;
    let mut migrations = Vec::with_capacity(rows.len());
    let mut created_paths = Vec::new();

    for row in rows {
        let id: String = row.try_get("", "id")?;
        let storage_path: String = row.try_get("", "storage_path")?;
        let expected_size: i64 = row.try_get("", "byte_size")?;
        ensure!(
            expected_size >= 0,
            "attachment {id} has a negative byte size"
        );
        let old_path = PathBuf::from(storage_path);
        let metadata = std::fs::symlink_metadata(&old_path)
            .with_context(|| format!("attachment {id} snapshot is unavailable"))?;
        ensure!(
            metadata.is_file() && !pl_core::path_safety::is_link_or_reparse(&metadata),
            "attachment {id} snapshot is not a regular file"
        );
        let canonical_old_path = std::fs::canonicalize(&old_path)
            .with_context(|| format!("attachment {id} snapshot cannot be resolved"))?;
        ensure!(
            canonical_old_path.starts_with(&canonical_attachments_dir),
            "attachment {id} snapshot escapes the attachments directory"
        );
        let bytes = tokio::fs::read(&canonical_old_path)
            .await
            .with_context(|| format!("attachment {id} snapshot cannot be read"))?;
        ensure!(
            bytes.len() == usize::try_from(expected_size)?,
            "attachment {id} snapshot size does not match the database"
        );
        let content_sha256 = format!("{:x}", Sha256::digest(&bytes));
        let object_dir = attachments_dir.join("objects").join(&content_sha256[..2]);
        tokio::fs::create_dir_all(&object_dir).await?;
        let object_path = object_dir.join(&content_sha256);
        if tokio::fs::try_exists(&object_path).await? {
            let existing = tokio::fs::read(&object_path).await?;
            ensure!(
                Sha256::digest(&existing).as_slice() == Sha256::digest(&bytes).as_slice(),
                "attachment {id} content-addressed target is inconsistent"
            );
        } else {
            if let Err(error) = tokio::fs::write(&object_path, &bytes).await {
                cleanup_migration_blobs(&created_paths).await;
                return Err(error).context("failed to create migrated attachment snapshot");
            }
            created_paths.push(object_path.clone());
        }
        migrations.push((id, object_path, content_sha256));
    }

    let migration = async {
        let transaction = db.begin().await?;
        transaction
            .execute_unprepared(
                "DROP INDEX IF EXISTS idx_attachments_item_id;
                 DROP INDEX IF EXISTS idx_attachments_thread_id;
                 DROP INDEX IF EXISTS idx_thread_inputs_queue;
                 ALTER TABLE attachments RENAME TO attachments_v13;
                 ALTER TABLE thread_inputs RENAME TO thread_inputs_v13;
                 CREATE TABLE thread_inputs (
                     id TEXT PRIMARY KEY NOT NULL,
                     thread_id TEXT NOT NULL,
                     mail_id TEXT NOT NULL UNIQUE,
                     turn_id TEXT NOT NULL,
                     content TEXT NOT NULL,
                     attachments_json TEXT NOT NULL CHECK (json_valid(attachments_json)),
                     metadata_json TEXT NOT NULL,
                     presentation TEXT NOT NULL,
                     state_json TEXT NOT NULL CHECK (json_valid(state_json)),
                     state_kind TEXT GENERATED ALWAYS AS (
                         json_extract(state_json, '$.kind')
                     ) STORED NOT NULL CHECK (state_kind IN ('pending', 'claimed', 'consumed')),
                     queue_ordinal INTEGER NOT NULL,
                     queued_at INTEGER NOT NULL,
                     FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE
                 );
                 INSERT INTO thread_inputs (
                     id, thread_id, mail_id, turn_id, content, attachments_json,
                     metadata_json, presentation, state_json, queue_ordinal, queued_at
                 )
                 SELECT id, thread_id, mail_id, turn_id, content, '[]', metadata_json,
                        presentation, state_json, queue_ordinal, queued_at
                 FROM thread_inputs_v13;
                 DROP TABLE thread_inputs_v13;",
            )
            .await?;
        Schema::new(transaction.get_database_backend())
            .builder()
            .register(entity::attachment::Entity)
            .apply(&transaction)
            .await?;
        for (id, object_path, content_sha256) in &migrations {
            transaction
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "INSERT INTO attachments (
                         id, thread_id, kind, media_type, filename, storage_path,
                         byte_size, content_sha256, width, height, created_at
                     )
                     SELECT id, thread_id, 'image', media_type, filename, ?,
                            byte_size, ?, width, height, created_at
                     FROM attachments_v13 WHERE id = ?",
                    [
                        object_path.to_string_lossy().to_string().into(),
                        content_sha256.clone().into(),
                        id.clone().into(),
                    ],
                ))
                .await?;
        }
        transaction
            .execute_unprepared("DROP TABLE attachments_v13")
            .await?;
        create_thread_input_index(&transaction).await?;
        create_attachment_indexes(&transaction).await?;
        set_schema_version(&transaction, STUDIO_DATABASE_SCHEMA_VERSION).await?;
        transaction.commit().await?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    if migration.is_err() {
        cleanup_migration_blobs(&created_paths).await;
    }
    migration
}

async fn cleanup_migration_blobs(paths: &[PathBuf]) {
    for path in paths {
        let _ = tokio::fs::remove_file(path).await;
    }
}

pub(super) async fn initialize_studio_schema(db: &DatabaseConnection) -> Result<()> {
    create_task_run_table(db).await?;
    create_task_stop_event_table(db).await?;
    create_work_unit_table(db).await?;
    create_work_completion_table(db).await?;
    create_review_round_table(db).await?;
    create_task_issue_table(db).await?;
    create_merge_record_table(db).await?;
    create_thread_lifecycle_tables(db).await?;
    db.get_schema_builder()
        .register(entity::app_setting::Entity)
        .register(entity::project::Entity)
        .register(entity::attachment::Entity)
        .register(entity::thread_submission::Entity)
        .register(entity::thread_context_segment::Entity)
        .register(entity::thread_session_state::Entity)
        .apply(db)
        .await?;
    create_state_indexes(db).await?;
    create_task_relation_guards(db).await?;
    set_schema_version(db, STUDIO_DATABASE_SCHEMA_VERSION).await?;
    Ok(())
}

async fn create_task_stop_event_table(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS task_stop_events (
            id TEXT PRIMARY KEY NOT NULL,
            task_run_id TEXT NOT NULL,
            generation INTEGER NOT NULL CHECK (generation > 0),
            origin TEXT NOT NULL CHECK (
                origin IN ('userRequest', 'plannerDecision', 'runtimeFailure', 'applicationShutdown')
            ),
            reason TEXT NOT NULL,
            source_turn_id TEXT,
            created_at INTEGER NOT NULL,
            FOREIGN KEY (task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE,
            UNIQUE(task_run_id, generation)
        )
        "#,
    )
    .await?;
    Ok(())
}

async fn create_task_issue_table(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS task_issues (
            id TEXT PRIMARY KEY NOT NULL,
            task_run_id TEXT NOT NULL,
            source_thread_id TEXT NOT NULL,
            source_turn_id TEXT NOT NULL,
            source_agent_id TEXT NOT NULL,
            source_role TEXT NOT NULL,
            work_unit_id TEXT,
            review_round_id TEXT,
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN ('openRecoverable', 'openFatal', 'resolved')
            ),
            revision INTEGER NOT NULL CHECK (revision >= 0),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE,
            FOREIGN KEY (work_unit_id) REFERENCES work_units(id) ON DELETE SET NULL,
            FOREIGN KEY (review_round_id) REFERENCES review_rounds(id) ON DELETE SET NULL
        )
        "#,
    )
    .await?;
    Ok(())
}

async fn create_work_completion_table(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS work_completions (
            id TEXT PRIMARY KEY NOT NULL,
            task_run_id TEXT NOT NULL,
            work_unit_id TEXT NOT NULL,
            executor_agent_id TEXT NOT NULL,
            revision INTEGER NOT NULL CHECK (revision > 0),
            content_json TEXT NOT NULL CHECK (json_valid(content_json)),
            content_kind TEXT GENERATED ALWAYS AS (
                json_extract(content_json, '$.kind')
            ) STORED NOT NULL CHECK (content_kind IN ('delivery', 'noDelivery')),
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN ('readyForReview', 'changesRequired', 'approved')
            ),
            state_revision INTEGER NOT NULL CHECK (state_revision >= 0),
            base_commit TEXT NOT NULL,
            verification_summary TEXT NOT NULL,
            worktree_path TEXT NOT NULL,
            branch TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE,
            FOREIGN KEY (work_unit_id) REFERENCES work_units(id) ON DELETE CASCADE
        )
        "#,
    )
    .await?;
    Ok(())
}

async fn create_merge_record_table(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS merge_records (
            id TEXT PRIMARY KEY NOT NULL,
            task_run_id TEXT NOT NULL,
            work_unit_id TEXT NOT NULL,
            completion_id TEXT NOT NULL,
            completion_revision INTEGER NOT NULL,
            executor_agent_id TEXT NOT NULL,
            expected_previous_head TEXT NOT NULL,
            resulting_head TEXT NOT NULL,
            delivery_head TEXT NOT NULL,
            method TEXT NOT NULL,
            summary TEXT NOT NULL,
            cleanup_state_json TEXT NOT NULL CHECK (json_valid(cleanup_state_json)),
            cleanup_state_kind TEXT GENERATED ALWAYS AS (
                json_extract(cleanup_state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                cleanup_state_kind IN (
                    'pending', 'deferred', 'attempting', 'discarded',
                    'alreadyAbsent', 'failed'
                )
            ),
            revision INTEGER NOT NULL CHECK (revision >= 0),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE,
            FOREIGN KEY (work_unit_id) REFERENCES work_units(id) ON DELETE CASCADE,
            FOREIGN KEY (completion_id) REFERENCES work_completions(id) ON DELETE CASCADE
        )
        "#,
    )
    .await?;
    Ok(())
}

async fn create_thread_lifecycle_tables(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS threads (
            id TEXT PRIMARY KEY NOT NULL,
            project_id TEXT NOT NULL,
            title TEXT NOT NULL,
            mode TEXT NOT NULL,
            root_thread_id TEXT NOT NULL,
            parent_thread_id TEXT,
            role TEXT NOT NULL,
            agent_path TEXT NOT NULL UNIQUE,
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN (
                    'idle', 'queued', 'running', 'waitingTool',
                    'waitingInteraction', 'cancelling', 'closing', 'closed',
                    'faulted'
                )
            ),
            revision INTEGER NOT NULL,
            runtime_revision INTEGER,
            event_sequence INTEGER NOT NULL,
            metadata_json TEXT NOT NULL,
            usage_json TEXT NOT NULL,
            last_context_tokens INTEGER,
            trace_sequence INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            archived INTEGER NOT NULL,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS thread_inputs (
            id TEXT PRIMARY KEY NOT NULL,
            thread_id TEXT NOT NULL,
            mail_id TEXT NOT NULL UNIQUE,
            turn_id TEXT NOT NULL,
            content TEXT NOT NULL,
            attachments_json TEXT NOT NULL CHECK (json_valid(attachments_json)),
            metadata_json TEXT NOT NULL,
            presentation TEXT NOT NULL,
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (state_kind IN ('pending', 'claimed', 'consumed')),
            queue_ordinal INTEGER NOT NULL,
            queued_at INTEGER NOT NULL,
            FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS turns (
            id TEXT PRIMARY KEY NOT NULL,
            thread_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            revision INTEGER NOT NULL CHECK (revision >= 0),
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN (
                    'queued', 'running', 'completed', 'cancelled', 'failed',
                    'budgetLimited'
                )
            ),
            model_json TEXT,
            usage_json TEXT NOT NULL,
            metadata_json TEXT,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS items (
            id TEXT PRIMARY KEY NOT NULL,
            thread_id TEXT NOT NULL,
            turn_id TEXT NOT NULL,
            ordinal INTEGER NOT NULL,
            revision INTEGER NOT NULL CHECK (revision >= 0),
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN (
                    'text', 'thinking', 'tool', 'agent', 'turn', 'inference',
                    'plan', 'skill', 'file', 'contextCompaction'
                )
            ),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE,
            FOREIGN KEY (turn_id) REFERENCES turns(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS interactions (
            id TEXT PRIMARY KEY NOT NULL,
            thread_id TEXT NOT NULL,
            turn_id TEXT NOT NULL,
            item_id TEXT,
            tool_id TEXT,
            agent_path TEXT,
            revision INTEGER NOT NULL CHECK (revision >= 0),
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            interaction_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                interaction_kind IN ('userInput', 'toolApproval', 'planConfirmation')
            ),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.data.state.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN ('pending', 'resolved', 'cancelled', 'expired')
            ),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE
        )
        "#,
    )
    .await?;
    Ok(())
}

async fn create_task_run_table(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS task_runs (
            id TEXT PRIMARY KEY NOT NULL,
            project_id TEXT NOT NULL,
            root_thread_id TEXT NOT NULL,
            request TEXT NOT NULL,
            plan_json TEXT CHECK (plan_json IS NULL OR json_valid(plan_json)),
            workspace_root TEXT NOT NULL,
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN (
                    'planning', 'pendingConfirmation', 'editingDocuments',
                    'working', 'reviewing', 'completed'
                )
            ),
            revision INTEGER NOT NULL CHECK (revision >= 0),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY (root_thread_id) REFERENCES threads(id) ON DELETE CASCADE
        )
        "#,
    )
    .await?;
    Ok(())
}

async fn create_work_unit_table(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS work_units (
            id TEXT PRIMARY KEY NOT NULL,
            task_run_id TEXT NOT NULL,
            title TEXT NOT NULL,
            scope_hints_json TEXT NOT NULL,
            base_commit TEXT NOT NULL,
            worktree_path TEXT NOT NULL,
            branch TEXT NOT NULL,
            attempt INTEGER NOT NULL,
            supersedes_work_unit_id TEXT,
            executor_thread_id TEXT,
            requested_by_call_id TEXT NOT NULL,
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN (
                    'pending', 'running', 'waitingReview', 'reviewPassed',
                    'changesRequired', 'paused', 'completed', 'failed', 'cancelled'
                )
            ),
            revision INTEGER NOT NULL CHECK (revision >= 0),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE,
            FOREIGN KEY (supersedes_work_unit_id) REFERENCES work_units(id)
        )
        "#,
    )
    .await?;
    Ok(())
}

async fn create_review_round_table(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS review_rounds (
            id TEXT PRIMARY KEY NOT NULL,
            task_run_id TEXT NOT NULL,
            round INTEGER NOT NULL,
            scope TEXT NOT NULL CHECK (scope IN ('delivery', 'integrated')),
            work_unit_id TEXT,
            completion_id TEXT,
            completion_revision INTEGER,
            reviewed_head TEXT NOT NULL,
            requested_by_call_id TEXT NOT NULL,
            reviewer_thread_id TEXT,
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN (
                    'pendingDispatch', 'dispatched', 'running', 'passed',
                    'changesRequired', 'blocked', 'failed', 'cancelled'
                )
            ),
            revision INTEGER NOT NULL CHECK (revision >= 0),
            design_references_json TEXT NOT NULL,
            findings_json TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            file_reviews_json TEXT,
            CHECK (
                (scope = 'delivery' AND work_unit_id IS NOT NULL
                    AND completion_id IS NOT NULL AND completion_revision IS NOT NULL)
                OR
                (scope = 'integrated' AND work_unit_id IS NULL
                    AND completion_id IS NULL AND completion_revision IS NULL)
            ),
            FOREIGN KEY (task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE,
            FOREIGN KEY (work_unit_id) REFERENCES work_units(id) ON DELETE CASCADE,
            FOREIGN KEY (completion_id) REFERENCES work_completions(id) ON DELETE CASCADE
        )
        "#,
    )
    .await?;
    Ok(())
}

pub(super) fn non_empty_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "新会话".to_string()
    } else {
        title.chars().take(80).collect()
    }
}

async fn set_schema_version(db: &impl ConnectionTrait, version: i64) -> Result<()> {
    db.execute_unprepared(&format!("PRAGMA user_version = {version}"))
        .await?;
    Ok(())
}

async fn create_state_indexes(db: &DatabaseConnection) -> Result<()> {
    create_attachment_indexes(db).await?;
    let indexes = [
        Index::create()
            .name("idx_interactions_thread_state_updated")
            .table(entity::interaction::Entity)
            .col(entity::interaction::Column::ThreadId)
            .col(entity::interaction::Column::StateKind)
            .col((entity::interaction::Column::UpdatedAt, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_interactions_thread_turn")
            .table(entity::interaction::Entity)
            .col(entity::interaction::Column::ThreadId)
            .col(entity::interaction::Column::TurnId)
            .to_owned(),
        Index::create()
            .name("idx_merge_records_run_updated")
            .table(entity::merge_record::Entity)
            .col(entity::merge_record::Column::TaskRunId)
            .col((entity::merge_record::Column::UpdatedAt, IndexOrder::Desc))
            .col((entity::merge_record::Column::Id, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_projects_closed_last_opened_at")
            .table(entity::project::Entity)
            .col(entity::project::Column::Closed)
            .col((entity::project::Column::LastOpenedAt, IndexOrder::Desc))
            .col((entity::project::Column::UpdatedAt, IndexOrder::Desc))
            .col((entity::project::Column::Id, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_review_rounds_run_round")
            .table(entity::review_round::Entity)
            .col(entity::review_round::Column::TaskRunId)
            .col(entity::review_round::Column::Round)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_review_rounds_run_call")
            .table(entity::review_round::Entity)
            .col(entity::review_round::Column::TaskRunId)
            .col(entity::review_round::Column::RequestedByCallId)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_task_issues_run_created")
            .table(entity::task_issue::Entity)
            .col(entity::task_issue::Column::TaskRunId)
            .col((entity::task_issue::Column::CreatedAt, IndexOrder::Desc))
            .col((entity::task_issue::Column::Id, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_task_issues_run_turn")
            .table(entity::task_issue::Entity)
            .col(entity::task_issue::Column::TaskRunId)
            .col(entity::task_issue::Column::SourceTurnId)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_task_runs_state_updated")
            .table(entity::task_run::Entity)
            .col(entity::task_run::Column::StateKind)
            .col((entity::task_run::Column::UpdatedAt, IndexOrder::Desc))
            .col((entity::task_run::Column::Id, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_task_runs_root_thread_updated")
            .table(entity::task_run::Entity)
            .col(entity::task_run::Column::RootThreadId)
            .col((entity::task_run::Column::UpdatedAt, IndexOrder::Desc))
            .col((entity::task_run::Column::Id, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_threads_project_updated")
            .table(entity::thread::Entity)
            .col(entity::thread::Column::ProjectId)
            .col(entity::thread::Column::Archived)
            .col((entity::thread::Column::UpdatedAt, IndexOrder::Desc))
            .col((entity::thread::Column::Id, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_threads_root_parent")
            .table(entity::thread::Entity)
            .col(entity::thread::Column::RootThreadId)
            .col(entity::thread::Column::ParentThreadId)
            .col(entity::thread::Column::CreatedAt)
            .to_owned(),
        Index::create()
            .name("idx_thread_inputs_queue")
            .table(entity::thread_input::Entity)
            .col(entity::thread_input::Column::ThreadId)
            .col(entity::thread_input::Column::StateKind)
            .col(entity::thread_input::Column::QueueOrdinal)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_thread_submissions_ordinal")
            .table(entity::thread_submission::Entity)
            .col(entity::thread_submission::Column::ThreadId)
            .col(entity::thread_submission::Column::Ordinal)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_turns_thread_ordinal")
            .table(entity::turn::Entity)
            .col(entity::turn::Column::ThreadId)
            .col(entity::turn::Column::Ordinal)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_items_thread_ordinal")
            .table(entity::item::Entity)
            .col(entity::item::Column::ThreadId)
            .col(entity::item::Column::Ordinal)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_items_turn_ordinal")
            .table(entity::item::Entity)
            .col(entity::item::Column::TurnId)
            .col(entity::item::Column::Ordinal)
            .to_owned(),
        Index::create()
            .name("idx_items_thread_state_kind_ordinal")
            .table(entity::item::Entity)
            .col(entity::item::Column::ThreadId)
            .col(entity::item::Column::StateKind)
            .col((entity::item::Column::Ordinal, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_thread_context_segments_thread_ordinal")
            .table(entity::thread_context_segment::Entity)
            .col(entity::thread_context_segment::Column::ThreadId)
            .col(entity::thread_context_segment::Column::Ordinal)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_thread_context_segments_thread_revision")
            .table(entity::thread_context_segment::Entity)
            .col(entity::thread_context_segment::Column::ThreadId)
            .col(entity::thread_context_segment::Column::Revision)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_work_units_run_state_kind")
            .table(entity::work_unit::Entity)
            .col(entity::work_unit::Column::TaskRunId)
            .col(entity::work_unit::Column::StateKind)
            .col(entity::work_unit::Column::CreatedAt)
            .col(entity::work_unit::Column::Id)
            .to_owned(),
        Index::create()
            .name("idx_work_completions_unit_revision")
            .table(entity::work_completion::Entity)
            .col(entity::work_completion::Column::WorkUnitId)
            .col(entity::work_completion::Column::Revision)
            .unique()
            .to_owned(),
    ];
    for index in indexes {
        execute_index(db, index).await?;
    }

    db.execute_unprepared(
        r#"
        CREATE UNIQUE INDEX idx_task_runs_one_open_per_root
        ON task_runs(root_thread_id)
        WHERE state_kind != 'completed'
        "#,
    )
    .await?;

    db.execute_unprepared(
        r#"
        CREATE UNIQUE INDEX idx_review_rounds_active_delivery
        ON review_rounds(work_unit_id)
        WHERE scope = 'delivery'
            AND state_kind IN ('pendingDispatch', 'dispatched', 'running');

        CREATE UNIQUE INDEX idx_review_rounds_active_integrated
        ON review_rounds(task_run_id)
        WHERE scope = 'integrated'
            AND state_kind IN ('pendingDispatch', 'dispatched', 'running');
        "#,
    )
    .await?;
    Ok(())
}

async fn create_attachment_indexes(db: &impl ConnectionTrait) -> Result<()> {
    for index in [Index::create()
        .name("idx_attachments_thread_id")
        .table(entity::attachment::Entity)
        .col(entity::attachment::Column::ThreadId)
        .to_owned()]
    {
        db.execute(&index).await?;
    }
    Ok(())
}

async fn create_thread_input_index(db: &impl ConnectionTrait) -> Result<()> {
    let index = Index::create()
        .name("idx_thread_inputs_queue")
        .table(entity::thread_input::Entity)
        .col(entity::thread_input::Column::ThreadId)
        .col(entity::thread_input::Column::StateKind)
        .col(entity::thread_input::Column::QueueOrdinal)
        .unique()
        .to_owned();
    db.execute(&index).await?;
    Ok(())
}

async fn create_task_relation_guards(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"
        CREATE TRIGGER guard_work_completion_owner_insert
        BEFORE INSERT ON work_completions
        WHEN NOT EXISTS (
            SELECT 1 FROM work_units
            WHERE id = NEW.work_unit_id AND task_run_id = NEW.task_run_id
        )
        BEGIN
            SELECT RAISE(ABORT, 'work completion owner mismatch');
        END;

        CREATE TRIGGER guard_work_completion_owner_update
        BEFORE UPDATE ON work_completions
        WHEN NOT EXISTS (
            SELECT 1 FROM work_units
            WHERE id = NEW.work_unit_id AND task_run_id = NEW.task_run_id
        )
        BEGIN
            SELECT RAISE(ABORT, 'work completion owner mismatch');
        END;

        CREATE TRIGGER guard_review_round_owner_insert
        BEFORE INSERT ON review_rounds
        WHEN NEW.scope = 'delivery' AND (
            NOT EXISTS (
                SELECT 1 FROM work_units
                WHERE id = NEW.work_unit_id AND task_run_id = NEW.task_run_id
            )
            OR NOT EXISTS (
                SELECT 1 FROM work_completions
                WHERE id = NEW.completion_id
                    AND task_run_id = NEW.task_run_id
                    AND work_unit_id = NEW.work_unit_id
                    AND revision = NEW.completion_revision
            )
        )
        BEGIN
            SELECT RAISE(ABORT, 'review round owner mismatch');
        END;

        CREATE TRIGGER guard_review_round_owner_update
        BEFORE UPDATE ON review_rounds
        WHEN NEW.scope = 'delivery' AND (
            NOT EXISTS (
                SELECT 1 FROM work_units
                WHERE id = NEW.work_unit_id AND task_run_id = NEW.task_run_id
            )
            OR NOT EXISTS (
                SELECT 1 FROM work_completions
                WHERE id = NEW.completion_id
                    AND task_run_id = NEW.task_run_id
                    AND work_unit_id = NEW.work_unit_id
                    AND revision = NEW.completion_revision
            )
        )
        BEGIN
            SELECT RAISE(ABORT, 'review round owner mismatch');
        END;

        CREATE TRIGGER guard_merge_record_owner_insert
        BEFORE INSERT ON merge_records
        WHEN NOT EXISTS (
            SELECT 1 FROM work_units
            WHERE id = NEW.work_unit_id AND task_run_id = NEW.task_run_id
        ) OR NOT EXISTS (
            SELECT 1 FROM work_completions
            WHERE id = NEW.completion_id
                AND task_run_id = NEW.task_run_id
                AND work_unit_id = NEW.work_unit_id
                AND revision = NEW.completion_revision
        )
        BEGIN
            SELECT RAISE(ABORT, 'merge record owner mismatch');
        END;

        CREATE TRIGGER guard_merge_record_owner_update
        BEFORE UPDATE ON merge_records
        WHEN NOT EXISTS (
            SELECT 1 FROM work_units
            WHERE id = NEW.work_unit_id AND task_run_id = NEW.task_run_id
        ) OR NOT EXISTS (
            SELECT 1 FROM work_completions
            WHERE id = NEW.completion_id
                AND task_run_id = NEW.task_run_id
                AND work_unit_id = NEW.work_unit_id
                AND revision = NEW.completion_revision
        )
        BEGIN
            SELECT RAISE(ABORT, 'merge record owner mismatch');
        END;
        "#,
    )
    .await?;
    Ok(())
}

async fn execute_index(db: &DatabaseConnection, index: IndexCreateStatement) -> Result<()> {
    db.execute(&index).await?;
    Ok(())
}
