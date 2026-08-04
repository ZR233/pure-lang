use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentDelivery, AgentOutcomeStatus, AgentWorktreeDelivery, DeliveryScope,
    DeliveryScopeResolution, TaskRunPhase, WorkCompletionKind, WorkCompletionRecord,
    WorkCompletionStatus, WorkUnitStatus,
};
use pl_core::TurnOutcomeKind;

use super::outcome::agent_outcome_record;
use super::task_run_record;
use super::work_unit::work_unit_record;

impl StudioStore {
    pub(crate) async fn resolve_active_completion_scope(
        &self,
        agent_id: &str,
        worktree_path: &str,
        branch: &str,
    ) -> Result<Option<DeliveryScopeResolution>> {
        let work_units = entities::work_unit::Entity::find()
            .filter(entities::work_unit::Column::AgentId.eq(agent_id.to_string()))
            .all(&self.db)
            .await?;
        let mut matching = Vec::new();
        let mut fallback = Vec::new();
        for work_unit in work_units {
            let Some(outcome) = entities::agent_outcome::Entity::find()
                .filter(entities::agent_outcome::Column::WorkUnitId.eq(work_unit.id.clone()))
                .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
                .one(&self.db)
                .await?
            else {
                continue;
            };
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
                outcome: agent_outcome_record(outcome)?,
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
                return Ok(scopes
                    .pop()
                    .map(Box::new)
                    .map(DeliveryScopeResolution::Resolved));
            }
            _ => bail!("ambiguous active completion scope for executor worktree"),
        }

        let outcomes = entities::agent_outcome::Entity::find()
            .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
            .all(&self.db)
            .await?;
        let mut missing = Vec::new();
        for outcome in outcomes {
            let active = entities::task_run::Entity::find_by_id(outcome.task_run_id.clone())
                .filter(entities::task_run::Column::Phase.is_not_in([
                    TaskRunPhase::Stopping.as_str(),
                    TaskRunPhase::Completed.as_str(),
                    TaskRunPhase::Blocked.as_str(),
                    TaskRunPhase::Failed.as_str(),
                    TaskRunPhase::Cancelled.as_str(),
                ]))
                .one(&self.db)
                .await?
                .is_some();
            if !active {
                continue;
            }
            let has_work_unit = match outcome.work_unit_id.as_deref() {
                Some(work_unit_id) => entities::work_unit::Entity::find_by_id(work_unit_id)
                    .one(&self.db)
                    .await?
                    .is_some(),
                None => false,
            };
            if !has_work_unit {
                missing.push(agent_outcome_record(outcome)?);
            }
        }
        match missing.len() {
            0 => Ok(None),
            1 => Ok(missing
                .pop()
                .map(Box::new)
                .map(DeliveryScopeResolution::MissingWorkUnit)),
            _ => bail!("ambiguous active completion scope for executor worktree"),
        }
    }

    pub(crate) async fn create_work_completion(
        &self,
        outcome_id: &str,
        work_unit_id: &str,
        kind: WorkCompletionKind,
        delivery: Option<&AgentDelivery>,
        verification_summary: &str,
    ) -> Result<WorkCompletionRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let outcome = entities::agent_outcome::Entity::find_by_id(outcome_id.to_string())
                .one(&tx)
                .await?
                .context("agent outcome not found")?;
            let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
                .one(&tx)
                .await?
                .context("work unit not found")?;
            validate_link(&outcome, &work_unit)?;
            let run = entities::task_run::Entity::find_by_id(work_unit.task_run_id.clone())
                .one(&tx)
                .await?
                .context("task run not found")?;
            let phase = TaskRunPhase::from_str(&run.phase)
                .with_context(|| format!("invalid task phase: {}", run.phase))?;
            if phase == TaskRunPhase::Stopping || phase.is_terminal() || run.stop_requested != 0 {
                bail!("task is not accepting executor completion");
            }
            if outcome.status != AgentOutcomeStatus::Running.as_str() {
                bail!("executor outcome is not active");
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
            let completion = entities::work_completion::ActiveModel {
                id: Set(new_id("completion")),
                task_run_id: Set(run.id),
                work_unit_id: Set(work_unit.id.clone()),
                executor_agent_id: Set(outcome.agent_id.clone()),
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
            work_unit_active.updated_at = Set(now);
            work_unit_active.update(&tx).await?;
            let mut outcome_active: entities::agent_outcome::ActiveModel = outcome.into();
            outcome_active.status = Set(AgentOutcomeStatus::Completed.as_str().to_string());
            outcome_active.summary = Set(Some(verification_summary.to_string()));
            outcome_active.error = Set(None);
            outcome_active.updated_at = Set(now);
            outcome_active.update(&tx).await?;
            work_completion_record(completion)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn mark_executor_turn_started(&self, agent_id: &str) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let outcome = executor_outcome(&tx, agent_id).await?;
            let work_unit_id = outcome
                .work_unit_id
                .as_deref()
                .context("executor outcome has no work unit")?;
            let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id)
                .one(&tx)
                .await?
                .context("executor work unit not found")?;
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
            if outcome.status == AgentOutcomeStatus::Running.as_str() {
                return Ok(());
            }
            let mut active: entities::agent_outcome::ActiveModel = outcome.into();
            active.status = Set(AgentOutcomeStatus::Running.as_str().to_string());
            active.error = Set(None);
            active.updated_at = Set(unix_seconds());
            active.update(&tx).await?;
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn authorize_executor_message(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let outcome = executor_outcome(&tx, agent_id).await?;
            let work_unit_id = outcome
                .work_unit_id
                .as_deref()
                .context("executor outcome has no work unit")?;
            let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id)
                .one(&tx)
                .await?
                .context("executor work unit not found")?;
            validate_link(&outcome, &work_unit)?;
            let run = entities::task_run::Entity::find_by_id(work_unit.task_run_id.clone())
                .one(&tx)
                .await?
                .context("executor task run not found")?;
            let phase = TaskRunPhase::from_str(&run.phase)
                .with_context(|| format!("invalid task phase: {}", run.phase))?;
            if run.session_id != session_id
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
            let outcome = executor_outcome(&tx, agent_id).await?;
            let work_unit_id = outcome
                .work_unit_id
                .as_deref()
                .context("executor outcome has no work unit")?;
            let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id)
                .one(&tx)
                .await?
                .context("executor work unit not found")?;
            let work_status = WorkUnitStatus::from_str(&work_unit.status)
                .with_context(|| format!("invalid work unit status: {}", work_unit.status))?;
            if matches!(
                work_status,
                WorkUnitStatus::ReadyForReview
                    | WorkUnitStatus::Reviewing
                    | WorkUnitStatus::Approved
                    | WorkUnitStatus::Merging
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
                let mut active: entities::work_unit::ActiveModel = work_unit.into();
                active.status = Set(WorkUnitStatus::AwaitingCompletion.as_str().to_string());
                active.updated_at = Set(now);
                active.update(&tx).await?;
            }
            let status = match outcome_kind {
                TurnOutcomeKind::Completed => AgentOutcomeStatus::Completed,
                TurnOutcomeKind::Failed | TurnOutcomeKind::BudgetLimited => {
                    AgentOutcomeStatus::Failed
                }
                TurnOutcomeKind::Cancelled => AgentOutcomeStatus::Cancelled,
            };
            let mut active: entities::agent_outcome::ActiveModel = outcome.into();
            active.status = Set(status.as_str().to_string());
            active.error = Set(reason.map(str::to_string).or_else(|| {
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

    pub(crate) async fn read_approved_work_completion(
        &self,
        work_unit_id: &str,
    ) -> Result<WorkCompletionRecord> {
        let completion = entities::work_completion::Entity::find()
            .filter(entities::work_completion::Column::WorkUnitId.eq(work_unit_id.to_string()))
            .order_by_desc(entities::work_completion::Column::Revision)
            .one(&self.db)
            .await?
            .context("executor work unit has no completion")?;
        if completion.status != WorkCompletionStatus::Approved.as_str() {
            bail!("latest executor completion is not approved");
        }
        work_completion_record(completion)
    }
}

async fn executor_outcome(
    tx: &sea_orm::DatabaseTransaction,
    agent_id: &str,
) -> Result<entities::agent_outcome::Model> {
    let outcomes = entities::agent_outcome::Entity::find()
        .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
        .filter(entities::agent_outcome::Column::Role.eq("executor"))
        .all(tx)
        .await?;
    match outcomes.as_slice() {
        [outcome] => Ok(outcome.clone()),
        [] => bail!("executor outcome not found"),
        _ => bail!("executor owns multiple outcomes"),
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

fn validate_link(
    outcome: &entities::agent_outcome::Model,
    work_unit: &entities::work_unit::Model,
) -> Result<()> {
    if outcome.work_unit_id.as_deref() != Some(work_unit.id.as_str())
        || outcome.task_run_id != work_unit.task_run_id
        || work_unit.agent_id.as_deref() != Some(outcome.agent_id.as_str())
        || outcome.role != "executor"
    {
        bail!("agent outcome and work unit do not describe the same executor");
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
