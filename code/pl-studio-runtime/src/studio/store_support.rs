use anyhow::Result;
use sea_orm::sea_query::{Expr, ExprTrait, Index, IndexCreateStatement, IndexOrder};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConditionalStatement, ConnectionTrait, DatabaseConnection,
};

use crate::studio::entity;

pub(super) const STATE_DATABASE_SCHEMA_VERSION: i64 = 11;
pub(super) const HISTORY_DATABASE_SCHEMA_VERSION: i64 = 1;
pub(super) const STATE_DATABASE_KIND: &str = "state";
pub(super) const HISTORY_DATABASE_KIND: &str = "history";
const STORAGE_METADATA_ID: &str = "primary";

pub(super) async fn initialize_state_schema(
    db: &DatabaseConnection,
    storage_generation_id: &str,
    created_at: i64,
) -> Result<()> {
    db.get_schema_builder()
        .register(entity::storage_metadata::Entity)
        .register(entity::history_gc_job::Entity)
        .register(entity::app_setting::Entity)
        .register(entity::project::Entity)
        .register(entity::session::Entity)
        .register(entity::attachment::Entity)
        .register(entity::interaction::Entity)
        .register(entity::task_run::Entity)
        .register(entity::work_unit::Entity)
        .register(entity::work_completion::Entity)
        .register(entity::agent_outcome::Entity)
        .register(entity::review_round::Entity)
        .register(entity::merge_record::Entity)
        .register(entity::branch_lease::Entity)
        .register(entity::agent_runtime_state::Entity)
        .register(entity::agent_runtime_session::Entity)
        .register(entity::agent_pending_input::Entity)
        .register(entity::agent_active_input::Entity)
        .register(entity::agent_turn::Entity)
        .register(entity::session_view_snapshot::Entity)
        .apply(db)
        .await?;
    create_state_indexes(db).await?;
    write_storage_metadata(
        db,
        STATE_DATABASE_KIND,
        STATE_DATABASE_SCHEMA_VERSION,
        storage_generation_id,
        created_at,
    )
    .await?;
    set_schema_version(db, STATE_DATABASE_SCHEMA_VERSION).await?;
    Ok(())
}

pub(super) async fn initialize_history_schema(
    db: &DatabaseConnection,
    storage_generation_id: &str,
    created_at: i64,
) -> Result<()> {
    db.get_schema_builder()
        .register(entity::storage_metadata::Entity)
        .register(entity::session_history_turn::Entity)
        .register(entity::session_history_item::Entity)
        .register(entity::session_history_checkpoint::Entity)
        .apply(db)
        .await?;
    create_history_indexes(db).await?;
    write_storage_metadata(
        db,
        HISTORY_DATABASE_KIND,
        HISTORY_DATABASE_SCHEMA_VERSION,
        storage_generation_id,
        created_at,
    )
    .await?;
    set_schema_version(db, HISTORY_DATABASE_SCHEMA_VERSION).await?;
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

async fn write_storage_metadata(
    db: &DatabaseConnection,
    database_kind: &str,
    schema_version: i64,
    storage_generation_id: &str,
    created_at: i64,
) -> Result<()> {
    entity::storage_metadata::ActiveModel {
        id: Set(STORAGE_METADATA_ID.to_string()),
        database_kind: Set(database_kind.to_string()),
        schema_version: Set(schema_version),
        storage_generation_id: Set(storage_generation_id.to_string()),
        created_at: Set(created_at),
    }
    .insert(db)
    .await?;
    Ok(())
}

async fn set_schema_version(db: &DatabaseConnection, version: i64) -> Result<()> {
    db.execute_unprepared(&format!("PRAGMA user_version = {version}"))
        .await?;
    Ok(())
}

async fn create_state_indexes(db: &DatabaseConnection) -> Result<()> {
    let indexes = [
        Index::create()
            .name("idx_agent_outcomes_run_agent_attempt")
            .table(entity::agent_outcome::Entity)
            .col(entity::agent_outcome::Column::TaskRunId)
            .col(entity::agent_outcome::Column::AgentId)
            .col(entity::agent_outcome::Column::Attempt)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_agent_outcomes_run_status")
            .table(entity::agent_outcome::Entity)
            .col(entity::agent_outcome::Column::TaskRunId)
            .col(entity::agent_outcome::Column::Status)
            .col((entity::agent_outcome::Column::UpdatedAt, IndexOrder::Desc))
            .col((entity::agent_outcome::Column::Id, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_attachments_message_id")
            .table(entity::attachment::Entity)
            .col(entity::attachment::Column::MessageId)
            .to_owned(),
        Index::create()
            .name("idx_attachments_session_id")
            .table(entity::attachment::Entity)
            .col(entity::attachment::Column::SessionId)
            .to_owned(),
        Index::create()
            .name("idx_branch_leases_common_branch")
            .table(entity::branch_lease::Entity)
            .col(entity::branch_lease::Column::GitCommonDir)
            .col(entity::branch_lease::Column::Branch)
            .unique()
            .to_owned(),
        Index::create()
            .name("idx_interactions_session_status_updated")
            .table(entity::interaction::Entity)
            .col(entity::interaction::Column::SessionId)
            .col(entity::interaction::Column::Status)
            .col((entity::interaction::Column::UpdatedAt, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_interactions_session_turn")
            .table(entity::interaction::Entity)
            .col(entity::interaction::Column::SessionId)
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
            .name("idx_sessions_parent_session")
            .table(entity::session::Entity)
            .col(entity::session::Column::ParentSessionId)
            .to_owned(),
        Index::create()
            .name("idx_sessions_root_session")
            .table(entity::session::Entity)
            .col(entity::session::Column::RootSessionId)
            .col(entity::session::Column::CreatedAt)
            .col(entity::session::Column::Id)
            .to_owned(),
        Index::create()
            .name("idx_sessions_project_updated_at")
            .table(entity::session::Entity)
            .col(entity::session::Column::ProjectId)
            .col(entity::session::Column::Archived)
            .col((entity::session::Column::UpdatedAt, IndexOrder::Desc))
            .col((entity::session::Column::Id, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_task_runs_phase_updated")
            .table(entity::task_run::Entity)
            .col(entity::task_run::Column::Phase)
            .col((entity::task_run::Column::UpdatedAt, IndexOrder::Desc))
            .col((entity::task_run::Column::Id, IndexOrder::Desc))
            .to_owned(),
        Index::create()
            .name("idx_task_runs_session_updated")
            .table(entity::task_run::Entity)
            .col(entity::task_run::Column::SessionId)
            .col((entity::task_run::Column::UpdatedAt, IndexOrder::Desc))
            .col((entity::task_run::Column::Id, IndexOrder::Desc))
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
        Index::create()
            .name("idx_agent_turns_session_started")
            .table(entity::agent_turn::Entity)
            .col(entity::agent_turn::Column::SessionId)
            .col((entity::agent_turn::Column::StartedAt, IndexOrder::Desc))
            .col((entity::agent_turn::Column::TurnId, IndexOrder::Desc))
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

async fn create_history_indexes(db: &DatabaseConnection) -> Result<()> {
    for index in [
        Index::create()
            .name("idx_history_items_turn_sequence")
            .table(entity::session_history_item::Entity)
            .col(entity::session_history_item::Column::SessionId)
            .col(entity::session_history_item::Column::TurnId)
            .col(entity::session_history_item::Column::Sequence)
            .to_owned(),
        Index::create()
            .name("idx_history_checkpoints_through_sequence")
            .table(entity::session_history_checkpoint::Entity)
            .col(entity::session_history_checkpoint::Column::SessionId)
            .col((
                entity::session_history_checkpoint::Column::ThroughSequence,
                IndexOrder::Desc,
            ))
            .to_owned(),
    ] {
        execute_index(db, index).await?;
    }
    Ok(())
}

async fn execute_index(db: &DatabaseConnection, index: IndexCreateStatement) -> Result<()> {
    db.execute(&index).await?;
    Ok(())
}
