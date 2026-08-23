use anyhow::{Context, Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};

use super::work_unit::{apply_work_unit_command, work_unit_record};

use crate::studio::entity as entities;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    ExecutorCloseDisposition, TaskWorktreeDisposition, WaitingReviewPhase, WorkUnitCommand,
    WorkUnitStateKind,
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
            let _record = super::task_run_record(run)?;

            let work_units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id.to_string()))
                .all(&tx)
                .await?;
            for work_unit in work_units {
                let record = work_unit_record(work_unit.clone())?;
                if !record.kind().is_terminal() {
                    apply_work_unit_command(
                        &tx,
                        work_unit,
                        WorkUnitCommand::Cancel {
                            operation_id: format!("recovery-cleanup:{task_run_id}"),
                            reason: "recovery cleanup requested by user".to_string(),
                            disposition: TaskWorktreeDisposition::CleanupRequested,
                        },
                    )
                    .await?;
                }
            }
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

            if plan.cancel_active {
                apply_work_unit_command(
                    &tx,
                    work_unit,
                    WorkUnitCommand::Cancel {
                        operation_id: format!("executor-close:{agent_id}"),
                        reason: "executor discarded by planner".to_string(),
                        disposition: TaskWorktreeDisposition::CleanupRequested,
                    },
                )
                .await?;
            }

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
    let record = work_unit_record(work_unit.clone())?;
    if record.kind() == WorkUnitStateKind::ReviewPassed {
        return Ok(ExecutorClosePlan {
            disposition: ExecutorCloseDisposition::PreserveForMerge,
            cancel_active: false,
        });
    }
    if record.kind() == WorkUnitStateKind::ChangesRequired
        || matches!(
            record.waiting_review_phase(),
            Some(WaitingReviewPhase::Ready(_) | WaitingReviewPhase::Reviewing(_))
        )
    {
        bail!("executor cannot close while its completion review is active");
    }

    let active_pair = !record.kind().is_terminal();
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
