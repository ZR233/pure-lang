use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

use super::super::outcome::agent_outcome_record;
use super::super::work_unit::work_unit_record;
use super::super::{branch_lease_record, task_run_record};
use super::{merge_record, parse_required_evidence};
use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentDelivery, MergeCleanupEvidence, MergeRecord, MergeStatus, TaskMergeScope, TaskRunPhase,
};

impl StudioStore {
    pub(crate) async fn find_accepted_merge_scope(
        &self,
        session_id: &str,
        agent_id: &str,
        expected_head: &str,
    ) -> Result<Option<TaskMergeScope>> {
        let Some(run) = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::SessionId.eq(session_id.to_string()))
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
        let outcome = entities::agent_outcome::Entity::find_by_id(evidence.outcome_id)
            .one(&self.db)
            .await?
            .context("accepted merge outcome not found")?;
        let delivery: AgentDelivery = serde_json::from_str(
            outcome
                .delivery_json
                .as_deref()
                .context("accepted merge delivery disappeared")?,
        )?;
        if work_unit.task_run_id != run.id
            || outcome.task_run_id != run.id
            || outcome.work_unit_id.as_deref() != Some(work_unit.id.as_str())
            || outcome.agent_id != merge.agent_id
            || work_unit.agent_id.as_deref() != Some(merge.agent_id.as_str())
            || delivery.head_commit != evidence.delivery_head
        {
            bail!("accepted merge work unit, outcome, and delivery identity drifted");
        }
        Ok(TaskMergeScope {
            #[cfg(test)]
            origin_phase: TaskRunPhase::from_str(&run.phase)
                .context("accepted merge run has invalid phase")?,
            run: task_run_record(run)?,
            lease: branch_lease_record(lease),
            work_unit: work_unit_record(work_unit)?,
            outcome: agent_outcome_record(outcome)?,
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

            let run = entities::task_run::Entity::find_by_id(task_run_id)
                .one(&tx)
                .await?
                .context("accepted task run not found")?;
            let mut run_active: entities::task_run::ActiveModel = run.into();
            run_active.phase = Set(TaskRunPhase::Blocked.as_str().to_string());
            run_active.status_message = Set(Some(reason.to_string()));
            run_active.updated_at = Set(now);
            run_active.update(&tx).await?;
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
