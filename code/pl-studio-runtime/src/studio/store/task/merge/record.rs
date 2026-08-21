use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use super::super::work_unit::{update_work_unit_state, work_unit_state};
use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    MergeCleanupEvidence, MergeMethod, MergeRecord, RecordTaskMerge, ReviewVerdict, TaskCommand,
    TaskRunStateKind, TaskWorktreeDisposition, ThreadExecutionStatus, WorkCompletionKind,
    WorkCompletionStatus, WorkUnitState, WorkUnitStatus,
};

impl StudioStore {
    pub(crate) async fn record_task_merge(&self, input: RecordTaskMerge) -> Result<MergeRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let runs = entities::task_run::Entity::find()
                .filter(entities::task_run::Column::RootThreadId.eq(input.thread_id.clone()))
                .filter(
                    entities::task_run::Column::StateKind.eq(TaskRunStateKind::Merging.as_str()),
                )
                .all(&tx)
                .await?;
            let run = match runs.as_slice() {
                [run] => run.clone(),
                [] => bail!("merging TaskRun not found"),
                _ => bail!("multiple merging TaskRuns found"),
            };
            if run.expected_head != input.expected_previous_head {
                bail!("TaskRun head changed before merge accounting");
            }
            let lease = entities::branch_lease::Entity::find()
                .filter(entities::branch_lease::Column::TaskRunId.eq(run.id.clone()))
                .one(&tx)
                .await?
                .context("task branch lease not found")?;
            if lease.expected_head != input.expected_previous_head
                || lease.branch != run.branch
                || lease.git_common_dir != run.git_common_dir
            {
                bail!("BranchLease changed before merge accounting");
            }

            let work_unit = entities::work_unit::Entity::find_by_id(input.work_unit_id.clone())
                .one(&tx)
                .await?
                .context("merge candidate WorkUnit not found")?;
            let completion =
                entities::work_completion::Entity::find_by_id(input.completion_id.clone())
                    .one(&tx)
                    .await?
                    .context("merge candidate Completion not found")?;
            let executor = entities::thread::Entity::find_by_id(input.executor_agent_id.clone())
                .one(&tx)
                .await?
                .context("executor canonical Thread not found")?;
            if executor.role != "executor" || executor.status != "closed" {
                bail!("executor must remain canonically closed during merge accounting");
            }
            let work_unit_state = work_unit_state(&work_unit)?;
            if work_unit.task_run_id != run.id
                || work_unit.executor_thread_id.as_deref() != Some(input.executor_agent_id.as_str())
                || work_unit_state.status() != WorkUnitStatus::Approved
                || work_unit_state.execution_status() != ThreadExecutionStatus::Completed
                || completion.task_run_id != run.id
                || completion.work_unit_id != work_unit.id
                || completion.executor_agent_id != input.executor_agent_id
                || completion.revision != i32::try_from(input.completion_revision)?
                || completion.kind != WorkCompletionKind::Delivery.as_str()
                || completion.status != WorkCompletionStatus::Approved.as_str()
            {
                bail!("approved Completion changed before merge accounting");
            }
            let delivery_head = completion
                .head_commit
                .clone()
                .context("approved delivery Completion has no head commit")?;
            if entities::merge_record::Entity::find()
                .filter(entities::merge_record::Column::TaskRunId.eq(run.id.clone()))
                .filter(entities::merge_record::Column::CompletionId.eq(completion.id.clone()))
                .one(&tx)
                .await?
                .is_some()
            {
                bail!("executor Completion already has a recorded merge");
            }

            let now = unix_seconds();
            let merge = entities::merge_record::ActiveModel {
                id: Set(new_id("merge")),
                task_run_id: Set(run.id.clone()),
                work_unit_id: Set(work_unit.id.clone()),
                completion_id: Set(completion.id),
                completion_revision: Set(i32::try_from(input.completion_revision)?),
                executor_agent_id: Set(input.executor_agent_id),
                expected_previous_head: Set(input.expected_previous_head.clone()),
                resulting_head: Set(input.resulting_head.clone()),
                delivery_head: Set(delivery_head),
                method: Set(input.method.as_str().to_string()),
                summary: Set(input.summary),
                cleanup_status: Set("pending".to_string()),
                cleanup_detail: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await?;

            let mut progress = work_unit_state.into_progress();
            progress.worktree_disposition = TaskWorktreeDisposition::CleanupRequested;
            update_work_unit_state(&tx, work_unit, WorkUnitState::merged(progress)).await?;

            let remaining_approved = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
                .filter(
                    entities::work_unit::Column::StateKind.eq(WorkUnitStatus::Approved.as_str()),
                )
                .one(&tx)
                .await?
                .is_some();
            let prior_rework = entities::review_round::Entity::find()
                .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
                .filter(
                    entities::review_round::Column::StateKind
                        .eq(ReviewVerdict::ChangesRequired.as_str()),
                )
                .one(&tx)
                .await?
                .is_some();
            let record = super::super::task_run_record(run.clone())?;
            let next_state = if remaining_approved {
                record.state.clone()
            } else if prior_rework {
                record
                    .decide(TaskCommand::BeginReworking {
                        status_message: "merged delivery still requires rework".to_string(),
                    })?
                    .next_state
            } else {
                record.decide(TaskCommand::BeginImplementing)?.next_state
            };
            super::super::compare_and_swap_task_run(
                &tx,
                &run,
                Some(&next_state),
                Some(&input.resulting_head),
            )
            .await?
            .context("TaskRun merge accounting lost its revision CAS")?;

            let lease_update = entities::branch_lease::Entity::update_many()
                .set(entities::branch_lease::ActiveModel {
                    expected_head: Set(input.resulting_head),
                    updated_at: Set(now),
                    ..Default::default()
                })
                .filter(entities::branch_lease::Column::Id.eq(lease.id))
                .filter(
                    entities::branch_lease::Column::ExpectedHead.eq(input.expected_previous_head),
                )
                .exec(&tx)
                .await?;
            if lease_update.rows_affected != 1 {
                bail!("BranchLease merge accounting lost its expected-head CAS");
            }

            merge_record(merge)
        }
        .await;
        match result {
            Ok(record) => {
                tx.commit().await?;
                Ok(record)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn list_merge_records(&self, task_run_id: &str) -> Result<Vec<MergeRecord>> {
        entities::merge_record::Entity::find()
            .filter(entities::merge_record::Column::TaskRunId.eq(task_run_id.to_string()))
            .order_by_asc(entities::merge_record::Column::CreatedAt)
            .order_by_asc(entities::merge_record::Column::Id)
            .all(&self.db)
            .await?
            .into_iter()
            .map(merge_record)
            .collect()
    }
}

pub(crate) fn merge_record(model: entities::merge_record::Model) -> Result<MergeRecord> {
    Ok(MergeRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        work_unit_id: model.work_unit_id,
        completion_id: model.completion_id,
        completion_revision: u32::try_from(model.completion_revision)?,
        executor_agent_id: model.executor_agent_id,
        expected_previous_head: model.expected_previous_head,
        resulting_head: model.resulting_head,
        delivery_head: model.delivery_head,
        method: MergeMethod::from_str(&model.method)
            .with_context(|| format!("invalid merge method: {}", model.method))?,
        summary: model.summary,
        cleanup: MergeCleanupEvidence {
            status: model.cleanup_status,
            detail: model.cleanup_detail,
        },
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
