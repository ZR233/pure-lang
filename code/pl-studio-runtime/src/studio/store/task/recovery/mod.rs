use anyhow::{Context, Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};

use crate::studio::entity as entities;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    ExecutorContinuationState, RestartAgentReconciliation, ReviewRoundState, ReviewScope,
    ReviewVerdict, TaskCommand, TaskRunStateKind, TaskWorktreeCleanupState,
    TaskWorktreeCreationState, TaskWorktreeOwnerResource, TaskWorktreeOwnerSnapshot,
    ThreadExecutionStatus, WorkCompletionStatus, WorkUnitState, WorkUnitStatus,
};

use super::{
    review::{review_round_state, update_review_round_state},
    task_run_record,
    work_completion::work_completion_record,
    work_unit::{update_work_unit_state, work_unit_record, work_unit_state},
};

const RESTART_DIAGNOSTIC: &str = "agent interrupted by application restart";
const RESTART_BEFORE_CREATE_DIAGNOSTIC: &str =
    "agent interrupted by application restart before worktree creation";
const REVIEW_RESTART_DIAGNOSTIC: &str =
    "reviewer interrupted by application restart before review_exit";

impl StudioStore {
    pub(crate) async fn clear_task_stop_for_recovery(
        &self,
        task_run_id: &str,
        expected_generation: u64,
        expected_phase: TaskRunStateKind,
        expected_head: &str,
    ) -> Result<bool> {
        if !matches!(
            expected_phase,
            TaskRunStateKind::DesignUpdating
                | TaskRunStateKind::Implementing
                | TaskRunStateKind::Reworking
        ) {
            bail!(
                "Task recovery cannot clear StopRequested during phase {}",
                expected_phase.as_str()
            );
        }
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("Task recovery run not found")?;
            let record = task_run_record(run.clone())?;
            if record.generation() != expected_generation
                || record.kind() != expected_phase
                || run.expected_head != expected_head
                || record.kind().is_terminal()
            {
                bail!("Task recovery facts changed before StopRequested could be cleared");
            }
            let lease = entities::branch_lease::Entity::find()
                .filter(entities::branch_lease::Column::TaskRunId.eq(task_run_id.to_string()))
                .one(&tx)
                .await?
                .context("Task recovery branch lease not found")?;
            if lease.expected_head != expected_head
                || lease.branch != run.branch
                || lease.git_common_dir != run.git_common_dir
            {
                bail!("Task recovery branch lease changed before StopRequested clear");
            }
            if !record.is_stop_requested() {
                return Ok(false);
            }
            bail!("Task recovery cannot clear a stop after the run entered stopping")
        }
        .await;
        match result {
            Ok(cleared) => {
                tx.commit().await?;
                Ok(cleared)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn list_all_task_worktree_owners(
        &self,
    ) -> Result<Vec<TaskWorktreeOwnerSnapshot>> {
        let mut common_dirs = entities::task_run::Entity::find()
            .all(&self.db)
            .await?
            .into_iter()
            .map(|run| run.git_common_dir)
            .collect::<Vec<_>>();
        common_dirs.sort();
        common_dirs.dedup();
        let mut owners = Vec::new();
        for common_dir in common_dirs {
            owners.extend(
                self.list_task_worktree_owners_by_git_common_dir(&common_dir)
                    .await?,
            );
        }
        Ok(owners)
    }

    pub(crate) async fn list_task_worktree_owners_by_git_common_dir(
        &self,
        git_common_dir: &str,
    ) -> Result<Vec<TaskWorktreeOwnerSnapshot>> {
        let runs = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::GitCommonDir.eq(git_common_dir.to_string()))
            .all(&self.db)
            .await?;
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
                let state = work_unit_state(&work_unit)?;
                let status = state.status();
                let execution_status = state.execution_status();
                let transient_execution = is_transient_execution(execution_status);
                let continuation_state = state.progress().continuation_state;
                if status == WorkUnitStatus::Pending {
                    let mut progress = state.into_progress();
                    progress.execution_error = Some(RESTART_BEFORE_CREATE_DIAGNOSTIC.to_string());
                    update_work_unit_state(&tx, work_unit, WorkUnitState::cancelled(progress))
                        .await?;
                    summary.cancelled_work_units += 1;
                } else if (status == WorkUnitStatus::Running
                    && continuation_state != ExecutorContinuationState::PendingStart)
                    || transient_execution
                {
                    let mut progress = state.into_progress();
                    progress.execution_error = Some(RESTART_DIAGNOSTIC.to_string());
                    update_work_unit_state(
                        &tx,
                        work_unit,
                        WorkUnitState::awaiting_cancelled(progress),
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
        .filter(entities::review_round::Column::StateKind.eq(ReviewVerdict::Pending.as_str()))
        .all(tx)
        .await?;
    let mut cancelled_reviewers = 0;
    for round in rounds {
        validate_restart_reviewer(&round)?;
        let round_state = review_round_state(&round)?;
        if round.reviewer_thread_id.is_some()
            && matches!(
                round_state.reviewer_status(),
                ThreadExecutionStatus::Queued | ThreadExecutionStatus::Running
            )
        {
            cancelled_reviewers += 1;
        }
        match ReviewScope::from_str(&round.scope).context("invalid stored review scope")? {
            ReviewScope::Delivery => {
                restore_delivery_review_after_restart(tx, &round, work_units).await?;
            }
            ReviewScope::Integrated => {
                if round.work_unit_id.is_some()
                    || round.completion_id.is_some()
                    || round.completion_revision.is_some()
                    || round.reviewed_head != run.expected_head
                    || task_run_record(run.clone())?.kind() != TaskRunStateKind::Reviewing
                {
                    bail!("pending integrated review does not match the task run");
                }
                super::apply_task_command(
                    tx,
                    run.clone(),
                    TaskCommand::BeginReworking {
                        status_message: REVIEW_RESTART_DIAGNOSTIC.to_string(),
                    },
                )
                .await?;
            }
        }
        let state = ReviewRoundState::cancelled(
            REVIEW_RESTART_DIAGNOSTIC.to_string(),
            REVIEW_RESTART_DIAGNOSTIC.to_string(),
        );
        update_review_round_state(tx, round, state).await?;
    }
    Ok(cancelled_reviewers)
}

fn validate_restart_reviewer(round: &entities::review_round::Model) -> Result<()> {
    let Some(_reviewer_thread_id) = round.reviewer_thread_id.as_deref() else {
        return Ok(());
    };
    if !matches!(
        review_round_state(round)?.reviewer_status(),
        ThreadExecutionStatus::Queued | ThreadExecutionStatus::Running
    ) {
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
    let reviewed_head = completion
        .head_commit
        .as_deref()
        .unwrap_or(work_unit.base_commit.as_str());
    let state = work_unit_state(work_unit)?;
    if state.status() != WorkUnitStatus::Reviewing
        || completion.task_run_id != round.task_run_id
        || completion.work_unit_id != work_unit.id
        || completion.revision != completion_revision
        || completion.status != WorkCompletionStatus::ReadyForReview.as_str()
        || round.reviewed_head != reviewed_head
    {
        bail!("pending delivery review target does not match its completion");
    }
    let progress = state.into_progress();
    update_work_unit_state(
        tx,
        work_unit.clone(),
        WorkUnitState::ready_for_review(progress),
    )
    .await?;
    Ok(())
}

fn merge_cleanup_state(
    work_unit: &crate::studio::task_coordinator::WorkUnit,
    merges: &[entities::merge_record::Model],
) -> Result<TaskWorktreeCleanupState> {
    if work_unit.status() != WorkUnitStatus::Merged {
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
    match merge.cleanup_status.as_str() {
        "discarded" | "alreadyAbsent" => Ok(TaskWorktreeCleanupState::Cleanup),
        "pending" | "attempting" => Ok(TaskWorktreeCleanupState::Replay {
            merge_id: merge.id.clone(),
        }),
        "failed" | "deferred" => Ok(TaskWorktreeCleanupState::Protect),
        status => bail!("accepted merge has unknown cleanup status `{status}`"),
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

fn is_transient_execution(status: ThreadExecutionStatus) -> bool {
    matches!(
        status,
        ThreadExecutionStatus::Queued | ThreadExecutionStatus::Running
    )
}
