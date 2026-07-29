use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

use super::{merge_record, parse_required_evidence};
use crate::agent::worktree::same_worktree_path;
use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AbortConflictMerge, AgentDelivery, AgentOutcomeStatus, CompleteConflictMerge,
    ConflictVerificationEvidence, MergeRecord, MergeStatus, RecordConflictVerification,
    TaskRunPhase, WorkUnitStatus,
};

impl StudioStore {
    pub(crate) async fn record_conflict_verification(
        &self,
        input: RecordConflictVerification,
    ) -> Result<MergeRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let merge = entities::merge_record::Entity::find_by_id(input.merge_id)
                .one(&tx)
                .await?
                .context("merge record not found")?;
            if merge.status != MergeStatus::Conflicted.as_str()
                || merge.expected_head != input.expected_head
            {
                bail!("merge record no longer matches the conflict verification scope");
            }
            let task_run_id = merge.task_run_id.clone();
            let run = entities::task_run::Entity::find_by_id(task_run_id.clone())
                .one(&tx)
                .await?
                .context("task run not found")?;
            if run.phase != TaskRunPhase::ResolvingConflict.as_str()
                || run.expected_head != input.expected_head
            {
                bail!("task run no longer matches the conflict verification scope");
            }
            let attempt = u32::try_from(merge.attempt)? + 1;
            if attempt > 3 {
                bail!("conflict resolution exceeded the three-attempt limit");
            }
            let mut evidence = parse_required_evidence(merge.verification_json.as_deref())?;
            if evidence.conflict_manifest.is_none() {
                bail!("conflicted merge has no durable manifest");
            }
            evidence.verification_steps = input.steps.clone();
            evidence.conflict_verification = Some(ConflictVerificationEvidence {
                attempt,
                success: input.success,
                index_tree: input.index_tree,
                steps: input.steps,
                diagnostic: input.diagnostic,
            });
            let mut active: entities::merge_record::ActiveModel = merge.into();
            active.verification_json = Set(Some(serde_json::to_string(&evidence)?));
            active.attempt = Set(i32::try_from(attempt)?);
            active.updated_at = Set(unix_seconds());
            merge_record(active.update(&tx).await?)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn abort_conflict_merge(
        &self,
        input: AbortConflictMerge,
    ) -> Result<MergeRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let merge = entities::merge_record::Entity::find_by_id(input.merge_id)
                .one(&tx)
                .await?
                .context("merge record not found")?;
            if merge.status != MergeStatus::Conflicted.as_str()
                || merge.expected_head != input.expected_head
            {
                bail!("merge record no longer matches the conflict abort scope");
            }
            let mut evidence = parse_required_evidence(merge.verification_json.as_deref())?;
            evidence.compensation = Some(input.compensation);
            let task_run_id = merge.task_run_id.clone();
            let run = entities::task_run::Entity::find_by_id(task_run_id.clone())
                .one(&tx)
                .await?
                .context("task run not found")?;
            if run.phase != TaskRunPhase::ResolvingConflict.as_str()
                || run.expected_head != input.expected_head
            {
                bail!("task run no longer matches the conflict abort scope");
            }
            let now = unix_seconds();
            let mut merge_active: entities::merge_record::ActiveModel = merge.into();
            merge_active.status = Set(MergeStatus::Aborted.as_str().to_string());
            merge_active.resolution_summary = Set(Some(input.reason.clone()));
            merge_active.verification_json = Set(Some(serde_json::to_string(&evidence)?));
            merge_active.updated_at = Set(now);
            let merge = merge_record(merge_active.update(&tx).await?)?;
            super::super::write_task_terminal_fact(
                &tx,
                run,
                TaskRunPhase::Blocked,
                Some(input.reason),
                None,
            )
            .await?;
            super::super::delete_blocked_branch_lease(&tx, &task_run_id).await?;
            Ok(merge)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn complete_conflict_merge(
        &self,
        input: CompleteConflictMerge,
    ) -> Result<MergeRecord> {
        let tx = self.db.begin().await?;
        let result = complete_conflict_merge_transaction(&tx, input).await;
        finish_transaction(tx, result).await
    }
}

async fn complete_conflict_merge_transaction(
    tx: &sea_orm::DatabaseTransaction,
    input: CompleteConflictMerge,
) -> Result<MergeRecord> {
    let merge = entities::merge_record::Entity::find_by_id(input.merge_id)
        .one(tx)
        .await?
        .context("merge record not found")?;
    if merge.status != MergeStatus::Conflicted.as_str()
        || merge.expected_head != input.expected_head
    {
        bail!("merge record no longer matches the conflict completion scope");
    }
    let mut evidence = parse_required_evidence(merge.verification_json.as_deref())?;
    let verification = evidence
        .conflict_verification
        .as_ref()
        .context("conflict merge has not been verified")?;
    if !verification.success
        || verification.index_tree.is_none()
        || verification.attempt != u32::try_from(merge.attempt)?
        || verification.attempt > 3
    {
        bail!("conflict merge does not have a current successful verification");
    }
    let run = entities::task_run::Entity::find_by_id(merge.task_run_id.clone())
        .one(tx)
        .await?
        .context("task run not found")?;
    if run.phase != TaskRunPhase::ResolvingConflict.as_str()
        || run.expected_head != input.expected_head
    {
        bail!("task run no longer matches the conflict completion scope");
    }
    let lease = entities::branch_lease::Entity::find()
        .filter(entities::branch_lease::Column::TaskRunId.eq(run.id.clone()))
        .one(tx)
        .await?
        .context("task branch lease not found")?;
    if lease.expected_head != input.expected_head
        || lease.branch != run.branch
        || lease.git_common_dir != run.git_common_dir
    {
        bail!("branch lease no longer matches the conflict completion scope");
    }
    let work_unit = entities::work_unit::Entity::find_by_id(evidence.work_unit_id.clone())
        .one(tx)
        .await?
        .context("merge work unit not found")?;
    let outcome = entities::agent_outcome::Entity::find_by_id(evidence.outcome_id.clone())
        .one(tx)
        .await?
        .context("merge outcome not found")?;
    let delivery: AgentDelivery = serde_json::from_str(
        outcome
            .delivery_json
            .as_deref()
            .context("completed outcome delivery disappeared")?,
    )?;
    if work_unit.task_run_id != run.id
        || work_unit.agent_id.as_deref() != Some(merge.agent_id.as_str())
        || work_unit.status != WorkUnitStatus::Delivered.as_str()
        || outcome.task_run_id != run.id
        || outcome.work_unit_id.as_deref() != Some(work_unit.id.as_str())
        || outcome.agent_id != merge.agent_id
        || outcome.status != AgentOutcomeStatus::Completed.as_str()
        || delivery.head_commit != merge.source_commit
        || evidence.delivery_head != merge.source_commit
        || !same_worktree_path(&delivery.worktree.path, &work_unit.worktree_path)
        || delivery.worktree.branch != work_unit.branch
        || delivery.base_commit != work_unit.base_commit
        || delivery.changed_files != evidence.changed_files
    {
        bail!("delivered executor identity changed before conflict merge acceptance");
    }
    let now = unix_seconds();
    evidence.merge_commit = Some(input.merge_commit.clone());
    evidence.conflict_continuation_requested = false;
    evidence.merge_completion_continuation_requested = false;
    let mut merge_active: entities::merge_record::ActiveModel = merge.into();
    merge_active.status = Set(MergeStatus::Merged.as_str().to_string());
    merge_active.resolution_summary = Set(Some(input.resolution_summary));
    merge_active.verification_json = Set(Some(serde_json::to_string(&evidence)?));
    merge_active.updated_at = Set(now);
    let merged = merge_record(merge_active.update(tx).await?)?;
    let mut work_unit_active: entities::work_unit::ActiveModel = work_unit.into();
    work_unit_active.status = Set(WorkUnitStatus::Merged.as_str().to_string());
    work_unit_active.updated_at = Set(now);
    work_unit_active.update(tx).await?;
    let mut run_active: entities::task_run::ActiveModel = run.into();
    run_active.phase = Set(evidence.origin_phase.as_str().to_string());
    run_active.expected_head = Set(input.merge_commit.clone());
    run_active.status_message = Set(None);
    run_active.updated_at = Set(now);
    run_active.update(tx).await?;
    let mut lease_active: entities::branch_lease::ActiveModel = lease.into();
    lease_active.expected_head = Set(input.merge_commit);
    lease_active.updated_at = Set(now);
    lease_active.update(tx).await?;
    Ok(merged)
}

async fn finish_transaction<T>(tx: sea_orm::DatabaseTransaction, result: Result<T>) -> Result<T> {
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
