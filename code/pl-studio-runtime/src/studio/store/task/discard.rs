use anyhow::{Context, Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};

use super::work_unit::{update_work_unit_state, work_unit_state};

use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    ExecutorCloseDisposition, ExecutorContinuationState, TaskCommand, TaskRunStateKind,
    TaskStopOrigin, TaskStopReason, TaskWorktreeDisposition, ThreadExecutionStatus, WorkUnitStatus,
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
            let record = super::task_run_record(run.clone())?;
            let phase = record.kind();
            let now = unix_seconds();
            if !phase.is_terminal() {
                let reason = TaskStopReason::new("recovery cleanup requested by user")
                    .context("recovery cleanup stop reason must not be empty")?;
                let run = if phase == TaskRunStateKind::Stopping {
                    run
                } else {
                    super::apply_task_command(
                        &tx,
                        run,
                        TaskCommand::RequestStop((TaskStopOrigin::UserRequest, reason, now).into()),
                    )
                    .await?
                };
                let stopping = super::task_run_record(run.clone())?;
                super::write_task_terminal_fact(
                    &tx,
                    run,
                    TaskRunStateKind::Cancelled,
                    Some("recovery cleanup requested by user".to_string()),
                    Some(stopping.generation()),
                )
                .await?;
            }

            let work_units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id.to_string()))
                .all(&tx)
                .await?;
            for work_unit in work_units {
                let state = work_unit_state(&work_unit)?;
                let status = state.status();
                let execution = state.execution_status();
                let mut progress = state.into_progress();
                progress.worktree_disposition = TaskWorktreeDisposition::CleanupRequested;
                update_work_unit_state(&tx, work_unit, status, execution, progress).await?;
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

            let state = work_unit_state(&work_unit)?;
            let mut progress = state.clone().into_progress();
            let mut status = state.status();
            let mut execution = state.execution_status();
            if plan.cancel_active {
                status = WorkUnitStatus::Cancelled;
                execution = ThreadExecutionStatus::Cancelled;
                progress.execution_error = Some("executor discarded by planner".to_string());
                progress.continuation_state = ExecutorContinuationState::None;
                progress.continuation_source_turn_id = None;
                progress.continuation_revision = progress.continuation_revision.saturating_add(1);
            }
            progress.worktree_disposition = TaskWorktreeDisposition::CleanupRequested;
            update_work_unit_state(&tx, work_unit, status, execution, progress).await?;

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
    let state = work_unit_state(work_unit)?;
    let work_status = state.status();
    let execution_status = state.execution_status();
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
                WorkUnitStatus::Running,
                ThreadExecutionStatus::BudgetLimited
            )
            | (
                WorkUnitStatus::NeedsAttention,
                ThreadExecutionStatus::BudgetLimited
            )
            | (
                WorkUnitStatus::AwaitingCompletion,
                ThreadExecutionStatus::Completed
            )
            | (
                WorkUnitStatus::AwaitingCompletion,
                ThreadExecutionStatus::Cancelled
            )
    );
    if !active_pair && !terminal_pair {
        bail!(
            "executor close lifecycle state mismatch: workUnit={}, execution={}",
            work_status.as_str(),
            execution_status.as_str()
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
