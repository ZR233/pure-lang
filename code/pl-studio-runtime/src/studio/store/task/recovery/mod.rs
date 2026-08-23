use anyhow::{Context, Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};

use crate::studio::entity as entities;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    ExecutorContinuationStateKind, MergeCleanupState, RestartAgentReconciliation,
    ReviewRoundCommand, ReviewRoundStateKind, ReviewScope, TaskCommand, TaskWorktreeCleanupState,
    TaskWorktreeCreationState, TaskWorktreeDisposition, TaskWorktreeOwnerResource,
    TaskWorktreeOwnerSnapshot, WaitingReviewPhase, WorkCompletionStatus, WorkUnitCommand,
    WorkUnitCompletionOutcome, WorkUnitStateKind,
};

use super::{
    review::{review_round_state, update_review_round_state},
    task_run_record,
    work_completion::work_completion_record,
    work_unit::{apply_work_unit_command, work_unit_record, work_unit_state},
};

const RESTART_DIAGNOSTIC: &str = "agent interrupted by application restart";
const RESTART_BEFORE_CREATE_DIAGNOSTIC: &str =
    "agent interrupted by application restart before worktree creation";
const REVIEW_RESTART_DIAGNOSTIC: &str =
    "reviewer interrupted by application restart before review_exit";

impl StudioStore {
    pub(crate) async fn list_all_task_worktree_owners(
        &self,
    ) -> Result<Vec<TaskWorktreeOwnerSnapshot>> {
        let runs = entities::task_run::Entity::find().all(&self.db).await?;
        self.task_worktree_owners_for_runs(runs).await
    }

    async fn task_worktree_owners_for_runs(
        &self,
        runs: Vec<entities::task_run::Model>,
    ) -> Result<Vec<TaskWorktreeOwnerSnapshot>> {
        let mut snapshots = Vec::with_capacity(runs.len());
        for run in runs {
            let work_units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
                .all(&self.db)
                .await?
                .into_iter()
                .map(work_unit_record)
                .collect::<Result<Vec<_>>>()?;
            let merges = entities::merge_record::Entity::find()
                .filter(entities::merge_record::Column::TaskRunId.eq(run.id.clone()))
                .all(&self.db)
                .await?;
            let completions = entities::work_completion::Entity::find()
                .filter(entities::work_completion::Column::TaskRunId.eq(run.id.clone()))
                .all(&self.db)
                .await?;
            let resources = work_units
                .into_iter()
                .map(|work_unit| -> Result<_> {
                    let creation_state =
                        if work_unit.execution_error() == Some(RESTART_BEFORE_CREATE_DIAGNOSTIC) {
                            TaskWorktreeCreationState::UncreatedBeforeRestart
                        } else {
                            TaskWorktreeCreationState::MustExist
                        };
                    let completion = completions
                        .iter()
                        .filter(|completion| completion.work_unit_id == work_unit.id)
                        .max_by_key(|completion| completion.revision)
                        .cloned()
                        .map(work_completion_record)
                        .transpose()?;
                    let cleanup_state = merge_cleanup_state(&work_unit, &merges)?;
                    Ok(TaskWorktreeOwnerResource {
                        work_unit,
                        completion,
                        creation_state,
                        cleanup_state,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            snapshots.push(TaskWorktreeOwnerSnapshot {
                run: task_run_record(run)?,
                resources,
            });
        }
        Ok(snapshots)
    }

    pub(crate) async fn reconcile_task_agents_after_restart(
        &self,
        task_run_id: &str,
    ) -> Result<RestartAgentReconciliation> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found during agent restart reconciliation")?;
            let work_units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id.to_string()))
                .all(&tx)
                .await?;
            validate_work_units(task_run_id, &work_units)?;

            let cancelled_reviewers =
                reconcile_pending_reviews_after_restart(&tx, &run, &work_units).await?;
            let mut summary = RestartAgentReconciliation {
                cancelled_thread_executions: cancelled_reviewers,
                ..RestartAgentReconciliation::default()
            };
            for work_unit in work_units {
                let record = work_unit_record(work_unit.clone())?;
                let transient_execution = matches!(
                    record.kind(),
                    WorkUnitStateKind::Pending | WorkUnitStateKind::Running
                );
                if record.kind() == WorkUnitStateKind::Pending {
                    apply_work_unit_command(
                        &tx,
                        work_unit,
                        WorkUnitCommand::Cancel {
                            operation_id: format!("restart-before-create:{}", record.id),
                            reason: RESTART_BEFORE_CREATE_DIAGNOSTIC.to_string(),
                            disposition: TaskWorktreeDisposition::CleanupRequested,
                        },
                    )
                    .await?;
                    summary.cancelled_work_units += 1;
                } else if record.kind() == WorkUnitStateKind::Running
                    && record.continuation_state() != ExecutorContinuationStateKind::PendingStart
                {
                    apply_work_unit_command(
                        &tx,
                        work_unit,
                        WorkUnitCommand::Cancel {
                            operation_id: format!("restart-active:{}", record.id),
                            reason: RESTART_DIAGNOSTIC.to_string(),
                            disposition: TaskWorktreeDisposition::Protect,
                        },
                    )
                    .await?;
                }
                if transient_execution {
                    summary.cancelled_thread_executions += 1;
                }
            }
            Ok(summary)
        }
        .await;
        match result {
            Ok(summary) => {
                tx.commit().await?;
                Ok(summary)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }
}

async fn reconcile_pending_reviews_after_restart(
    tx: &sea_orm::DatabaseTransaction,
    run: &entities::task_run::Model,
    work_units: &[entities::work_unit::Model],
) -> Result<usize> {
    let rounds = entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
        .filter(entities::review_round::Column::StateKind.is_in([
            ReviewRoundStateKind::PendingDispatch.as_str(),
            ReviewRoundStateKind::Dispatched.as_str(),
            ReviewRoundStateKind::Running.as_str(),
        ]))
        .all(tx)
        .await?;
    let mut cancelled_reviewers = 0;
    for round in rounds {
        validate_restart_reviewer(&round)?;
        let round_state = review_round_state(&round)?;
        if round_state.reviewer_thread_id().is_some() {
            cancelled_reviewers += 1;
        }
        match ReviewScope::from_str(&round.scope).context("invalid stored review scope")? {
            ReviewScope::Delivery => {
                restore_delivery_review_after_restart(tx, &round, work_units).await?;
            }
            ReviewScope::Integrated => {
                let record = task_run_record(run.clone())?;
                let target_matches = record.state.review_target().is_some_and(|target| {
                    target.review_round_id == round.id
                        && target.reviewed_head == round.reviewed_head
                });
                if round.work_unit_id.is_some()
                    || round.completion_id.is_some()
                    || round.completion_revision.is_some()
                    || !target_matches
                {
                    bail!("pending integrated review does not match the task run");
                }
                super::apply_task_command(
                    tx,
                    run.clone(),
                    TaskCommand::ReturnToWorking {
                        summary: REVIEW_RESTART_DIAGNOSTIC.to_string(),
                    },
                )
                .await?;
            }
        }
        let state = round_state
            .decide(
                &round.id,
                ReviewRoundCommand::Cancel {
                    reviewer_thread_id: round_state.reviewer_thread_id().map(str::to_string),
                    reason: REVIEW_RESTART_DIAGNOSTIC.to_string(),
                    summary: REVIEW_RESTART_DIAGNOSTIC.to_string(),
                },
            )?
            .next_state();
        update_review_round_state(tx, round, state).await?;
    }
    Ok(cancelled_reviewers)
}

fn validate_restart_reviewer(round: &entities::review_round::Model) -> Result<()> {
    let Some(_reviewer_thread_id) = round.reviewer_thread_id.as_deref() else {
        return Ok(());
    };
    if !review_round_state(round)?.kind().is_active() {
        bail!("pending review has an invalid reviewer Thread status");
    }
    Ok(())
}

async fn restore_delivery_review_after_restart(
    tx: &sea_orm::DatabaseTransaction,
    round: &entities::review_round::Model,
    work_units: &[entities::work_unit::Model],
) -> Result<()> {
    let work_unit_id = round
        .work_unit_id
        .as_deref()
        .context("pending delivery review has no work unit")?;
    let completion_id = round
        .completion_id
        .as_deref()
        .context("pending delivery review has no completion")?;
    let completion_revision = round
        .completion_revision
        .context("pending delivery review has no completion revision")?;
    let work_unit = work_units
        .iter()
        .find(|unit| unit.id == work_unit_id)
        .context("pending delivery review work unit not found")?;
    let completion = entities::work_completion::Entity::find_by_id(completion_id)
        .one(tx)
        .await?
        .context("pending delivery review completion not found")?;
    let completion_record = work_completion_record(completion.clone())?;
    let reviewed_head = completion_record
        .head_commit()
        .unwrap_or(work_unit.base_commit.as_str());
    let record = work_unit_record(work_unit.clone())?;
    if !matches!(
        record.waiting_review_phase(),
        Some(WaitingReviewPhase::Reviewing(_))
    ) || completion.task_run_id != round.task_run_id
        || completion.work_unit_id != work_unit.id
        || completion.revision != completion_revision
        || completion_record.status() != WorkCompletionStatus::ReadyForReview
        || round.reviewed_head != reviewed_head
    {
        bail!("pending delivery review target does not match its completion");
    }
    apply_work_unit_command(
        tx,
        work_unit.clone(),
        WorkUnitCommand::ReviewFailed {
            review_round_id: round.id.clone(),
        },
    )
    .await?;
    Ok(())
}

fn merge_cleanup_state(
    work_unit: &crate::studio::task_coordinator::WorkUnit,
    merges: &[entities::merge_record::Model],
) -> Result<TaskWorktreeCleanupState> {
    if !matches!(
        work_unit.completion_outcome(),
        Some(WorkUnitCompletionOutcome::Merged { .. })
    ) {
        return Ok(TaskWorktreeCleanupState::NotMerged);
    }
    let mut matches = Vec::new();
    for merge in merges {
        if merge.work_unit_id == work_unit.id {
            matches.push(merge);
        }
    }
    let merge = match matches.as_slice() {
        [merge] => *merge,
        [] => bail!("merged work unit has no accepted merge evidence"),
        _ => bail!("merged work unit has ambiguous accepted merge evidence"),
    };
    let cleanup: MergeCleanupState = serde_json::from_str(&merge.cleanup_state_json)
        .context("invalid merge cleanup state during recovery")?;
    if cleanup.kind().as_str() != merge.cleanup_state_kind {
        bail!("merge cleanup discriminator mismatch during recovery");
    }
    match cleanup {
        MergeCleanupState::Discarded(_) | MergeCleanupState::AlreadyAbsent(_) => {
            Ok(TaskWorktreeCleanupState::Cleanup)
        }
        MergeCleanupState::Pending(_) | MergeCleanupState::Attempting(_) => {
            Ok(TaskWorktreeCleanupState::Replay {
                merge_id: merge.id.clone(),
            })
        }
        MergeCleanupState::Failed(_) | MergeCleanupState::Deferred(_) => {
            Ok(TaskWorktreeCleanupState::Protect)
        }
    }
}

fn validate_work_units(task_run_id: &str, work_units: &[entities::work_unit::Model]) -> Result<()> {
    for unit in work_units {
        if unit.task_run_id != task_run_id {
            bail!("work unit belongs to another task run");
        }
        if unit.executor_thread_id.is_none() || unit.attempt <= 0 {
            bail!("work unit has no valid executor Thread identity");
        }
        work_unit_state(unit)?;
    }
    Ok(())
}
