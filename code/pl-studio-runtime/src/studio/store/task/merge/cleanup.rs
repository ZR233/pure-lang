use anyhow::{Context, Result, bail};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};

use super::super::task_run_record;
use super::super::work_completion::{delivery_from_completion, work_completion_record};
use super::super::work_unit::{work_unit_record, work_unit_state};
use super::merge_record;
use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    MergeCleanupEvidence, MergeRecord, TaskMergeScope, TaskWorktreeDisposition,
    ThreadExecutionStatus, WorkCompletionStatus, WorkUnitStatus,
};

impl StudioStore {
    pub(crate) async fn read_accepted_merge_scope(&self, merge_id: &str) -> Result<TaskMergeScope> {
        let merge = entities::merge_record::Entity::find_by_id(merge_id.to_string())
            .one(&self.db)
            .await?
            .context("recorded merge not found")?;
        let run = entities::task_run::Entity::find_by_id(merge.task_run_id.clone())
            .one(&self.db)
            .await?
            .context("recorded merge task run not found")?;
        let lease = entities::branch_lease::Entity::find()
            .filter(entities::branch_lease::Column::TaskRunId.eq(run.id.clone()))
            .one(&self.db)
            .await?
            .context("recorded merge branch lease not found")?;
        if lease.expected_head != run.expected_head
            || lease.branch != run.branch
            || lease.git_common_dir != run.git_common_dir
        {
            bail!("recorded merge branch lease no longer matches the task head");
        }
        let work_unit = entities::work_unit::Entity::find_by_id(merge.work_unit_id.clone())
            .one(&self.db)
            .await?
            .context("recorded merge work unit not found")?;
        let completion = entities::work_completion::Entity::find_by_id(merge.completion_id.clone())
            .one(&self.db)
            .await?
            .context("recorded merge completion not found")?;
        let work_unit_state = work_unit_state(&work_unit)?;
        if work_unit.task_run_id != run.id
            || work_unit.executor_thread_id.as_deref() != Some(merge.executor_agent_id.as_str())
            || work_unit_state.execution_status() != ThreadExecutionStatus::Completed
            || work_unit_state.status() != WorkUnitStatus::Merged
            || work_unit_state.progress().worktree_disposition
                != TaskWorktreeDisposition::CleanupRequested
            || completion.task_run_id != run.id
            || completion.work_unit_id != work_unit.id
            || completion.executor_agent_id != merge.executor_agent_id
            || completion.revision != merge.completion_revision
            || completion.status != WorkCompletionStatus::Approved.as_str()
        {
            bail!("recorded merge work unit and completion identity drifted");
        }
        let completion = work_completion_record(completion)?;
        let delivery = delivery_from_completion(&completion)?;
        if delivery.head_commit != merge.delivery_head {
            bail!("recorded merge delivery head drifted");
        }
        Ok(TaskMergeScope {
            run: task_run_record(run)?,
            work_unit: work_unit_record(work_unit)?,
            completion,
            delivery,
            merge: merge_record(merge)?,
        })
    }

    pub(crate) async fn record_merge_cleanup_attempting(
        &self,
        merge_id: &str,
    ) -> Result<MergeRecord> {
        self.record_merge_cleanup(
            merge_id,
            MergeCleanupEvidence {
                status: "attempting".to_string(),
                detail: None,
            },
        )
        .await
    }

    pub(crate) async fn record_merge_cleanup(
        &self,
        merge_id: &str,
        cleanup: MergeCleanupEvidence,
    ) -> Result<MergeRecord> {
        let merge = entities::merge_record::Entity::find_by_id(merge_id.to_string())
            .one(&self.db)
            .await?
            .context("merge record not found")?;
        let mut active: entities::merge_record::ActiveModel = merge.into();
        active.cleanup_status = Set(cleanup.status);
        active.cleanup_detail = Set(cleanup.detail);
        active.updated_at = Set(unix_seconds());
        merge_record(active.update(&self.db).await?)
    }
}
