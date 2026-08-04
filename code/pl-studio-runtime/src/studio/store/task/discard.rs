use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentOutcomeStatus, ExecutorCloseDisposition, TaskRunPhase, TaskStopOrigin,
    TaskWorktreeDisposition, WorkUnitStatus,
};

struct ExecutorCloseScope {
    work_unit: entities::work_unit::Model,
    outcome: entities::agent_outcome::Model,
}

#[derive(Clone, Copy)]
struct ExecutorClosePlan {
    disposition: ExecutorCloseDisposition,
    cancel_active: bool,
}

impl StudioStore {
    pub(crate) async fn authorize_recovery_cleanup(&self, task_run_id: &str) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("recovery cleanup task run not found")?;
            let phase = TaskRunPhase::from_str(&run.phase)
                .with_context(|| format!("invalid task phase: {}", run.phase))?;
            let now = unix_seconds();
            if !phase.is_terminal() {
                let next_generation = run
                    .task_generation
                    .checked_add(1)
                    .context("task generation overflow while authorizing recovery cleanup")?;
                let mut active: entities::task_run::ActiveModel = run.into();
                active.stop_requested = Set(1);
                active.stop_requested_origin =
                    Set(Some(TaskStopOrigin::UserRequest.as_str().to_string()));
                active.stop_requested_reason =
                    Set(Some("recovery cleanup requested by user".to_string()));
                active.stop_requested_at = Set(Some(now));
                active.task_generation = Set(next_generation);
                active.updated_at = Set(now);
                let run = active.update(&tx).await?;
                super::write_task_terminal_fact(
                    &tx,
                    run,
                    TaskRunPhase::Cancelled,
                    Some("recovery cleanup requested by user".to_string()),
                    Some(u64::try_from(next_generation)?),
                )
                .await?;
            }

            let work_units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id.to_string()))
                .all(&tx)
                .await?;
            for work_unit in work_units {
                let mut active: entities::work_unit::ActiveModel = work_unit.into();
                active.worktree_disposition = Set(TaskWorktreeDisposition::CleanupRequested
                    .as_str()
                    .to_string());
                active.updated_at = Set(now);
                active.update(&tx).await?;
            }
            entities::branch_lease::Entity::delete_many()
                .filter(entities::branch_lease::Column::TaskRunId.eq(task_run_id.to_string()))
                .exec(&tx)
                .await?;
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn settle_executor_close(
        &self,
        session_id: &str,
        work_unit_id: &str,
        agent_id: &str,
    ) -> Result<ExecutorCloseDisposition> {
        let tx = self.db.begin().await?;
        let result = async {
            let ExecutorCloseScope { work_unit, outcome } =
                load_executor_close_scope(&tx, session_id, work_unit_id, agent_id).await?;
            let plan = plan_executor_close(&work_unit, &outcome)?;
            if plan.disposition == ExecutorCloseDisposition::PreserveForMerge {
                return Ok(plan.disposition);
            }

            let now = unix_seconds();
            let mut active_work_unit: entities::work_unit::ActiveModel = work_unit.into();
            if plan.cancel_active {
                active_work_unit.status = Set(WorkUnitStatus::Cancelled.as_str().to_string());
            }
            active_work_unit.worktree_disposition = Set(TaskWorktreeDisposition::CleanupRequested
                .as_str()
                .to_string());
            active_work_unit.updated_at = Set(now);
            active_work_unit.update(&tx).await?;

            if plan.cancel_active {
                let mut active_outcome: entities::agent_outcome::ActiveModel = outcome.into();
                active_outcome.status = Set(AgentOutcomeStatus::Cancelled.as_str().to_string());
                active_outcome.error = Set(Some("executor discarded by planner".to_string()));
                active_outcome.updated_at = Set(now);
                active_outcome.update(&tx).await?;
            }
            Ok(plan.disposition)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn preflight_executor_close(
        &self,
        session_id: &str,
        work_unit_id: &str,
        agent_id: &str,
    ) -> Result<ExecutorCloseDisposition> {
        let tx = self.db.begin().await?;
        let result = async {
            let scope = load_executor_close_scope(&tx, session_id, work_unit_id, agent_id).await?;
            Ok(plan_executor_close(&scope.work_unit, &scope.outcome)?.disposition)
        }
        .await;
        finish_transaction(tx, result).await
    }
}

async fn load_executor_close_scope(
    tx: &sea_orm::DatabaseTransaction,
    session_id: &str,
    work_unit_id: &str,
    agent_id: &str,
) -> Result<ExecutorCloseScope> {
    let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
        .one(tx)
        .await?
        .context("executor work unit not found")?;
    let outcome = entities::agent_outcome::Entity::find()
        .filter(entities::agent_outcome::Column::WorkUnitId.eq(Some(work_unit_id.to_string())))
        .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
        .one(tx)
        .await?
        .context("executor outcome not found")?;
    let run = entities::task_run::Entity::find_by_id(work_unit.task_run_id.clone())
        .one(tx)
        .await?
        .context("executor task run not found")?;
    if run.session_id != session_id
        || outcome.task_run_id != run.id
        || outcome.work_unit_id.as_deref() != Some(work_unit.id.as_str())
        || work_unit.agent_id.as_deref() != Some(agent_id)
        || outcome.role != "executor"
    {
        bail!("executor close lifecycle identity does not match durable assignment");
    }
    Ok(ExecutorCloseScope { work_unit, outcome })
}

fn plan_executor_close(
    work_unit: &entities::work_unit::Model,
    outcome: &entities::agent_outcome::Model,
) -> Result<ExecutorClosePlan> {
    let work_status = WorkUnitStatus::from_str(&work_unit.status)
        .with_context(|| format!("invalid work unit status: {}", work_unit.status))?;
    let outcome_status = AgentOutcomeStatus::from_str(&outcome.status)
        .with_context(|| format!("invalid agent outcome status: {}", outcome.status))?;
    if work_status == WorkUnitStatus::Approved && outcome_status == AgentOutcomeStatus::Completed {
        return Ok(ExecutorClosePlan {
            disposition: ExecutorCloseDisposition::PreserveForMerge,
            cancel_active: false,
        });
    }
    if matches!(
        work_status,
        WorkUnitStatus::ReadyForReview
            | WorkUnitStatus::Reviewing
            | WorkUnitStatus::ChangesRequested
            | WorkUnitStatus::Merging
    ) {
        bail!("executor cannot close while its completion or merge is active");
    }

    let terminal_pair = matches!(
        (work_status, outcome_status),
        (WorkUnitStatus::Merged, AgentOutcomeStatus::Completed)
            | (WorkUnitStatus::NoDelivery, AgentOutcomeStatus::Completed)
            | (WorkUnitStatus::Failed, AgentOutcomeStatus::Failed)
            | (WorkUnitStatus::Cancelled, AgentOutcomeStatus::Cancelled)
    );
    let active_pair = matches!(
        (work_status, outcome_status),
        (WorkUnitStatus::Pending, AgentOutcomeStatus::Queued)
            | (WorkUnitStatus::Running, AgentOutcomeStatus::Running)
            | (
                WorkUnitStatus::AwaitingCompletion,
                AgentOutcomeStatus::Completed
            )
    );
    if !active_pair && !terminal_pair {
        bail!(
            "executor close lifecycle state mismatch: workUnit={}, outcome={}",
            work_unit.status,
            outcome.status
        );
    }
    Ok(ExecutorClosePlan {
        disposition: ExecutorCloseDisposition::Discard,
        cancel_active: active_pair,
    })
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
