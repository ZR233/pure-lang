use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentOutcomeStatus, MergeStatus, ReviewVerdict, TaskRunPhase, TaskRunRecord, WorkUnitStatus,
};

impl StudioStore {
    pub(crate) async fn complete_reviewed_task(
        &self,
        session_id: &str,
        expected_head: &str,
        verification_summary: &str,
    ) -> Result<TaskRunRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_run_for_session(&tx, session_id).await?;
            if run.phase != TaskRunPhase::Reviewing.as_str()
                || run.expected_head != expected_head
                || run.design_commit.as_deref() != Some(expected_head)
            {
                bail!("task completion requires reviewed design at the current HEAD");
            }
            validate_lease(&tx, &run, expected_head).await?;
            validate_completion_children(&tx, &run).await?;
            let now = unix_seconds();
            let mut active: entities::task_run::ActiveModel = run.into();
            active.phase = Set(TaskRunPhase::Completed.as_str().to_string());
            active.status_message = Set(Some(verification_summary.to_string()));
            active.updated_at = Set(now);
            let completed = active.update(&tx).await?;
            delete_lease(&tx, &completed.id).await?;
            super::task_run_record(completed)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn settle_agents_for_task_stop(
        &self,
        task_run_id: &str,
        reason: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found while stopping agents")?;
            let now = unix_seconds();
            for unit in entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id.to_string()))
                .all(&tx)
                .await?
            {
                let status = WorkUnitStatus::from_str(&unit.status)
                    .with_context(|| format!("invalid work unit status: {}", unit.status))?;
                if matches!(
                    status,
                    WorkUnitStatus::Pending
                        | WorkUnitStatus::Running
                        | WorkUnitStatus::WaitingForDelivery
                ) {
                    let mut active: entities::work_unit::ActiveModel = unit.into();
                    active.status = Set(WorkUnitStatus::Cancelled.as_str().to_string());
                    active.updated_at = Set(now);
                    active.update(&tx).await?;
                }
            }
            for outcome in entities::agent_outcome::Entity::find()
                .filter(entities::agent_outcome::Column::TaskRunId.eq(task_run_id.to_string()))
                .all(&tx)
                .await?
            {
                let status = AgentOutcomeStatus::from_str(&outcome.status)
                    .with_context(|| format!("invalid agent outcome status: {}", outcome.status))?;
                let mut active: entities::agent_outcome::ActiveModel = outcome.into();
                if matches!(
                    status,
                    AgentOutcomeStatus::Queued
                        | AgentOutcomeStatus::Running
                        | AgentOutcomeStatus::WaitingForDelivery
                ) {
                    active.status = Set(AgentOutcomeStatus::Cancelled.as_str().to_string());
                    active.error = Set(Some(reason.to_string()));
                }
                active.terminal_observed = Set(1);
                active.updated_at = Set(now);
                active.update(&tx).await?;
            }
            for round in entities::review_round::Entity::find()
                .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
                .filter(entities::review_round::Column::Status.eq(ReviewVerdict::Pending.as_str()))
                .all(&tx)
                .await?
            {
                let mut active: entities::review_round::ActiveModel = round.into();
                active.status = Set(ReviewVerdict::Failed.as_str().to_string());
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
            if phase.is_terminal() || run.expected_head != expected_head {
                bail!("task stop no longer matches the active task HEAD");
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
            let now = unix_seconds();
            let mut active: entities::task_run::ActiveModel = run.into();
            active.phase = Set(TaskRunPhase::Cancelled.as_str().to_string());
            active.status_message = Set(Some(reason.to_string()));
            active.updated_at = Set(now);
            let cancelled = active.update(&tx).await?;
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
    if latest_review.head_commit != run.expected_head
        || latest_review.status != ReviewVerdict::Pass.as_str()
    {
        bail!("latest review must pass for the current task HEAD");
    }
    let units = entities::work_unit::Entity::find()
        .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
        .all(tx)
        .await?;
    if units.iter().any(|unit| {
        matches!(
            WorkUnitStatus::from_str(&unit.status),
            Some(
                WorkUnitStatus::Pending
                    | WorkUnitStatus::Running
                    | WorkUnitStatus::WaitingForDelivery
                    | WorkUnitStatus::Delivered
            )
        )
    }) {
        bail!("all executor deliveries must be terminal and consumed before completion");
    }
    let outcomes = entities::agent_outcome::Entity::find()
        .filter(entities::agent_outcome::Column::TaskRunId.eq(run.id.clone()))
        .all(tx)
        .await?;
    if outcomes.iter().any(|outcome| {
        matches!(
            AgentOutcomeStatus::from_str(&outcome.status),
            Some(
                AgentOutcomeStatus::Queued
                    | AgentOutcomeStatus::Running
                    | AgentOutcomeStatus::WaitingForDelivery
            )
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
    session_id: &str,
) -> Result<entities::task_run::Model> {
    let runs = entities::task_run::Entity::find()
        .filter(entities::task_run::Column::SessionId.eq(session_id.to_string()))
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
