use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    ExecutorCloseDisposition, TaskRunPhase, TaskStopOrigin, TaskWorktreeDisposition,
    ThreadExecutionStatus, WorkUnitStatus,
};

struct ExecutorCloseScope {
    work_unit: entities::work_unit::Model,
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
        thread_id: &str,
        work_unit_id: &str,
        agent_id: &str,
    ) -> Result<ExecutorCloseDisposition> {
        let tx = self.db.begin().await?;
        let result = async {
            let ExecutorCloseScope { work_unit } =
                load_executor_close_scope(&tx, thread_id, work_unit_id, agent_id).await?;
            let plan = plan_executor_close(&work_unit)?;
            if plan.disposition == ExecutorCloseDisposition::PreserveForMerge {
                return Ok(plan.disposition);
            }

            let now = unix_seconds();
            let mut active_work_unit: entities::work_unit::ActiveModel = work_unit.into();
            if plan.cancel_active {
                active_work_unit.status = Set(WorkUnitStatus::Cancelled.as_str().to_string());
                active_work_unit.execution_status =
                    Set(ThreadExecutionStatus::Cancelled.as_str().to_string());
                active_work_unit.execution_error =
                    Set(Some("executor discarded by planner".to_string()));
            }
            active_work_unit.worktree_disposition = Set(TaskWorktreeDisposition::CleanupRequested
                .as_str()
                .to_string());
            active_work_unit.updated_at = Set(now);
            active_work_unit.update(&tx).await?;

            Ok(plan.disposition)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn preflight_executor_close(
        &self,
        thread_id: &str,
        work_unit_id: &str,
        agent_id: &str,
    ) -> Result<ExecutorCloseDisposition> {
        let tx = self.db.begin().await?;
        let result = async {
            let scope = load_executor_close_scope(&tx, thread_id, work_unit_id, agent_id).await?;
            Ok(plan_executor_close(&scope.work_unit)?.disposition)
        }
        .await;
        finish_transaction(tx, result).await
    }
}

async fn load_executor_close_scope(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
    work_unit_id: &str,
    agent_id: &str,
) -> Result<ExecutorCloseScope> {
    let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
        .filter(entities::work_unit::Column::ExecutorThreadId.eq(agent_id.to_string()))
        .one(tx)
        .await?
        .context("executor work unit not found")?;
    let run = entities::task_run::Entity::find_by_id(work_unit.task_run_id.clone())
        .one(tx)
        .await?
        .context("executor task run not found")?;
    if run.root_thread_id != thread_id
        || work_unit.task_run_id != run.id
        || work_unit.executor_thread_id.as_deref() != Some(agent_id)
    {
        bail!("executor close lifecycle identity does not match durable assignment");
    }
    Ok(ExecutorCloseScope { work_unit })
}

fn plan_executor_close(work_unit: &entities::work_unit::Model) -> Result<ExecutorClosePlan> {
    let work_status = WorkUnitStatus::from_str(&work_unit.status)
        .with_context(|| format!("invalid work unit status: {}", work_unit.status))?;
    let execution_status = ThreadExecutionStatus::from_str(&work_unit.execution_status)
        .with_context(|| {
            format!(
                "invalid Thread execution status: {}",
                work_unit.execution_status
            )
        })?;
    if work_status == WorkUnitStatus::Approved
        && execution_status == ThreadExecutionStatus::Completed
    {
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
    ) {
        bail!("executor cannot close while its completion review is active");
    }

    let terminal_pair = matches!(
        (work_status, execution_status),
        (WorkUnitStatus::Merged, ThreadExecutionStatus::Completed)
            | (WorkUnitStatus::NoDelivery, ThreadExecutionStatus::Completed)
            | (WorkUnitStatus::Failed, ThreadExecutionStatus::Failed)
            | (WorkUnitStatus::Cancelled, ThreadExecutionStatus::Cancelled)
    );
    let active_pair = matches!(
        (work_status, execution_status),
        (WorkUnitStatus::Pending, ThreadExecutionStatus::Queued)
            | (WorkUnitStatus::Running, ThreadExecutionStatus::Running)
            | (
                WorkUnitStatus::AwaitingCompletion,
                ThreadExecutionStatus::Completed
            )
    );
    if !active_pair && !terminal_pair {
        bail!(
            "executor close lifecycle state mismatch: workUnit={}, execution={}",
            work_unit.status,
            work_unit.execution_status
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
