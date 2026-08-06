use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    MergeCleanupEvidence, MergeMethod, MergeRecord, RecordTaskMerge, ReviewVerdict, TaskRunPhase,
    TaskWorktreeDisposition, ThreadExecutionStatus, WorkCompletionKind, WorkCompletionStatus,
    WorkUnitStatus,
};

impl StudioStore {
    pub(crate) async fn record_task_merge(&self, input: RecordTaskMerge) -> Result<MergeRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let runs = entities::task_run::Entity::find()
                .filter(entities::task_run::Column::RootThreadId.eq(input.thread_id.clone()))
                .filter(entities::task_run::Column::Phase.eq(TaskRunPhase::Merging.as_str()))
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
            if work_unit.task_run_id != run.id
                || work_unit.executor_thread_id.as_deref() != Some(input.executor_agent_id.as_str())
                || work_unit.status != WorkUnitStatus::Approved.as_str()
                || work_unit.execution_status != ThreadExecutionStatus::Completed.as_str()
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
                expected_previous_head: Set(input.expected_previous_head),
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

            let mut work_unit_active: entities::work_unit::ActiveModel = work_unit.into();
            work_unit_active.status = Set(WorkUnitStatus::Merged.as_str().to_string());
            work_unit_active.worktree_disposition = Set(TaskWorktreeDisposition::CleanupRequested
                .as_str()
                .to_string());
            work_unit_active.updated_at = Set(now);
            work_unit_active.update(&tx).await?;

            let remaining_approved = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
                .filter(entities::work_unit::Column::Status.eq(WorkUnitStatus::Approved.as_str()))
                .one(&tx)
                .await?
                .is_some();
            let prior_rework = entities::review_round::Entity::find()
                .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
                .filter(
                    entities::review_round::Column::Status
                        .eq(ReviewVerdict::ChangesRequired.as_str()),
                )
                .one(&tx)
                .await?
                .is_some();
            let next_phase = if remaining_approved {
                TaskRunPhase::Merging
            } else if prior_rework {
                TaskRunPhase::Reworking
            } else {
                TaskRunPhase::Implementing
            };
            let mut run_active: entities::task_run::ActiveModel = run.into();
            run_active.phase = Set(next_phase.as_str().to_string());
            run_active.expected_head = Set(input.resulting_head.clone());
            run_active.status_message = Set(None);
            run_active.updated_at = Set(now);
            run_active.update(&tx).await?;

            let mut lease_active: entities::branch_lease::ActiveModel = lease.into();
            lease_active.expected_head = Set(input.resulting_head);
            lease_active.updated_at = Set(now);
            lease_active.update(&tx).await?;

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

    #[cfg(test)]
    pub(crate) async fn create_test_merge_record(
        &self,
        task_run_id: &str,
        expected_previous_head: &str,
        resulting_head: &str,
    ) -> Result<MergeRecord> {
        let tx = self.db.begin().await?;
        let now = unix_seconds();
        let agent_id = new_id("test-executor");
        let work_unit_id = new_id("test-work-unit");
        let completion_id = new_id("test-completion");
        entities::work_unit::ActiveModel {
            id: Set(work_unit_id.clone()),
            task_run_id: Set(task_run_id.to_string()),
            title: Set("test recorded merge".to_string()),
            status: Set(WorkUnitStatus::Merged.as_str().to_string()),
            scope_hints_json: Set("[]".to_string()),
            base_commit: Set(expected_previous_head.to_string()),
            worktree_path: Set(".pure/worktrees/test/test".to_string()),
            branch: Set("pure-task-test-test".to_string()),
            worktree_disposition: Set(TaskWorktreeDisposition::CleanupRequested
                .as_str()
                .to_string()),
            attempt: Set(1),
            executor_thread_id: Set(Some(agent_id.clone())),
            requested_by_call_id: Set(new_id("test-call")),
            execution_status: Set(ThreadExecutionStatus::Completed.as_str().to_string()),
            execution_summary: Set(Some("test delivery".to_string())),
            execution_error: Set(None),
            budget_limit_json: Set(None),
            budget_slice_count: Set(1),
            continuation_state: Set(
                crate::studio::task_coordinator::ExecutorContinuationState::None
                    .as_str()
                    .to_string(),
            ),
            continuation_source_turn_id: Set(None),
            continuation_revision: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&tx)
        .await?;
        entities::work_completion::ActiveModel {
            id: Set(completion_id.clone()),
            task_run_id: Set(task_run_id.to_string()),
            work_unit_id: Set(work_unit_id.clone()),
            executor_agent_id: Set(agent_id.clone()),
            revision: Set(1),
            kind: Set(WorkCompletionKind::Delivery.as_str().to_string()),
            status: Set(WorkCompletionStatus::Approved.as_str().to_string()),
            base_commit: Set(expected_previous_head.to_string()),
            head_commit: Set(Some(resulting_head.to_string())),
            changed_files_json: Set("[]".to_string()),
            verification_summary: Set("test delivery".to_string()),
            worktree_path: Set(".pure/worktrees/test/test".to_string()),
            branch: Set("pure-task-test-test".to_string()),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&tx)
        .await?;
        let model = entities::merge_record::ActiveModel {
            id: Set(new_id("merge")),
            task_run_id: Set(task_run_id.to_string()),
            work_unit_id: Set(work_unit_id),
            completion_id: Set(completion_id),
            completion_revision: Set(1),
            executor_agent_id: Set(agent_id),
            expected_previous_head: Set(expected_previous_head.to_string()),
            resulting_head: Set(resulting_head.to_string()),
            delivery_head: Set(resulting_head.to_string()),
            method: Set(MergeMethod::Manual.as_str().to_string()),
            summary: Set("test recorded merge".to_string()),
            cleanup_status: Set("alreadyAbsent".to_string()),
            cleanup_detail: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&tx)
        .await?;
        tx.commit().await?;
        merge_record(model)
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
