use anyhow::Result;
use sea_orm::sea_query::{Expr, ExprTrait, Index, IndexCreateStatement, IndexOrder};
use sea_orm::{ConditionalStatement, ConnectionTrait, DatabaseConnection};

use crate::studio::entity;

/// 唯一支持的 Studio schema 版本；任何非 v10 库都按不兼容处理并精确重建。
pub(super) const STUDIO_DATABASE_SCHEMA_VERSION: i64 = 10;

pub(super) async fn initialize_studio_schema(db: &DatabaseConnection) -> Result<()> {
    create_task_run_table(db).await?;
    create_work_unit_table(db).await?;
    create_review_round_table(db).await?;
    db.get_schema_builder()
        .register(entity::app_setting::Entity)
        .register(entity::project::Entity)
        .register(entity::attachment::Entity)
        .register(entity::interaction::Entity)
        .register(entity::task_failure::Entity)
        .register(entity::work_completion::Entity)
        .register(entity::merge_record::Entity)
        .register(entity::branch_lease::Entity)
        .register(entity::thread::Entity)
        .register(entity::thread_input::Entity)
        .register(entity::thread_submission::Entity)
        .register(entity::turn::Entity)
        .register(entity::item::Entity)
        .register(entity::thread_context_segment::Entity)
        .register(entity::thread_session_state::Entity)
        .apply(db)
        .await?;
    create_state_indexes(db).await?;
    create_task_relation_guards(db).await?;
    set_schema_version(db, STUDIO_DATABASE_SCHEMA_VERSION).await?;
    Ok(())
}

async fn create_task_run_table(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"
        CREATE TABLE IF NOT EXISTS task_runs (
            id TEXT PRIMARY KEY NOT NULL,
            root_thread_id TEXT NOT NULL,
            plan TEXT NOT NULL,
            workspace_root TEXT NOT NULL,
            git_common_dir TEXT NOT NULL,
            branch TEXT NOT NULL,
            base_commit TEXT NOT NULL,
            expected_head TEXT NOT NULL,
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN (
                    'designUpdating', 'implementing', 'merging', 'reviewing',
                    'reworking', 'stopping', 'blocked', 'completed', 'failed',
                    'cancelled'
                )
            ),
            revision INTEGER NOT NULL CHECK (revision >= 0),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
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
            executor_thread_id TEXT,
            requested_by_call_id TEXT NOT NULL,
            state_json TEXT NOT NULL CHECK (json_valid(state_json)),
            state_kind TEXT GENERATED ALWAYS AS (
                json_extract(state_json, '$.kind')
            ) STORED NOT NULL CHECK (
                state_kind IN (
                    'pending', 'running', 'awaitingCompletion', 'readyForReview',
                    'reviewing', 'changesRequested', 'approved', 'merged',
                    'noDelivery', 'needsAttention', 'failed', 'cancelled'
                )
            ),
            revision INTEGER NOT NULL CHECK (revision >= 0),
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (task_run_id) REFERENCES task_runs(id) ON DELETE CASCADE
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
                state_kind IN ('pending', 'pass', 'changesRequired', 'blocked', 'failed')
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
    let indexes = [
        Index::create()
            .name("idx_attachments_item_id")
            .table(entity::attachment::Entity)
            .col(entity::attachment::Column::ItemId)
            .to_owned(),
        Index::create()
            .name("idx_attachments_thread_id")
            .table(entity::attachment::Entity)
            .col(entity::attachment::Column::ThreadId)
            .to_owned(),
        Index::create()
            .name("idx_branch_leases_common_branch")
            .table(entity::branch_lease::Entity)
            .col(entity::branch_lease::Column::GitCommonDir)
            .col(entity::branch_lease::Column::Branch)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_interactions_thread_status_updated")
            .table(entity::interaction::Entity)
            .col(entity::interaction::Column::ThreadId)
            .col(entity::interaction::Column::Status)
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
            .name("idx_task_failures_run_created")
            .table(entity::task_failure::Entity)
            .col(entity::task_failure::Column::TaskRunId)
            .col((entity::task_failure::Column::CreatedAt, IndexOrder::Desc))
            .col((entity::task_failure::Column::Id, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_task_failures_run_turn")
            .table(entity::task_failure::Entity)
            .col(entity::task_failure::Column::TaskRunId)
            .col(entity::task_failure::Column::SourceTurnId)
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
            .col(entity::thread_input::Column::State)
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
            .name("idx_work_units_run_status")
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
        WHERE state_kind NOT IN ('completed', 'failed', 'cancelled')
        "#,
    )
    .await?;

    execute_index(
        db,
        Index::create()
            .name("idx_review_rounds_active_delivery")
            .table(entity::review_round::Entity)
            .col(entity::review_round::Column::WorkUnitId)
            .and_where(Expr::col(entity::review_round::Column::Scope).eq("delivery"))
            .and_where(Expr::col(entity::review_round::Column::StateKind).eq("pending"))
            .unique()
            .to_owned(),
    )
    .await?;
    execute_index(
        db,
        Index::create()
            .name("idx_review_rounds_active_integrated")
            .table(entity::review_round::Entity)
            .col(entity::review_round::Column::TaskRunId)
            .and_where(Expr::col(entity::review_round::Column::Scope).eq("integrated"))
            .and_where(Expr::col(entity::review_round::Column::StateKind).eq("pending"))
            .unique()
            .to_owned(),
    )
    .await?;
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
