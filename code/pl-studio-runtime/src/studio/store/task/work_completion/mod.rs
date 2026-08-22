use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentDelivery, AgentWorktreeDelivery, DeliveryScope, ExecutorContinuationRequest,
    ExecutorContinuationState, MAX_EXECUTOR_BUDGET_SLICES, TaskRunStateKind, ThreadExecutionStatus,
    WorkCompletionKind, WorkCompletionRecord, WorkCompletionStatus, WorkUnitState, WorkUnitStatus,
};
use pl_core::{AgentTurnOutcome, MailboxBudgetAction, TurnOutcomeKind};
use pl_protocol::BudgetLimitKind;

use super::task_run_record;
use super::work_unit::{update_work_unit_state, work_unit_record, work_unit_state};

impl StudioStore {
    pub(crate) async fn resolve_active_completion_scope(
        &self,
        agent_id: &str,
        worktree_path: &str,
    ) -> Result<Option<DeliveryScope>> {
        let work_units = entities::work_unit::Entity::find()
            .filter(entities::work_unit::Column::ExecutorThreadId.eq(agent_id.to_string()))
            .all(&self.db)
            .await?;
        let mut matching = Vec::new();
        for work_unit in work_units {
            let Some(run) = entities::task_run::Entity::find_by_id(work_unit.task_run_id.clone())
                .filter(entities::task_run::Column::StateKind.is_not_in([
                    TaskRunStateKind::Stopping.as_str(),
                    TaskRunStateKind::Completed.as_str(),
                    TaskRunStateKind::Blocked.as_str(),
                    TaskRunStateKind::Failed.as_str(),
                    TaskRunStateKind::Cancelled.as_str(),
                ]))
                .one(&self.db)
                .await?
            else {
                continue;
            };
            let matches_caller = work_unit.worktree_path == worktree_path;
            let scope = DeliveryScope {
                run: task_run_record(run)?,
                work_unit: work_unit_record(work_unit)?,
            };
            if matches_caller {
                matching.push(scope);
            }
        }
        match matching.len() {
            0 => {}
            1 => {
                return Ok(matching.pop());
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
            let task = task_run_record(run.clone())?;
            let phase = task.kind();
            if phase == TaskRunStateKind::Stopping
                || phase == TaskRunStateKind::Blocked
                || phase.is_terminal()
                || task.is_stop_requested()
            {
                bail!("task is not accepting executor completion");
            }
            let work_unit_state = work_unit_state(&work_unit)?;
            if work_unit_state.execution_status() != ThreadExecutionStatus::Running {
                bail!("executor Thread is not active");
            }
            if !matches!(
                work_unit_state.status(),
                WorkUnitStatus::Running
                    | WorkUnitStatus::AwaitingCompletion
                    | WorkUnitStatus::ChangesRequested
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
            let mut progress = work_unit_state.into_progress();
            progress.execution_summary = Some(verification_summary.to_string());
            progress.execution_error = None;
            update_work_unit_state(&tx, work_unit, WorkUnitState::ready_for_review(progress))
                .await?;
            work_completion_record(completion)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn mark_executor_turn_started(
        &self,
        agent_id: &str,
        budget_action: MailboxBudgetAction,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let work_unit = executor_work_unit(&tx, agent_id).await?;
            let state = work_unit_state(&work_unit)?;
            let work_status = state.status();
            let execution_status = state.execution_status();
            let continuation_state = state.progress().continuation_state;
            let budget_attention = work_status == WorkUnitStatus::NeedsAttention
                && execution_status == ThreadExecutionStatus::BudgetLimited
                && state.progress().budget_limit.is_some()
                && continuation_state == ExecutorContinuationState::NeedsAttention;
            let budget_refresh_expected =
                budget_action == MailboxBudgetAction::Refresh && budget_attention;
            if !matches!(
                work_status,
                WorkUnitStatus::Running
                    | WorkUnitStatus::AwaitingCompletion
                    | WorkUnitStatus::ChangesRequested
            ) && !budget_refresh_expected
            {
                bail!(
                    "executor cannot start a turn while work unit is {}",
                    work_status.as_str()
                );
            }
            let already_running = work_status == WorkUnitStatus::Running
                && execution_status == ThreadExecutionStatus::Running
                && continuation_state == ExecutorContinuationState::None;
            let already_refreshed = already_running
                && state.progress().budget_slice_count == 1
                && state.progress().budget_limit.is_none()
                && state.progress().execution_error.is_none()
                && state.progress().continuation_source_turn_id.is_none();
            if (budget_action == MailboxBudgetAction::Preserve && already_running)
                || (budget_action == MailboxBudgetAction::Refresh && already_refreshed)
            {
                return Ok(());
            }
            let mut progress = state.into_progress();
            progress.continuation_revision = progress.continuation_revision.saturating_add(1);
            progress.execution_error = None;
            progress.continuation_state = ExecutorContinuationState::None;
            if budget_action == MailboxBudgetAction::Refresh {
                progress.budget_limit = None;
                progress.budget_slice_count = 1;
                progress.continuation_source_turn_id = None;
            }
            update_work_unit_state(&tx, work_unit, WorkUnitState::running(progress)).await?;
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn settle_executor_turn_finished(
        &self,
        agent_id: &str,
        outcome: &AgentTurnOutcome,
    ) -> Result<Option<ExecutorContinuationRequest>> {
        let tx = self.db.begin().await?;
        let result = async {
            let work_unit = executor_work_unit(&tx, agent_id).await?;
            let state = work_unit_state(&work_unit)?;
            let work_status = state.status();
            if matches!(
                work_status,
                WorkUnitStatus::ReadyForReview
                    | WorkUnitStatus::Reviewing
                    | WorkUnitStatus::Approved
                    | WorkUnitStatus::Merged
                    | WorkUnitStatus::NoDelivery
                    | WorkUnitStatus::NeedsAttention
                    | WorkUnitStatus::Failed
                    | WorkUnitStatus::Cancelled
            ) {
                return Ok(None);
            }
            let continuation_state = state.progress().continuation_state;
            if state.progress().continuation_source_turn_id.as_deref()
                == Some(outcome.turn_id.as_str())
            {
                if outcome.kind == TurnOutcomeKind::BudgetLimited
                    && continuation_state == ExecutorContinuationState::PendingStart
                {
                    return Ok(Some(ExecutorContinuationRequest {
                        agent_id: agent_id.to_string(),
                        work_unit_id: work_unit.id,
                        source_turn_id: outcome.turn_id.to_string(),
                        slice_count: state.progress().budget_slice_count,
                    }));
                }
                return Ok(None);
            }

            if outcome.kind == TurnOutcomeKind::BudgetLimited {
                let budget_limit = outcome
                    .budget_limit
                    .context("budget-limited executor outcome has no budget snapshot")?;
                let current_slice = state.progress().budget_slice_count;
                let can_continue = budget_limit.kind == BudgetLimitKind::WallClock
                    && outcome.rollover_compacted
                    && outcome.rollover_compaction_error.is_none()
                    && current_slice < MAX_EXECUTOR_BUDGET_SLICES;
                let mut progress = state.clone().into_progress();
                progress.budget_limit = Some(budget_limit);
                progress.continuation_source_turn_id = Some(outcome.turn_id.to_string());
                progress.continuation_revision = progress.continuation_revision.saturating_add(1);
                if can_continue {
                    let next_slice = current_slice.saturating_add(1);
                    let work_unit_id = work_unit.id.clone();
                    progress.budget_slice_count = next_slice;
                    progress.continuation_state = ExecutorContinuationState::PendingStart;
                    progress.execution_error = None;
                    update_work_unit_state(&tx, work_unit, WorkUnitState::budget_limited(progress))
                        .await?;
                    return Ok(Some(ExecutorContinuationRequest {
                        agent_id: agent_id.to_string(),
                        work_unit_id,
                        source_turn_id: outcome.turn_id.to_string(),
                        slice_count: next_slice,
                    }));
                }
                progress.continuation_state = ExecutorContinuationState::NeedsAttention;
                progress.execution_error =
                    outcome.rollover_compaction_error.clone().or_else(|| {
                        outcome.reason.clone().or_else(|| {
                            Some(if budget_limit.kind == BudgetLimitKind::WallClock {
                                format!(
                                    "executor reached the {MAX_EXECUTOR_BUDGET_SLICES}-slice limit"
                                )
                            } else {
                                format!(
                                    "executor stopped at the {} budget limit",
                                    budget_limit.kind.as_str()
                                )
                            })
                        })
                    });
                update_work_unit_state(&tx, work_unit, WorkUnitState::needs_attention(progress))
                    .await?;
                return Ok(None);
            }
            let mut progress = state.into_progress();
            progress.execution_error = outcome.reason.clone().or_else(|| {
                (outcome.kind == TurnOutcomeKind::Completed).then(|| {
                    "executor turn ended without a successful report_completion".to_string()
                })
            });
            if matches!(
                outcome.kind,
                TurnOutcomeKind::Completed | TurnOutcomeKind::Failed
            ) {
                progress.continuation_state = ExecutorContinuationState::PlannerWakePending;
                progress.continuation_source_turn_id = Some(outcome.turn_id.to_string());
                progress.continuation_revision = progress.continuation_revision.saturating_add(1);
            } else {
                progress.continuation_state = ExecutorContinuationState::None;
            }
            let next_state = match outcome.kind {
                TurnOutcomeKind::Completed => WorkUnitState::awaiting_completed(progress),
                TurnOutcomeKind::Failed => WorkUnitState::awaiting_failed(progress),
                TurnOutcomeKind::Cancelled => WorkUnitState::awaiting_cancelled(progress),
                TurnOutcomeKind::BudgetLimited => unreachable!("handled above"),
            };
            update_work_unit_state(&tx, work_unit, next_state).await?;
            Ok(None)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn fail_executor_continuation(
        &self,
        continuation: &ExecutorContinuationRequest,
        error: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let work_unit = entities::work_unit::Entity::find_by_id(&continuation.work_unit_id)
                .one(&tx)
                .await?
                .context("executor continuation work unit not found")?;
            let state = work_unit_state(&work_unit)?;
            if work_unit.executor_thread_id.as_deref() != Some(continuation.agent_id.as_str())
                || state.progress().continuation_source_turn_id.as_deref()
                    != Some(continuation.source_turn_id.as_str())
                || state.progress().continuation_state != ExecutorContinuationState::PendingStart
            {
                return Ok(());
            }
            let mut progress = state.into_progress();
            progress.continuation_state = ExecutorContinuationState::NeedsAttention;
            progress.continuation_revision = progress.continuation_revision.saturating_add(1);
            progress.execution_error = Some(error.to_string());
            update_work_unit_state(&tx, work_unit, WorkUnitState::needs_attention(progress))
                .await?;
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
