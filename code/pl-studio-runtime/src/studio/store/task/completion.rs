use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    MergeStatus, ReviewScope, ReviewVerdict, TaskRunPhase, TaskRunRecord, TaskStopOrigin,
    TaskStopReason, TaskWorktreeDisposition, ThreadExecutionStatus, WorkUnitStatus,
};

impl StudioStore {
    pub(crate) async fn request_task_stop(
        &self,
        task_run_id: &str,
        expected_head: &str,
        origin: TaskStopOrigin,
        reason: &TaskStopReason,
    ) -> Result<TaskRunRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found while requesting stop")?;
            let phase = TaskRunPhase::from_str(&run.phase)
                .with_context(|| format!("invalid task phase: {}", run.phase))?;
            if run.expected_head != expected_head || phase.is_terminal() {
                bail!("task stop no longer matches the active task HEAD");
            }
            if run.terminal_generation.is_some() {
                bail!("active task already has a terminal generation");
            }
            validate_lease(&tx, &run, expected_head).await?;
            if run.stop_requested != 0 {
                return super::task_run_record(run);
            }
            let now = unix_seconds();
            let next_generation = run
                .task_generation
                .checked_add(1)
                .context("task generation overflow while requesting stop")?;
            let mut active: entities::task_run::ActiveModel = run.into();
            active.stop_requested = Set(1);
            active.stop_requested_origin = Set(Some(origin.as_str().to_string()));
            active.stop_requested_reason = Set(Some(reason.as_str().to_string()));
            active.stop_requested_at = Set(Some(now));
            active.task_generation = Set(next_generation);
            active.terminal_generation = Set(None);
            active.status_message = Set(Some(format!(
                "task stop requested by {}; settling active turns",
                origin.as_str()
            )));
            active.updated_at = Set(now);
            super::task_run_record(active.update(&tx).await?)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn begin_task_stop(
        &self,
        task_run_id: &str,
        expected_head: &str,
        expected_generation: u64,
    ) -> Result<TaskRunRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found while beginning stop")?;
            let phase = TaskRunPhase::from_str(&run.phase)
                .with_context(|| format!("invalid task phase: {}", run.phase))?;
            if run.expected_head != expected_head || phase.is_terminal() {
                bail!("task stop no longer matches the active task HEAD");
            }
            if run.stop_requested == 0 {
                bail!("task stop must be requested before entering stopping");
            }
            if run.task_generation != i64::try_from(expected_generation)? {
                bail!("task stop generation changed before entering stopping");
            }
            validate_lease(&tx, &run, expected_head).await?;
            if phase == TaskRunPhase::Stopping {
                return super::task_run_record(run);
            }
            if !phase.can_transition_to(TaskRunPhase::Stopping) {
                bail!("task phase cannot transition to stopping");
            }
            let mut active: entities::task_run::ActiveModel = run.into();
            active.phase = Set(TaskRunPhase::Stopping.as_str().to_string());
            active.status_message = Set(Some("task stop is settling agents".to_string()));
            active.updated_at = Set(unix_seconds());
            super::task_run_record(active.update(&tx).await?)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn block_task_and_release_lease(
        &self,
        task_run_id: &str,
        reason: &str,
    ) -> Result<TaskRunRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found while blocking")?;
            let phase = TaskRunPhase::from_str(&run.phase)
                .with_context(|| format!("invalid task phase: {}", run.phase))?;
            if phase.is_terminal() {
                return super::task_run_record(run);
            }
            if !phase.can_transition_to(TaskRunPhase::Blocked) {
                bail!("task phase cannot transition to blocked");
            }
            let blocked = super::write_task_terminal_fact(
                &tx,
                run,
                TaskRunPhase::Blocked,
                Some(reason.to_string()),
                None,
            )
            .await?;
            super::delete_blocked_branch_lease(&tx, task_run_id).await?;
            super::task_run_record(blocked)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn complete_reviewed_task(
        &self,
        thread_id: &str,
        expected_head: &str,
        verification_summary: &str,
    ) -> Result<TaskRunRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_run_for_session(&tx, thread_id).await?;
            if run.phase != TaskRunPhase::Reviewing.as_str()
                || run.expected_head != expected_head
                || run.design_commit.as_deref() != Some(expected_head)
            {
                bail!("task completion requires reviewed design at the current HEAD");
            }
            if run.stop_requested != 0 {
                bail!("task completion is unavailable after stop was requested");
            }
            validate_lease(&tx, &run, expected_head).await?;
            validate_completion_children(&tx, &run).await?;
            let completed = super::write_task_terminal_fact(
                &tx,
                run,
                TaskRunPhase::Completed,
                Some(verification_summary.to_string()),
                None,
            )
            .await?;
            delete_lease(&tx, &completed.id).await?;
            super::task_run_record(completed)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn settle_agents_for_task_stop(
        &self,
        task_run_id: &str,
        expected_generation: u64,
        reason: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found while stopping agents")?;
            if run.phase != TaskRunPhase::Stopping.as_str() || run.stop_requested == 0 {
                bail!("task agents can only be settled after entering requested stopping");
            }
            if run.task_generation != i64::try_from(expected_generation)? {
                bail!("task stop generation changed before settling agents");
            }
            let now = unix_seconds();
            for unit in entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id.to_string()))
                .all(&tx)
                .await?
            {
                let status = WorkUnitStatus::from_str(&unit.status)
                    .with_context(|| format!("invalid work unit status: {}", unit.status))?;
                let cancel = matches!(
                    status,
                    WorkUnitStatus::Pending
                        | WorkUnitStatus::Running
                        | WorkUnitStatus::AwaitingCompletion
                        | WorkUnitStatus::ReadyForReview
                        | WorkUnitStatus::Reviewing
                        | WorkUnitStatus::ChangesRequested
                        | WorkUnitStatus::Approved
                );
                let authorize_cleanup = status != WorkUnitStatus::Merged;
                if cancel || authorize_cleanup {
                    let mut active: entities::work_unit::ActiveModel = unit.into();
                    if cancel {
                        active.status = Set(WorkUnitStatus::Cancelled.as_str().to_string());
                        active.execution_status =
                            Set(ThreadExecutionStatus::Cancelled.as_str().to_string());
                        active.execution_error = Set(Some(reason.to_string()));
                    }
                    if authorize_cleanup {
                        active.worktree_disposition =
                            Set(TaskWorktreeDisposition::CleanupRequested
                                .as_str()
                                .to_string());
                    }
                    active.updated_at = Set(now);
                    active.update(&tx).await?;
                }
            }
            for round in entities::review_round::Entity::find()
                .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
                .filter(entities::review_round::Column::Status.eq(ReviewVerdict::Pending.as_str()))
                .all(&tx)
                .await?
            {
                let mut active: entities::review_round::ActiveModel = round.into();
                active.status = Set(ReviewVerdict::Failed.as_str().to_string());
                active.reviewer_status = Set(ThreadExecutionStatus::Cancelled.as_str().to_string());
                active.reviewer_error = Set(Some(reason.to_string()));
                active.summary = Set(Some(reason.to_string()));
                active.updated_at = Set(now);
                active.update(&tx).await?;
            }
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn cancel_task_and_release_lease(
        &self,
        task_run_id: &str,
        expected_head: &str,
        expected_generation: u64,
        reason: &str,
    ) -> Result<TaskRunRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found")?;
            let phase = TaskRunPhase::from_str(&run.phase)
                .with_context(|| format!("invalid task phase: {}", run.phase))?;
            if phase.is_terminal() {
                if phase == TaskRunPhase::Cancelled
                    && run.terminal_generation == Some(i64::try_from(expected_generation)?)
                {
                    return super::task_run_record(run);
                }
                bail!("task terminal fact belongs to another generation");
            }
            if run.expected_head != expected_head {
                bail!("task stop no longer matches the active task HEAD");
            }
            if phase != TaskRunPhase::Stopping || run.stop_requested == 0 {
                bail!("task cancellation requires requested stopping phase");
            }
            if run.task_generation != i64::try_from(expected_generation)? {
                bail!("task stop generation changed before cancellation");
            }
            validate_lease(&tx, &run, expected_head).await?;
            let merges = entities::merge_record::Entity::find()
                .filter(entities::merge_record::Column::TaskRunId.eq(task_run_id.to_string()))
                .all(&tx)
                .await?;
            if merges.iter().any(|merge| {
                matches!(
                    MergeStatus::from_str(&merge.status),
                    Some(MergeStatus::Pending | MergeStatus::Verifying | MergeStatus::Conflicted)
                )
            }) {
                bail!("task stop requires all merge state to be settled");
            }
            let cancelled = super::write_task_terminal_fact(
                &tx,
                run,
                TaskRunPhase::Cancelled,
                Some(reason.to_string()),
                Some(expected_generation),
            )
            .await?;
            delete_lease(&tx, task_run_id).await?;
            super::task_run_record(cancelled)
        }
        .await;
        finish_transaction(tx, result).await
    }
}

async fn validate_completion_children(
    tx: &sea_orm::DatabaseTransaction,
    run: &entities::task_run::Model,
) -> Result<()> {
    let latest_review = entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
        .order_by_desc(entities::review_round::Column::Round)
        .one(tx)
        .await?
        .context("task completion requires a review round")?;
    if latest_review.scope != ReviewScope::Integrated.as_str()
        || latest_review.reviewed_head != run.expected_head
        || latest_review.status != ReviewVerdict::Pass.as_str()
    {
        bail!("latest review must pass for the current task HEAD");
    }
    let units = entities::work_unit::Entity::find()
        .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
        .all(tx)
        .await?;
    if units.iter().any(|unit| {
        !matches!(
            WorkUnitStatus::from_str(&unit.status),
            Some(WorkUnitStatus::Merged | WorkUnitStatus::NoDelivery)
        )
    }) {
        bail!("all executor deliveries must be terminal and consumed before completion");
    }
    if units.iter().any(|unit| {
        matches!(
            ThreadExecutionStatus::from_str(&unit.execution_status),
            Some(ThreadExecutionStatus::Queued | ThreadExecutionStatus::Running)
        )
    }) {
        bail!("all task agents must be terminal before completion");
    }
    let merges = entities::merge_record::Entity::find()
        .filter(entities::merge_record::Column::TaskRunId.eq(run.id.clone()))
        .all(tx)
        .await?;
    if merges.iter().any(|merge| {
        matches!(
            MergeStatus::from_str(&merge.status),
            Some(MergeStatus::Pending | MergeStatus::Verifying | MergeStatus::Conflicted)
        )
    }) {
        bail!("active merge must finish before completion");
    }
    Ok(())
}

async fn active_run_for_session(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<entities::task_run::Model> {
    let runs = entities::task_run::Entity::find()
        .filter(entities::task_run::Column::RootThreadId.eq(thread_id.to_string()))
        .filter(entities::task_run::Column::Phase.eq(TaskRunPhase::Reviewing.as_str()))
        .all(tx)
        .await?;
    match runs.as_slice() {
        [run] => Ok(run.clone()),
        [] => bail!("reviewing task run not found for completion"),
        _ => bail!("multiple reviewing task runs found for completion"),
    }
}

async fn validate_lease(
    tx: &sea_orm::DatabaseTransaction,
    run: &entities::task_run::Model,
    expected_head: &str,
) -> Result<()> {
    let lease = entities::branch_lease::Entity::find()
        .filter(entities::branch_lease::Column::TaskRunId.eq(run.id.clone()))
        .one(tx)
        .await?
        .context("task branch lease not found")?;
    if lease.expected_head != expected_head
        || lease.branch != run.branch
        || lease.git_common_dir != run.git_common_dir
    {
        bail!("task run and branch lease drifted before terminalization");
    }
    Ok(())
}

async fn delete_lease(tx: &sea_orm::DatabaseTransaction, task_run_id: &str) -> Result<()> {
    let deleted = entities::branch_lease::Entity::delete_many()
        .filter(entities::branch_lease::Column::TaskRunId.eq(task_run_id.to_string()))
        .exec(tx)
        .await?;
    if deleted.rows_affected != 1 {
        bail!("terminalization must release exactly one task branch lease");
    }
    Ok(())
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
