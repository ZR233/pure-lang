use anyhow::Result;
use sea_orm::sea_query::{Expr, ExprTrait, Index, IndexCreateStatement, IndexOrder};
use sea_orm::{ConditionalStatement, ConnectionTrait, DatabaseConnection};

use crate::studio::entity;

pub(super) const STUDIO_DATABASE_SCHEMA_VERSION: i64 = 1;

pub(super) async fn initialize_studio_schema(db: &DatabaseConnection) -> Result<()> {
    db.get_schema_builder()
        .register(entity::app_setting::Entity)
        .register(entity::project::Entity)
        .register(entity::attachment::Entity)
        .register(entity::interaction::Entity)
        .register(entity::task_run::Entity)
        .register(entity::work_unit::Entity)
        .register(entity::work_completion::Entity)
        .register(entity::review_round::Entity)
        .register(entity::merge_record::Entity)
        .register(entity::branch_lease::Entity)
        .register(entity::thread::Entity)
        .register(entity::thread_input::Entity)
        .register(entity::turn::Entity)
        .register(entity::item::Entity)
        .apply(db)
        .await?;
    create_state_indexes(db).await?;
    set_schema_version(db, STUDIO_DATABASE_SCHEMA_VERSION).await?;
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

async fn set_schema_version(db: &DatabaseConnection, version: i64) -> Result<()> {
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
            .name("idx_task_runs_phase_updated")
            .table(entity::task_run::Entity)
            .col(entity::task_run::Column::Phase)
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
            .name("idx_work_units_run_status")
            .table(entity::work_unit::Entity)
            .col(entity::work_unit::Column::TaskRunId)
            .col(entity::work_unit::Column::Status)
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

    execute_index(
        db,
        Index::create()
            .name("idx_review_rounds_active_delivery")
            .table(entity::review_round::Entity)
            .col(entity::review_round::Column::WorkUnitId)
            .and_where(Expr::col(entity::review_round::Column::Scope).eq("delivery"))
            .and_where(Expr::col(entity::review_round::Column::Status).eq("pending"))
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
            .and_where(Expr::col(entity::review_round::Column::Status).eq("pending"))
            .unique()
            .to_owned(),
    )
    .await?;
    Ok(())
}

async fn execute_index(db: &DatabaseConnection, index: IndexCreateStatement) -> Result<()> {
    db.execute(&index).await?;
    Ok(())
}
