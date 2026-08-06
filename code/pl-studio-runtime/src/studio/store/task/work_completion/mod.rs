use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentDelivery, AgentWorktreeDelivery, DeliveryScope, TaskRunPhase, ThreadExecutionStatus,
    WorkCompletionKind, WorkCompletionRecord, WorkCompletionStatus, WorkUnitStatus,
};
use pl_core::TurnOutcomeKind;

use super::task_run_record;
use super::work_unit::work_unit_record;

impl StudioStore {
    pub(crate) async fn resolve_active_completion_scope(
        &self,
        agent_id: &str,
        worktree_path: &str,
        branch: &str,
    ) -> Result<Option<DeliveryScope>> {
        let work_units = entities::work_unit::Entity::find()
            .filter(entities::work_unit::Column::ExecutorThreadId.eq(agent_id.to_string()))
            .all(&self.db)
            .await?;
        let mut matching = Vec::new();
        let mut fallback = Vec::new();
        for work_unit in work_units {
            let Some(run) = entities::task_run::Entity::find_by_id(work_unit.task_run_id.clone())
                .filter(entities::task_run::Column::Phase.is_not_in([
                    TaskRunPhase::Stopping.as_str(),
                    TaskRunPhase::Completed.as_str(),
                    TaskRunPhase::Blocked.as_str(),
                    TaskRunPhase::Failed.as_str(),
                    TaskRunPhase::Cancelled.as_str(),
                ]))
                .one(&self.db)
                .await?
            else {
                continue;
            };
            let matches_caller =
                work_unit.worktree_path == worktree_path && work_unit.branch == branch;
            let scope = DeliveryScope {
                run: task_run_record(run)?,
                work_unit: work_unit_record(work_unit)?,
            };
            if matches_caller {
                matching.push(scope);
            } else {
                fallback.push(scope);
            }
        }
        let scopes = if matching.is_empty() {
            &mut fallback
        } else {
            &mut matching
        };
        match scopes.len() {
            0 => {}
            1 => {
                return Ok(scopes.pop());
            }
            _ => bail!("ambiguous active completion scope for executor worktree"),
        }

        Ok(None)
    }

    pub(crate) async fn create_work_completion(
        &self,
        work_unit_id: &str,
        kind: WorkCompletionKind,
        delivery: Option<&AgentDelivery>,
        verification_summary: &str,
    ) -> Result<WorkCompletionRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
                .one(&tx)
                .await?
                .context("work unit not found")?;
            let run = entities::task_run::Entity::find_by_id(work_unit.task_run_id.clone())
                .one(&tx)
                .await?
                .context("task run not found")?;
            let phase = TaskRunPhase::from_str(&run.phase)
                .with_context(|| format!("invalid task phase: {}", run.phase))?;
            if phase == TaskRunPhase::Stopping || phase.is_terminal() || run.stop_requested != 0 {
                bail!("task is not accepting executor completion");
            }
            if work_unit.execution_status != ThreadExecutionStatus::Running.as_str() {
                bail!("executor Thread is not active");
            }
            if !matches!(
                WorkUnitStatus::from_str(&work_unit.status),
                Some(
                    WorkUnitStatus::Running
                        | WorkUnitStatus::AwaitingCompletion
                        | WorkUnitStatus::ChangesRequested
                )
            ) {
                bail!("work unit is not accepting a completion");
            }
            let latest = entities::work_completion::Entity::find()
                .filter(entities::work_completion::Column::WorkUnitId.eq(work_unit.id.clone()))
                .order_by_desc(entities::work_completion::Column::Revision)
                .one(&tx)
                .await?;
            if latest.as_ref().is_some_and(|completion| {
                completion.status == WorkCompletionStatus::ReadyForReview.as_str()
            }) {
                bail!("work unit already has an active completion review");
            }
            let revision = latest.map_or(1, |completion| completion.revision + 1);
            let (head_commit, changed_files) = match (kind, delivery) {
                (WorkCompletionKind::Delivery, Some(delivery)) => (
                    Some(delivery.head_commit.clone()),
                    delivery.changed_files.clone(),
                ),
                (WorkCompletionKind::NoDelivery, None) => (None, Vec::new()),
                _ => bail!("completion kind and delivery payload do not match"),
            };
            let now = unix_seconds();
            let executor_thread_id = work_unit
                .executor_thread_id
                .clone()
                .context("work unit has no executor Thread")?;
            let completion = entities::work_completion::ActiveModel {
                id: Set(new_id("completion")),
                task_run_id: Set(run.id),
                work_unit_id: Set(work_unit.id.clone()),
                executor_agent_id: Set(executor_thread_id),
                revision: Set(revision),
                kind: Set(kind.as_str().to_string()),
                status: Set(WorkCompletionStatus::ReadyForReview.as_str().to_string()),
                base_commit: Set(work_unit.base_commit.clone()),
                head_commit: Set(head_commit),
                changed_files_json: Set(serde_json::to_string(&changed_files)?),
                verification_summary: Set(verification_summary.to_string()),
                worktree_path: Set(work_unit.worktree_path.clone()),
                branch: Set(work_unit.branch.clone()),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await?;
            let mut work_unit_active: entities::work_unit::ActiveModel = work_unit.into();
            work_unit_active.status = Set(WorkUnitStatus::ReadyForReview.as_str().to_string());
            work_unit_active.execution_status =
                Set(ThreadExecutionStatus::Completed.as_str().to_string());
            work_unit_active.execution_summary = Set(Some(verification_summary.to_string()));
            work_unit_active.execution_error = Set(None);
            work_unit_active.updated_at = Set(now);
            work_unit_active.update(&tx).await?;
            work_completion_record(completion)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn mark_executor_turn_started(&self, agent_id: &str) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let work_unit = executor_work_unit(&tx, agent_id).await?;
            let work_status = WorkUnitStatus::from_str(&work_unit.status)
                .with_context(|| format!("invalid work unit status: {}", work_unit.status))?;
            if !matches!(
                work_status,
                WorkUnitStatus::Running
                    | WorkUnitStatus::AwaitingCompletion
                    | WorkUnitStatus::ChangesRequested
            ) {
                bail!(
                    "executor cannot start a turn while work unit is {}",
                    work_unit.status
                );
            }
            if work_unit.execution_status == ThreadExecutionStatus::Running.as_str() {
                return Ok(());
            }
            let mut active: entities::work_unit::ActiveModel = work_unit.into();
            active.execution_status = Set(ThreadExecutionStatus::Running.as_str().to_string());
            active.execution_error = Set(None);
            active.updated_at = Set(unix_seconds());
            active.update(&tx).await?;
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn authorize_executor_message(
        &self,
        thread_id: &str,
        agent_id: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let work_unit = executor_work_unit(&tx, agent_id).await?;
            let run = entities::task_run::Entity::find_by_id(work_unit.task_run_id.clone())
                .one(&tx)
                .await?
                .context("executor task run not found")?;
            let phase = TaskRunPhase::from_str(&run.phase)
                .with_context(|| format!("invalid task phase: {}", run.phase))?;
            if run.root_thread_id != thread_id
                || run.stop_requested != 0
                || phase == TaskRunPhase::Stopping
                || phase.is_terminal()
            {
                bail!("executor is not accepting messages for this task");
            }
            let work_status = WorkUnitStatus::from_str(&work_unit.status)
                .with_context(|| format!("invalid work unit status: {}", work_unit.status))?;
            if !matches!(
                work_status,
                WorkUnitStatus::Running
                    | WorkUnitStatus::AwaitingCompletion
                    | WorkUnitStatus::ChangesRequested
            ) {
                bail!(
                    "executor cannot receive a message while work unit is {}",
                    work_unit.status
                );
            }
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn settle_executor_turn_finished(
        &self,
        agent_id: &str,
        outcome_kind: TurnOutcomeKind,
        reason: Option<&str>,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let work_unit = executor_work_unit(&tx, agent_id).await?;
            let work_status = WorkUnitStatus::from_str(&work_unit.status)
                .with_context(|| format!("invalid work unit status: {}", work_unit.status))?;
            if matches!(
                work_status,
                WorkUnitStatus::ReadyForReview
                    | WorkUnitStatus::Reviewing
                    | WorkUnitStatus::Approved
                    | WorkUnitStatus::Merged
                    | WorkUnitStatus::NoDelivery
                    | WorkUnitStatus::Failed
                    | WorkUnitStatus::Cancelled
            ) {
                return Ok(());
            }

            let now = unix_seconds();
            if matches!(
                work_status,
                WorkUnitStatus::Running | WorkUnitStatus::ChangesRequested
            ) {
                let mut active: entities::work_unit::ActiveModel = work_unit.clone().into();
                active.status = Set(WorkUnitStatus::AwaitingCompletion.as_str().to_string());
                active.updated_at = Set(now);
                active.update(&tx).await?;
            }
            let status = match outcome_kind {
                TurnOutcomeKind::Completed => ThreadExecutionStatus::Completed,
                TurnOutcomeKind::Failed | TurnOutcomeKind::BudgetLimited => {
                    ThreadExecutionStatus::Failed
                }
                TurnOutcomeKind::Cancelled => ThreadExecutionStatus::Cancelled,
            };
            let mut active: entities::work_unit::ActiveModel = work_unit.into();
            active.execution_status = Set(status.as_str().to_string());
            active.execution_error = Set(reason.map(str::to_string).or_else(|| {
                (outcome_kind == TurnOutcomeKind::Completed).then(|| {
                    "executor turn ended without a successful report_completion".to_string()
                })
            }));
            active.updated_at = Set(now);
            active.update(&tx).await?;
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn list_work_completions(
        &self,
        task_run_id: &str,
    ) -> Result<Vec<WorkCompletionRecord>> {
        entities::work_completion::Entity::find()
            .filter(entities::work_completion::Column::TaskRunId.eq(task_run_id.to_string()))
            .order_by_asc(entities::work_completion::Column::CreatedAt)
            .order_by_asc(entities::work_completion::Column::Id)
            .all(&self.db)
            .await?
            .into_iter()
            .map(work_completion_record)
            .collect()
    }

    pub(crate) async fn read_work_completion(
        &self,
        completion_id: &str,
    ) -> Result<Option<WorkCompletionRecord>> {
        entities::work_completion::Entity::find_by_id(completion_id.to_string())
            .one(&self.db)
            .await?
            .map(work_completion_record)
            .transpose()
    }
}

async fn executor_work_unit(
    tx: &sea_orm::DatabaseTransaction,
    agent_id: &str,
) -> Result<entities::work_unit::Model> {
    let work_units = entities::work_unit::Entity::find()
        .filter(entities::work_unit::Column::ExecutorThreadId.eq(agent_id.to_string()))
        .all(tx)
        .await?;
    match work_units.as_slice() {
        [work_unit] => Ok(work_unit.clone()),
        [] => bail!("executor work unit not found"),
        _ => bail!("executor Thread owns multiple work units"),
    }
}

pub(super) fn work_completion_record(
    model: entities::work_completion::Model,
) -> Result<WorkCompletionRecord> {
    Ok(WorkCompletionRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        work_unit_id: model.work_unit_id,
        executor_agent_id: model.executor_agent_id,
        revision: u32::try_from(model.revision).context("completion revision must be positive")?,
        kind: WorkCompletionKind::from_str(&model.kind)
            .with_context(|| format!("invalid completion kind: {}", model.kind))?,
        status: WorkCompletionStatus::from_str(&model.status)
            .with_context(|| format!("invalid completion status: {}", model.status))?,
        base_commit: model.base_commit,
        head_commit: model.head_commit,
        changed_files: serde_json::from_str(&model.changed_files_json)?,
        verification_summary: model.verification_summary,
        worktree_path: model.worktree_path,
        branch: model.branch,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

pub(super) fn delivery_from_completion(completion: &WorkCompletionRecord) -> Result<AgentDelivery> {
    if completion.kind != WorkCompletionKind::Delivery
        || completion.status != WorkCompletionStatus::Approved
    {
        bail!("merge requires an approved delivery completion");
    }
    let head_commit = completion
        .head_commit
        .clone()
        .context("delivery completion has no head commit")?;
    Ok(AgentDelivery {
        worktree: AgentWorktreeDelivery {
            path: completion.worktree_path.clone(),
            branch: completion.branch.clone(),
        },
        base_commit: completion.base_commit.clone(),
        head_commit,
        changed_files: completion.changed_files.clone(),
        verification_summary: completion.verification_summary.clone(),
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
