use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    BeginIntegratedReview, ReviewFileCoverage, ReviewRoundRecord, ReviewRoundState, ReviewScope,
    ReviewTarget, TaskCommand, WorkCompletionStatus, WorkUnitState, WorkUnitStatus,
};

use super::super::apply_task_command;
use super::super::work_unit::{update_work_unit_state, work_unit_state};
use super::helpers::{
    active_implementation_run, ensure_no_pending_delivery_review, ensure_no_pending_review,
    ensure_review_call_unused, finish_transaction,
};
use super::record::review_round_record;

impl StudioStore {
    pub(crate) async fn begin_delivery_review(
        &self,
        thread_id: &str,
        executor_agent_id: &str,
        requested_by_call_id: &str,
    ) -> Result<ReviewRoundRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_implementation_run(&tx, thread_id).await?;
            ensure_review_call_unused(&tx, &run.id, requested_by_call_id).await?;
            let units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
                .filter(
                    entities::work_unit::Column::ExecutorThreadId.eq(executor_agent_id.to_string()),
                )
                .all(&tx)
                .await?;
            let work_unit = match units.as_slice() {
                [unit] if unit.state_kind == WorkUnitStatus::ReadyForReview.as_str() => {
                    unit.clone()
                }
                [unit] => bail!(
                    "executor work unit is {}, not readyForReview",
                    unit.state_kind
                ),
                [] => bail!("executor work unit not found"),
                _ => bail!("executor owns multiple work units"),
            };
            let completion = entities::work_completion::Entity::find()
                .filter(entities::work_completion::Column::WorkUnitId.eq(work_unit.id.clone()))
                .order_by_desc(entities::work_completion::Column::Revision)
                .one(&tx)
                .await?
                .context("work unit has no completion")?;
            if completion.executor_agent_id != executor_agent_id
                || completion.status != WorkCompletionStatus::ReadyForReview.as_str()
            {
                bail!("latest completion is not ready for review");
            }
            let changed_files = serde_json::from_str(&completion.changed_files_json)?;
            ensure_no_pending_delivery_review(&tx, &run.id, &work_unit.id).await?;
            let round = insert_review_round(
                &tx,
                NewReviewRound {
                    task_run_id: &run.id,
                    target: NewReviewTarget::Delivery {
                        work_unit_id: &work_unit.id,
                        completion_id: &completion.id,
                        completion_revision: completion.revision,
                        reviewed_head: completion
                            .head_commit
                            .as_deref()
                            .unwrap_or(work_unit.base_commit.as_str()),
                    },
                    requested_by_call_id,
                    changed_files,
                },
            )
            .await?;
            let state = work_unit_state(&work_unit)?;
            let progress = state.into_progress();
            update_work_unit_state(&tx, work_unit, WorkUnitState::reviewing(progress)).await?;
            Ok(round)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn begin_integrated_review(
        &self,
        thread_id: &str,
        request: BeginIntegratedReview,
    ) -> Result<ReviewRoundRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_implementation_run(&tx, thread_id).await?;
            let latest_merge = entities::merge_record::Entity::find()
                .filter(entities::merge_record::Column::TaskRunId.eq(run.id.clone()))
                .order_by_desc(entities::merge_record::Column::CreatedAt)
                .order_by_desc(entities::merge_record::Column::Id)
                .one(&tx)
                .await?
                .context("integrated review requires durable merge evidence")?;
            if latest_merge.resulting_head != request.reviewed_head {
                bail!("integrated review target changed before round creation");
            }
            ensure_review_call_unused(&tx, &run.id, &request.requested_by_call_id).await?;
            let work_units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
                .all(&tx)
                .await?;
            if work_units.iter().any(|unit| {
                !matches!(
                    WorkUnitStatus::from_str(&unit.state_kind),
                    Some(WorkUnitStatus::Merged | WorkUnitStatus::NoDelivery)
                )
            }) {
                bail!("integrated review requires every work unit to be merged or noDelivery");
            }
            ensure_no_pending_review(&tx, &run.id).await?;
            let round = insert_review_round(
                &tx,
                NewReviewRound {
                    task_run_id: &run.id,
                    target: NewReviewTarget::Integrated {
                        reviewed_head: &request.reviewed_head,
                    },
                    requested_by_call_id: &request.requested_by_call_id,
                    changed_files: request.changed_files,
                },
            )
            .await?;
            apply_task_command(
                &tx,
                run,
                TaskCommand::BeginReviewing(ReviewTarget::Integration {
                    reviewed_head: request.reviewed_head,
                }),
            )
            .await?;
            Ok(round)
        }
        .await;
        finish_transaction(tx, result).await
    }
}

struct NewReviewRound<'a> {
    task_run_id: &'a str,
    target: NewReviewTarget<'a>,
    requested_by_call_id: &'a str,
    changed_files: Vec<String>,
}

enum NewReviewTarget<'a> {
    Delivery {
        work_unit_id: &'a str,
        completion_id: &'a str,
        completion_revision: i32,
        reviewed_head: &'a str,
    },
    Integrated {
        reviewed_head: &'a str,
    },
}

async fn insert_review_round(
    tx: &sea_orm::DatabaseTransaction,
    request: NewReviewRound<'_>,
) -> Result<ReviewRoundRecord> {
    let count = entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(request.task_run_id.to_string()))
        .count(tx)
        .await?;
    let (scope, work_unit_id, completion_id, completion_revision, reviewed_head) =
        match request.target {
            NewReviewTarget::Delivery {
                work_unit_id,
                completion_id,
                completion_revision,
                reviewed_head,
            } => (
                ReviewScope::Delivery,
                Some(work_unit_id.to_string()),
                Some(completion_id.to_string()),
                Some(completion_revision),
                reviewed_head,
            ),
            NewReviewTarget::Integrated { reviewed_head } => {
                (ReviewScope::Integrated, None, None, None, reviewed_head)
            }
        };
    let now = unix_seconds();
    let file_reviews = ReviewFileCoverage::pending(request.changed_files);
    review_round_record(
        entities::review_round::ActiveModel {
            id: Set(new_id("review")),
            task_run_id: Set(request.task_run_id.to_string()),
            round: Set(i32::try_from(count + 1)?),
            scope: Set(scope.as_str().to_string()),
            work_unit_id: Set(work_unit_id),
            completion_id: Set(completion_id),
            completion_revision: Set(completion_revision),
            reviewed_head: Set(reviewed_head.to_string()),
            requested_by_call_id: Set(request.requested_by_call_id.to_string()),
            reviewer_thread_id: Set(None),
            state_json: Set(serde_json::to_string(&ReviewRoundState::pending())?),
            revision: Set(0),
            design_references_json: Set("[]".to_string()),
            findings_json: Set("[]".to_string()),
            file_reviews_json: Set(Some(serde_json::to_string(&file_reviews)?)),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(tx)
        .await?,
    )
}
