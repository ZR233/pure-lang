use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

use super::super::work_completion::{delivery_from_completion, work_completion_record};
use super::super::work_unit::work_unit_record;
use super::super::{branch_lease_record, task_run_record};
use super::{merge_record, parse_required_evidence};
use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    MergeCleanupEvidence, MergeRecord, MergeStatus, TaskMergeScope, TaskRunPhase,
    ThreadExecutionStatus, WorkCompletionStatus, WorkUnitStatus,
};

impl StudioStore {
    pub(crate) async fn find_accepted_merge_scope(
        &self,
        thread_id: &str,
        agent_id: &str,
        expected_head: &str,
    ) -> Result<Option<TaskMergeScope>> {
        let Some(run) = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::RootThreadId.eq(thread_id.to_string()))
            .filter(entities::task_run::Column::Phase.is_in([
                TaskRunPhase::Implementing.as_str(),
                TaskRunPhase::Reworking.as_str(),
            ]))
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        let merges = entities::merge_record::Entity::find()
            .filter(entities::merge_record::Column::TaskRunId.eq(run.id.clone()))
            .filter(entities::merge_record::Column::AgentId.eq(agent_id.to_string()))
            .filter(entities::merge_record::Column::Status.eq(MergeStatus::Merged.as_str()))
            .filter(entities::merge_record::Column::ExpectedHead.eq(expected_head.to_string()))
            .all(&self.db)
            .await?;
        let merge = match merges.as_slice() {
            [] => return Ok(None),
            [merge] => merge.clone(),
            _ => bail!("multiple accepted merges found for executor delivery"),
        };
        self.accepted_merge_scope(run, merge).await.map(Some)
    }

    pub(crate) async fn read_accepted_merge_scope(&self, merge_id: &str) -> Result<TaskMergeScope> {
        let merge = entities::merge_record::Entity::find_by_id(merge_id.to_string())
            .one(&self.db)
            .await?
            .context("accepted merge record not found")?;
        if merge.status != MergeStatus::Merged.as_str() {
            bail!("cleanup replay requires an accepted merge record");
        }
        let run = entities::task_run::Entity::find_by_id(merge.task_run_id.clone())
            .one(&self.db)
            .await?
            .context("accepted merge task run not found")?;
        self.accepted_merge_scope(run, merge).await
    }

    async fn accepted_merge_scope(
        &self,
        run: entities::task_run::Model,
        merge: entities::merge_record::Model,
    ) -> Result<TaskMergeScope> {
        let evidence = parse_required_evidence(merge.verification_json.as_deref())?;
        evidence
            .merge_commit
            .as_deref()
            .context("accepted merge evidence has no merge commit")?;
        let lease = entities::branch_lease::Entity::find()
            .filter(entities::branch_lease::Column::TaskRunId.eq(run.id.clone()))
            .one(&self.db)
            .await?
            .context("accepted merge branch lease not found")?;
        if lease.expected_head != run.expected_head
            || lease.branch != run.branch
            || lease.git_common_dir != run.git_common_dir
        {
            bail!("accepted merge branch lease no longer matches the task head");
        }
        let work_unit = entities::work_unit::Entity::find_by_id(evidence.work_unit_id)
            .one(&self.db)
            .await?
            .context("accepted merge work unit not found")?;
        let completion =
            entities::work_completion::Entity::find_by_id(evidence.completion_id.clone())
                .one(&self.db)
                .await?
                .context("accepted merge completion not found")?;
        if work_unit.task_run_id != run.id
            || work_unit.executor_thread_id.as_deref() != Some(merge.agent_id.as_str())
            || work_unit.execution_status != ThreadExecutionStatus::Completed.as_str()
            || work_unit.status != WorkUnitStatus::Merged.as_str()
            || completion.task_run_id != run.id
            || completion.work_unit_id != work_unit.id
            || completion.executor_agent_id != merge.agent_id
            || completion.revision != i32::try_from(evidence.completion_revision)?
            || completion.status != WorkCompletionStatus::Approved.as_str()
        {
            bail!("accepted merge work unit and completion identity drifted");
        }
        let completion = work_completion_record(completion)?;
        let delivery = delivery_from_completion(&completion)?;
        if delivery.head_commit != evidence.delivery_head {
            bail!("accepted merge delivery head drifted");
        }
        Ok(TaskMergeScope {
            run: task_run_record(run)?,
            lease: branch_lease_record(lease),
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
        if merge.status != MergeStatus::Merged.as_str() {
            bail!("cleanup evidence requires an accepted merge");
        }
        let mut evidence = parse_required_evidence(merge.verification_json.as_deref())?;
        evidence.cleanup = Some(cleanup);
        let mut active: entities::merge_record::ActiveModel = merge.into();
        active.verification_json = Set(Some(serde_json::to_string(&evidence)?));
        active.updated_at = Set(unix_seconds());
        merge_record(active.update(&self.db).await?)
    }

    pub(crate) async fn block_accepted_merge(
        &self,
        merge_id: &str,
        reason: &str,
        cleanup: MergeCleanupEvidence,
    ) -> Result<MergeRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let merge = entities::merge_record::Entity::find_by_id(merge_id.to_string())
                .one(&tx)
                .await?
                .context("merge record not found")?;
            if merge.status != MergeStatus::Merged.as_str() {
                bail!("accepted-merge block requires a merged record");
            }
            let mut evidence = parse_required_evidence(merge.verification_json.as_deref())?;
            evidence.cleanup = Some(cleanup);
            let now = unix_seconds();
            let task_run_id = merge.task_run_id.clone();
            let mut merge_active: entities::merge_record::ActiveModel = merge.into();
            merge_active.verification_json = Set(Some(serde_json::to_string(&evidence)?));
            merge_active.updated_at = Set(now);
            let merge = merge_record(merge_active.update(&tx).await?)?;

            let run = entities::task_run::Entity::find_by_id(task_run_id.clone())
                .one(&tx)
                .await?
                .context("accepted task run not found")?;
            super::super::write_task_terminal_fact(
                &tx,
                run,
                TaskRunPhase::Blocked,
                Some(reason.to_string()),
                None,
            )
            .await?;
            super::super::delete_blocked_branch_lease(&tx, &task_run_id).await?;
            Ok(merge)
        }
        .await;
        match result {
            Ok(merge) => {
                tx.commit().await?;
                Ok(merge)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }
}
