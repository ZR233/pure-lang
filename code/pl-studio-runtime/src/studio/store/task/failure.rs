use anyhow::{Context, Result};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    SqliteTransactionMode, TransactionOptions, TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    RecordTaskAgentFailure, ReviewRoundState, ReviewVerdict, TaskCommand, TaskFailureDisposition,
    TaskFailureRecord, TaskFailureSettlement, WorkUnitState, WorkUnitStatus,
};

use super::review::update_review_round_state;
use super::work_unit::{update_work_unit_state, work_unit_state};

impl StudioStore {
    pub(crate) async fn record_task_agent_failure(
        &self,
        input: RecordTaskAgentFailure,
    ) -> Result<Option<TaskFailureSettlement>> {
        // Acquire SQLite's write reservation before reading the TaskRun. This makes
        // terminalization first-writer-wins: a second fatal event cannot observe a
        // stale non-terminal phase and overwrite terminal_failure_id.
        let tx = self
            .db
            .begin_with_options(TransactionOptions {
                sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
                ..Default::default()
            })
            .await?;
        let result = async {
            let Some(run) = entities::task_run::Entity::find()
                .filter(entities::task_run::Column::RootThreadId.eq(input.root_thread_id.clone()))
                .order_by_desc(entities::task_run::Column::UpdatedAt)
                .one(&tx)
                .await?
            else {
                return Ok(None);
            };
            let record = super::task_run_record(run.clone())?;
            if record.kind().is_terminal() {
                return Ok(None);
            }
            if entities::task_failure::Entity::find()
                .filter(entities::task_failure::Column::TaskRunId.eq(run.id.clone()))
                .filter(
                    entities::task_failure::Column::SourceTurnId.eq(input.source_turn_id.clone()),
                )
                .one(&tx)
                .await?
                .is_some()
            {
                return Ok(Some(TaskFailureSettlement {
                    run: super::task_run_record(run)?,
                    terminalized: false,
                }));
            }

            let work_unit = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
                .filter(
                    entities::work_unit::Column::ExecutorThreadId
                        .eq(input.source_thread_id.clone()),
                )
                .one(&tx)
                .await?;
            let review_round = entities::review_round::Entity::find()
                .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
                .filter(
                    entities::review_round::Column::ReviewerThreadId
                        .eq(input.source_thread_id.clone()),
                )
                .order_by_desc(entities::review_round::Column::Round)
                .one(&tx)
                .await?;
            let disposition = TaskFailureDisposition::for_turn_failure(&input.failure);
            let now = unix_seconds();
            let failure_model = entities::task_failure::ActiveModel {
                id: Set(new_id("task-failure")),
                task_run_id: Set(run.id.clone()),
                source_thread_id: Set(input.source_thread_id),
                source_turn_id: Set(input.source_turn_id),
                source_agent_id: Set(input.source_agent_id),
                source_role: Set(input.source_role),
                work_unit_id: Set(work_unit.as_ref().map(|unit| unit.id.clone())),
                review_round_id: Set(review_round.as_ref().map(|round| round.id.clone())),
                disposition: Set(disposition.as_str().to_string()),
                failure_json: Set(serde_json::to_string(&input.failure)?),
                resolved_at: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await?;

            let terminalized = disposition == TaskFailureDisposition::Fatal;
            let run = if terminalized {
                settle_task_children(&tx, &failure_model.task_run_id, &input.failure.message)
                    .await?;
                entities::project_lease::Entity::delete_many()
                    .filter(
                        entities::project_lease::Column::TaskRunId
                            .eq(failure_model.task_run_id.clone()),
                    )
                    .exec(&tx)
                    .await?;
                super::apply_task_command(
                    &tx,
                    run,
                    TaskCommand::Fail {
                        message: input.failure.message.clone(),
                        failure_id: Some(failure_model.id.clone()),
                    },
                )
                .await?
            } else {
                run
            };
            Ok(Some(TaskFailureSettlement {
                run: super::task_run_record(run)?,
                terminalized,
            }))
        }
        .await;
        match result {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn list_task_failures(
        &self,
        task_run_id: &str,
    ) -> Result<Vec<TaskFailureRecord>> {
        entities::task_failure::Entity::find()
            .filter(entities::task_failure::Column::TaskRunId.eq(task_run_id))
            .order_by_asc(entities::task_failure::Column::CreatedAt)
            .all(&self.db)
            .await?
            .into_iter()
            .map(task_failure_record)
            .collect()
    }

    pub(crate) async fn resolve_recoverable_task_failures(
        &self,
        source_thread_id: &str,
    ) -> Result<()> {
        let now = unix_seconds();
        for model in entities::task_failure::Entity::find()
            .filter(entities::task_failure::Column::SourceThreadId.eq(source_thread_id.to_string()))
            .filter(
                entities::task_failure::Column::Disposition
                    .eq(TaskFailureDisposition::Recoverable.as_str()),
            )
            .filter(entities::task_failure::Column::ResolvedAt.is_null())
            .all(&self.db)
            .await?
        {
            let mut active: entities::task_failure::ActiveModel = model.into();
            active.resolved_at = Set(Some(now));
            active.updated_at = Set(now);
            active.update(&self.db).await?;
        }
        Ok(())
    }
}

async fn settle_task_children(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
    message: &str,
) -> Result<()> {
    for unit in entities::work_unit::Entity::find()
        .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id))
        .all(tx)
        .await?
    {
        let state = work_unit_state(&unit)?;
        let status = state.status();
        if matches!(status, WorkUnitStatus::Merged | WorkUnitStatus::NoDelivery) {
            continue;
        }
        let mut progress = state.into_progress();
        progress.execution_error = Some(message.to_string());
        update_work_unit_state(tx, unit, WorkUnitState::failed(progress)).await?;
    }
    for round in entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id))
        .filter(entities::review_round::Column::StateKind.eq(ReviewVerdict::Pending.as_str()))
        .all(tx)
        .await?
    {
        let state = ReviewRoundState::failed(message.to_string(), message.to_string());
        update_review_round_state(tx, round, state).await?;
    }
    Ok(())
}

fn task_failure_record(model: entities::task_failure::Model) -> Result<TaskFailureRecord> {
    Ok(TaskFailureRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        source_thread_id: model.source_thread_id,
        source_turn_id: model.source_turn_id,
        source_agent_id: model.source_agent_id,
        source_role: model.source_role,
        work_unit_id: model.work_unit_id,
        review_round_id: model.review_round_id,
        disposition: TaskFailureDisposition::from_str(&model.disposition)
            .with_context(|| format!("invalid task failure disposition: {}", model.disposition))?,
        failure: serde_json::from_str(&model.failure_json)?,
        resolved_at: model.resolved_at,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
