use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter,
    QueryOrder, TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentDelivery, AgentWorktreeDelivery, DeliveryScope, ExecutorContinuationRequest,
    ExecutorContinuationStateKind, ExecutorTerminalOutcome, MAX_EXECUTOR_BUDGET_SLICES,
    TaskRunStateKind, TaskWorktreeDisposition, WorkCompletionContent, WorkCompletionKind,
    WorkCompletionRecord, WorkCompletionState, WorkCompletionStatus, WorkUnitCommand,
    WorkUnitStateKind,
};
use pl_core::{AgentTurnOutcome, MailboxBudgetAction};
use pl_protocol::BudgetLimitKind;
use pl_protocol::{TurnOutcome, TurnRolloverOutcome};

use super::task_run_record;
use super::work_unit::{apply_work_unit_command, work_unit_record, work_unit_state};

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
                .filter(
                    entities::task_run::Column::StateKind.eq(TaskRunStateKind::Working.as_str()),
                )
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
        content: WorkCompletionContent,
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
            if phase != TaskRunStateKind::Working {
                bail!("task is not accepting executor completion");
            }
            let work_unit_state = work_unit_state(&work_unit)?;
            if work_unit_state.kind() != WorkUnitStateKind::Running {
                bail!("work unit is not accepting a completion");
            }
            let latest = entities::work_completion::Entity::find()
                .filter(entities::work_completion::Column::WorkUnitId.eq(work_unit.id.clone()))
                .order_by_desc(entities::work_completion::Column::Revision)
                .one(&tx)
                .await?;
            if latest.as_ref().is_some_and(|completion| {
                completion.state_kind == WorkCompletionStatus::ReadyForReview.as_str()
            }) {
                bail!("work unit already has an active completion review");
            }
            let revision = latest.map_or(1, |completion| completion.revision + 1);
            let state = WorkCompletionState::ready_for_review();
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
                content_json: Set(serde_json::to_string(&content)?),
                content_kind: NotSet,
                state_json: Set(serde_json::to_string(&state)?),
                state_kind: NotSet,
                state_revision: Set(0),
                base_commit: Set(work_unit.base_commit.clone()),
                verification_summary: Set(verification_summary.to_string()),
                worktree_path: Set(work_unit.worktree_path.clone()),
                branch: Set(work_unit.branch.clone()),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await?;
            apply_work_unit_command(
                &tx,
                work_unit,
                WorkUnitCommand::SubmitCompletion {
                    completion_id: completion.id.clone(),
                    completion_revision: u32::try_from(completion.revision)
                        .context("completion revision is negative")?,
                    verification_summary: verification_summary.to_string(),
                },
            )
            .await?;
            work_completion_record(completion)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn mark_executor_turn_started(
        &self,
        agent_id: &str,
        turn_id: &str,
        budget_action: MailboxBudgetAction,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let work_unit = executor_work_unit(&tx, agent_id).await?;
            apply_work_unit_command(
                &tx,
                work_unit,
                WorkUnitCommand::StartTurn {
                    turn_id: turn_id.to_string(),
                    reset_budget: budget_action == MailboxBudgetAction::Refresh,
                },
            )
            .await?;
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
            let record = work_unit_record(work_unit.clone())?;
            if record.continuation_source_turn_id() == Some(outcome.turn_id.as_str()) {
                if matches!(&outcome.outcome, TurnOutcome::BudgetLimited(_))
                    && record.continuation_state() == ExecutorContinuationStateKind::PendingStart
                {
                    return Ok(Some(ExecutorContinuationRequest {
                        agent_id: agent_id.to_string(),
                        work_unit_id: work_unit.id,
                        source_turn_id: outcome.turn_id.to_string(),
                        slice_count: record.budget_slice_count(),
                    }));
                }
                return Ok(None);
            }

            if record.kind() != WorkUnitStateKind::Running {
                return Ok(None);
            }

            if let TurnOutcome::BudgetLimited(budget) = &outcome.outcome {
                let budget_limit = budget.limit();
                let current_slice = record.budget_slice_count();
                let can_continue = budget_limit.kind == BudgetLimitKind::WallClock
                    && matches!(budget.rollover(), TurnRolloverOutcome::Succeeded)
                    && current_slice < MAX_EXECUTOR_BUDGET_SLICES;
                if can_continue {
                    let next_slice = current_slice.saturating_add(1);
                    let work_unit_id = work_unit.id.clone();
                    apply_work_unit_command(
                        &tx,
                        work_unit,
                        WorkUnitCommand::ContinueAfterBudget {
                            source_turn_id: outcome.turn_id.to_string(),
                            next_slice,
                            limit: *budget_limit,
                        },
                    )
                    .await?;
                    return Ok(Some(ExecutorContinuationRequest {
                        agent_id: agent_id.to_string(),
                        work_unit_id,
                        source_turn_id: outcome.turn_id.to_string(),
                        slice_count: next_slice,
                    }));
                }
                let detail = match budget.rollover() {
                    TurnRolloverOutcome::Failed { error } => error.clone(),
                    TurnRolloverOutcome::NotAttempted | TurnRolloverOutcome::Succeeded => {
                        if budget_limit.kind == BudgetLimitKind::WallClock {
                            format!("executor reached the {MAX_EXECUTOR_BUDGET_SLICES}-slice limit")
                        } else {
                            format!(
                                "executor stopped at the {} budget limit",
                                budget_limit.kind.as_str()
                            )
                        }
                    }
                };
                apply_work_unit_command(
                    &tx,
                    work_unit,
                    WorkUnitCommand::PauseForBudget {
                        source_turn_id: outcome.turn_id.to_string(),
                        limit: *budget_limit,
                        detail,
                    },
                )
                .await?;
                return Ok(None);
            }
            let command = match &outcome.outcome {
                TurnOutcome::Completed(_) => WorkUnitCommand::FinishTurn {
                    outcome: ExecutorTerminalOutcome::Completed {
                        source_turn_id: outcome.turn_id.to_string(),
                        detail: "executor turn ended without a successful report_completion"
                            .to_string(),
                    },
                },
                TurnOutcome::Failed(value) => WorkUnitCommand::FinishTurn {
                    outcome: ExecutorTerminalOutcome::Failed {
                        source_turn_id: outcome.turn_id.to_string(),
                        detail: value.failure().message.clone(),
                    },
                },
                TurnOutcome::Cancelled(value) => WorkUnitCommand::Cancel {
                    operation_id: outcome.turn_id.to_string(),
                    reason: format!("executor turn cancelled: {:?}", value.cause()),
                    disposition: TaskWorktreeDisposition::CleanupRequested,
                },
                TurnOutcome::BudgetLimited(_) => unreachable!("handled above"),
            };
            apply_work_unit_command(&tx, work_unit, command).await?;
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
            let record = work_unit_record(work_unit.clone())?;
            if work_unit.executor_thread_id.as_deref() != Some(continuation.agent_id.as_str())
                || record.continuation_source_turn_id()
                    != Some(continuation.source_turn_id.as_str())
                || record.continuation_state() != ExecutorContinuationStateKind::PendingStart
            {
                return Ok(());
            }
            let limit = record
                .budget_limit()
                .cloned()
                .context("pending executor continuation has no budget snapshot")?;
            apply_work_unit_command(
                &tx,
                work_unit,
                WorkUnitCommand::PauseForBudget {
                    source_turn_id: continuation.source_turn_id.clone(),
                    limit,
                    detail: error.to_string(),
                },
            )
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
    let content: WorkCompletionContent = serde_json::from_str(&model.content_json)
        .context("invalid stored WorkCompletion content JSON")?;
    if content.kind().as_str() != model.content_kind {
        bail!("stored WorkCompletion content discriminator mismatch");
    }
    let state: WorkCompletionState = serde_json::from_str(&model.state_json)
        .context("invalid stored WorkCompletion state JSON")?;
    if state.status().as_str() != model.state_kind {
        bail!("stored WorkCompletion state discriminator mismatch");
    }
    Ok(WorkCompletionRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        work_unit_id: model.work_unit_id,
        executor_agent_id: model.executor_agent_id,
        revision: u32::try_from(model.revision).context("completion revision must be positive")?,
        content,
        state,
        state_revision: u64::try_from(model.state_revision)
            .context("WorkCompletion state revision is negative")?,
        base_commit: model.base_commit,
        verification_summary: model.verification_summary,
        worktree_path: model.worktree_path,
        branch: model.branch,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

pub(super) fn delivery_from_completion(completion: &WorkCompletionRecord) -> Result<AgentDelivery> {
    if completion.kind() != WorkCompletionKind::Delivery
        || completion.status() != WorkCompletionStatus::Approved
    {
        bail!("merge requires an approved delivery completion");
    }
    let head_commit = completion
        .head_commit()
        .map(str::to_string)
        .context("delivery completion has no head commit")?;
    Ok(AgentDelivery {
        worktree: AgentWorktreeDelivery {
            path: completion.worktree_path.clone(),
            branch: completion.branch.clone(),
        },
        base_commit: completion.base_commit.clone(),
        head_commit,
        changed_files: completion.changed_files().to_vec(),
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
