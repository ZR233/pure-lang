use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::StudioIntegratedReviewGate;
use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    BlockedRecovery, ExecutorContinuationState, ReviewRoundState, ReviewScope, ReviewVerdict,
    TaskCommand, TaskRun, TaskRunStateKind, TaskStopOrigin, TaskStopReason,
    TaskWorktreeDisposition, ThreadExecutionStatus, WorkUnitState, WorkUnitStatus,
};

use super::review::{review_round_state, update_review_round_state};
use super::work_unit::{update_work_unit_state, work_unit_state};

#[derive(Debug, thiserror::Error)]
#[error("task root thread still has {total} pending interactions")]
pub(in crate::studio) struct PendingTaskInteractions {
    total: usize,
    preview: Vec<String>,
}

impl PendingTaskInteractions {
    pub(in crate::studio) fn user_message(&self) -> String {
        let preview = self.preview.join(", ");
        let remaining = self.total.saturating_sub(self.preview.len());
        let suffix = if remaining == 0 {
            String::new()
        } else {
            format!("，另有 {remaining} 条")
        };
        format!(
            "Task root Thread 仍有 {} 条 pending Interaction：{preview}{suffix}；请先解决或取消后重试 task_complete",
            self.total
        )
    }
}

impl StudioStore {
    pub(crate) async fn request_task_stop(
        &self,
        task_run_id: &str,
        expected_head: &str,
        origin: TaskStopOrigin,
        reason: &TaskStopReason,
    ) -> Result<TaskRun> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found while requesting stop")?;
            let record = super::task_run_record(run.clone())?;
            if run.expected_head != expected_head || record.kind().is_terminal() {
                bail!("task stop no longer matches the active task HEAD");
            }
            validate_lease(&tx, &run, expected_head).await?;
            if record.is_stop_requested() {
                return Ok(record);
            }
            let now = unix_seconds();
            let updated = super::apply_task_command(
                &tx,
                run,
                TaskCommand::RequestStop((origin, reason.clone(), now).into()),
            )
            .await?;
            super::task_run_record(updated)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn begin_task_stop(
        &self,
        task_run_id: &str,
        expected_head: &str,
        expected_generation: u64,
    ) -> Result<TaskRun> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found while beginning stop")?;
            let record = super::task_run_record(run.clone())?;
            if run.expected_head != expected_head || record.kind().is_terminal() {
                bail!("task stop no longer matches the active task HEAD");
            }
            if !record.is_stop_requested() {
                bail!("task stop must be requested before entering stopping");
            }
            if record.generation() != expected_generation {
                bail!("task stop generation changed before entering stopping");
            }
            validate_lease(&tx, &run, expected_head).await?;
            if record.kind() != TaskRunStateKind::Stopping {
                bail!("task stop request did not enter the stopping state");
            }
            Ok(record)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn block_task_and_release_lease(
        &self,
        task_run_id: &str,
        reason: &str,
    ) -> Result<TaskRun> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found while blocking")?;
            let record = super::task_run_record(run.clone())?;
            if record.kind().is_terminal() {
                return super::task_run_record(run);
            }
            let recovery = if super::is_retryable_merge_recovery_message(reason) {
                BlockedRecovery::RetryMerge
            } else {
                BlockedRecovery::ManualOnly
            };
            let blocked = super::apply_task_command(
                &tx,
                run,
                TaskCommand::Block {
                    message: reason.to_string(),
                    recovery,
                },
            )
            .await?;
            super::delete_blocked_branch_lease(&tx, task_run_id).await?;
            super::task_run_record(blocked)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn retry_blocked_merge_task(&self, expected: &TaskRun) -> Result<TaskRun> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(expected.id.clone())
                .one(&tx)
                .await?
                .context("blocked merge task run not found while retrying recovery")?;
            let record = super::task_run_record(run.clone())?;
            if record.kind() != TaskRunStateKind::Blocked
                || record.generation() != expected.generation()
                || record.revision != expected.revision
                || run.updated_at != expected.updated_at
                || run.workspace_root != expected.workspace_root
                || run.git_common_dir != expected.git_common_dir
                || run.branch != expected.branch
                || run.expected_head != expected.expected_head
                || record.status_message() != expected.status_message()
            {
                bail!("merge recovery state changed before retry");
            }
            let now = unix_seconds();
            entities::branch_lease::ActiveModel {
                id: Set(new_id("branch-lease")),
                task_run_id: Set(run.id.clone()),
                git_common_dir: Set(run.git_common_dir.clone()),
                branch: Set(run.branch.clone()),
                expected_head: Set(run.expected_head.clone()),
                acquired_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await
            .context("merge recovery could not reacquire the durable branch lease")?;

            let updated = super::apply_task_command(
                &tx,
                run,
                TaskCommand::RecoverBlocked {
                    recovery: BlockedRecovery::RetryMerge,
                    status_message: "retrying blocked merge".to_string(),
                },
            )
            .await?;
            super::task_run_record(updated)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn complete_task(
        &self,
        thread_id: &str,
        expected_head: &str,
        gate: &StudioIntegratedReviewGate,
    ) -> Result<TaskRun> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_run_for_session(&tx, thread_id).await?;
            let record = super::task_run_record(run.clone())?;
            let phase = record.kind();
            if !matches!(
                phase,
                TaskRunStateKind::Implementing
                    | TaskRunStateKind::Reworking
                    | TaskRunStateKind::Reviewing
            ) || run.expected_head != expected_head
                || record.design().is_none()
            {
                bail!("task completion requires a finalized design stage at the current task HEAD");
            }
            if record.is_stop_requested() {
                bail!("task completion is unavailable after stop was requested");
            }
            validate_lease(&tx, &run, expected_head).await?;
            validate_completion_children(&tx, &run, phase, gate).await?;
            validate_no_pending_interactions(&tx, &run.root_thread_id).await?;
            let completed =
                super::write_task_terminal_fact(&tx, run, TaskRunStateKind::Completed, None, None)
                    .await?;
            delete_lease(&tx, &completed.id).await?;
            super::task_run_record(completed)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn settle_agents_for_task_stop(
        &self,
        task_run_id: &str,
        expected_generation: u64,
        reason: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found while stopping agents")?;
            let record = super::task_run_record(run.clone())?;
            if record.kind() != TaskRunStateKind::Stopping || !record.is_stop_requested() {
                bail!("task agents can only be settled after entering requested stopping");
            }
            if record.generation() != expected_generation {
                bail!("task stop generation changed before settling agents");
            }
            for unit in entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id.to_string()))
                .all(&tx)
                .await?
            {
                let state = work_unit_state(&unit)?;
                let status = state.status();
                let cancel = matches!(
                    status,
                    WorkUnitStatus::Pending
                        | WorkUnitStatus::Running
                        | WorkUnitStatus::AwaitingCompletion
                        | WorkUnitStatus::ReadyForReview
                        | WorkUnitStatus::Reviewing
                        | WorkUnitStatus::ChangesRequested
                        | WorkUnitStatus::Approved
                        | WorkUnitStatus::NeedsAttention
                );
                let authorize_cleanup = status != WorkUnitStatus::Merged;
                if cancel || authorize_cleanup {
                    let mut progress = state.clone().into_progress();
                    if cancel {
                        progress.execution_error = Some(reason.to_string());
                        progress.continuation_state = ExecutorContinuationState::None;
                        progress.continuation_source_turn_id = None;
                        progress.continuation_revision =
                            progress.continuation_revision.saturating_add(1);
                    }
                    if authorize_cleanup {
                        progress.worktree_disposition = TaskWorktreeDisposition::CleanupRequested;
                    }
                    let next_state = if cancel {
                        WorkUnitState::cancelled(progress)
                    } else {
                        state.with_progress(progress)
                    };
                    update_work_unit_state(&tx, unit, next_state).await?;
                }
            }
            for round in entities::review_round::Entity::find()
                .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
                .filter(
                    entities::review_round::Column::StateKind.eq(ReviewVerdict::Pending.as_str()),
                )
                .all(&tx)
                .await?
            {
                let _current = review_round_state(&round)?;
                let state = ReviewRoundState::cancelled(reason.to_string(), reason.to_string());
                update_review_round_state(&tx, round, state).await?;
            }
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn cancel_task_and_release_lease(
        &self,
        task_run_id: &str,
        expected_head: &str,
        expected_generation: u64,
        reason: &str,
    ) -> Result<TaskRun> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found")?;
            let record = super::task_run_record(run.clone())?;
            let phase = record.kind();
            if phase.is_terminal() {
                if phase == TaskRunStateKind::Cancelled
                    && record.terminal_generation() == Some(expected_generation)
                {
                    return super::task_run_record(run);
                }
                bail!("task terminal fact belongs to another generation");
            }
            if run.expected_head != expected_head {
                bail!("task stop no longer matches the active task HEAD");
            }
            if phase != TaskRunStateKind::Stopping || !record.is_stop_requested() {
                bail!("task cancellation requires requested stopping phase");
            }
            if record.generation() != expected_generation {
                bail!("task stop generation changed before cancellation");
            }
            validate_lease(&tx, &run, expected_head).await?;
            let cancelled = super::write_task_terminal_fact(
                &tx,
                run,
                TaskRunStateKind::Cancelled,
                Some(reason.to_string()),
                Some(expected_generation),
            )
            .await?;
            delete_lease(&tx, task_run_id).await?;
            super::task_run_record(cancelled)
        }
        .await;
        finish_transaction(tx, result).await
    }
}

async fn validate_no_pending_interactions(
    tx: &sea_orm::DatabaseTransaction,
    root_thread_id: &str,
) -> Result<()> {
    const PREVIEW_LIMIT: usize = 8;
    // 只校验 root 自身：子 Thread 的 WorkUnit 已在完成门禁中强制结算，
    // 其残留 Interaction 不再阻塞完成。
    let pending = entities::interaction::Entity::find()
        .filter(entities::interaction::Column::ThreadId.eq(root_thread_id.to_string()))
        .filter(entities::interaction::Column::Status.eq("pending"))
        .order_by_asc(entities::interaction::Column::CreatedAt)
        .order_by_asc(entities::interaction::Column::Id)
        .all(tx)
        .await?;
    if pending.is_empty() {
        return Ok(());
    }
    let total = pending.len();
    let preview = pending
        .into_iter()
        .take(PREVIEW_LIMIT)
        .map(|interaction| {
            format!(
                "{}/{} ({})",
                interaction.thread_id, interaction.id, interaction.kind
            )
        })
        .collect();
    Err(PendingTaskInteractions { total, preview }.into())
}

async fn validate_completion_children(
    tx: &sea_orm::DatabaseTransaction,
    run: &entities::task_run::Model,
    phase: TaskRunStateKind,
    gate: &StudioIntegratedReviewGate,
) -> Result<()> {
    let units = entities::work_unit::Entity::find()
        .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
        .all(tx)
        .await?;
    if units.iter().any(|unit| {
        work_unit_state(unit)
            .map(|state| {
                !matches!(
                    state.status(),
                    WorkUnitStatus::Merged | WorkUnitStatus::NoDelivery
                )
            })
            .unwrap_or(true)
    }) {
        bail!("all executor deliveries must be terminal and consumed before completion");
    }
    if units.iter().any(|unit| {
        work_unit_state(unit)
            .map(|state| {
                matches!(
                    state.execution_status(),
                    ThreadExecutionStatus::Queued | ThreadExecutionStatus::Running
                )
            })
            .unwrap_or(true)
    }) {
        bail!("all task agents must be terminal before completion");
    }
    let merges = entities::merge_record::Entity::find()
        .filter(entities::merge_record::Column::TaskRunId.eq(run.id.clone()))
        .all(tx)
        .await?;
    let reviews = entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
        .all(tx)
        .await?;
    if reviews.iter().any(|review| {
        review_round_state(review)
            .map(|state| {
                matches!(
                    state.reviewer_status(),
                    ThreadExecutionStatus::Queued | ThreadExecutionStatus::Running
                )
            })
            .unwrap_or(true)
    }) {
        bail!("all task reviewers must be terminal before completion");
    }
    match gate {
        StudioIntegratedReviewGate::Required { .. } => {
            bail!("integrated review is still required")
        }
        StudioIntegratedReviewGate::SatisfiedByReview {
            review_round_id,
            reviewed_head,
        } => {
            if phase != TaskRunStateKind::Reviewing || reviewed_head != &run.expected_head {
                bail!("integrated review gate no longer matches task phase or HEAD")
            }
            let review = reviews
                .iter()
                .find(|review| review.id == *review_round_id)
                .context("integrated review gate round disappeared")?;
            if review.scope != ReviewScope::Integrated.as_str()
                || review.state_kind != ReviewVerdict::Pass.as_str()
                || review.reviewed_head != run.expected_head
            {
                bail!("integrated review gate no longer identifies a passing current review")
            }
        }
        StudioIntegratedReviewGate::NotRequiredNoDelivery => {
            if !merges.is_empty()
                || units
                    .iter()
                    .any(|unit| unit.state_kind != WorkUnitStatus::NoDelivery.as_str())
            {
                bail!("no-delivery review exemption no longer matches task children")
            }
        }
        StudioIntegratedReviewGate::NotRequiredSingleExecutorEquivalent {
            work_unit_id,
            completion_revision,
            merge_record_id,
        } => {
            validate_single_executor_gate(SingleExecutorGateValidation {
                tx,
                run,
                units: &units,
                merges: &merges,
                reviews: &reviews,
                work_unit_id,
                completion_revision: *completion_revision,
                merge_record_id,
            })
            .await?;
        }
    }
    Ok(())
}

struct SingleExecutorGateValidation<'a> {
    tx: &'a sea_orm::DatabaseTransaction,
    run: &'a entities::task_run::Model,
    units: &'a [entities::work_unit::Model],
    merges: &'a [entities::merge_record::Model],
    reviews: &'a [entities::review_round::Model],
    work_unit_id: &'a str,
    completion_revision: u32,
    merge_record_id: &'a str,
}

async fn validate_single_executor_gate(validation: SingleExecutorGateValidation<'_>) -> Result<()> {
    let SingleExecutorGateValidation {
        tx,
        run,
        units,
        merges,
        reviews,
        work_unit_id,
        completion_revision,
        merge_record_id,
    } = validation;
    let [unit] = units else {
        bail!("single-executor review exemption requires exactly one work unit")
    };
    let [merge] = merges else {
        bail!("single-executor review exemption requires exactly one merge record")
    };
    if unit.id != work_unit_id
        || unit.state_kind != WorkUnitStatus::Merged.as_str()
        || merge.id != merge_record_id
        || merge.work_unit_id != unit.id
        || merge.completion_revision != i32::try_from(completion_revision)?
    {
        bail!("single-executor review exemption identity changed before completion")
    }
    if reviews
        .iter()
        .any(|review| review.scope == ReviewScope::Integrated.as_str())
    {
        bail!("an integrated review round already exists")
    }
    let completion = entities::work_completion::Entity::find_by_id(merge.completion_id.clone())
        .one(tx)
        .await?
        .context("single-executor approved completion disappeared")?;
    if completion.task_run_id != run.id
        || completion.work_unit_id != unit.id
        || completion.revision != i32::try_from(completion_revision)?
        || completion.kind != "delivery"
        || completion.status != "approved"
        || completion.base_commit != merge.expected_previous_head
        || completion.head_commit.as_deref() != Some(merge.delivery_head.as_str())
    {
        bail!("single-executor approved completion changed before completion")
    }
    let passing_delivery_reviews = reviews
        .iter()
        .filter(|review| {
            review.scope == ReviewScope::Delivery.as_str()
                && review.state_kind == ReviewVerdict::Pass.as_str()
        })
        .collect::<Vec<_>>();
    let [review] = passing_delivery_reviews.as_slice() else {
        bail!("single-executor review exemption requires exactly one passing delivery review")
    };
    if review.work_unit_id.as_deref() != Some(unit.id.as_str())
        || review.completion_id.as_deref() != Some(completion.id.as_str())
        || review.completion_revision != Some(i32::try_from(completion_revision)?)
        || review.reviewed_head != merge.delivery_head
    {
        bail!("passing delivery review changed before completion")
    }
    Ok(())
}

async fn active_run_for_session(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<entities::task_run::Model> {
    let runs = entities::task_run::Entity::find()
        .filter(entities::task_run::Column::RootThreadId.eq(thread_id.to_string()))
        .filter(entities::task_run::Column::StateKind.is_in([
            TaskRunStateKind::Implementing.as_str(),
            TaskRunStateKind::Reworking.as_str(),
            TaskRunStateKind::Reviewing.as_str(),
        ]))
        .all(tx)
        .await?;
    match runs.as_slice() {
        [run] => Ok(run.clone()),
        [] => bail!("completable task run not found"),
        _ => bail!("multiple completable task runs found"),
    }
}

async fn validate_lease(
    tx: &sea_orm::DatabaseTransaction,
    run: &entities::task_run::Model,
    expected_head: &str,
) -> Result<()> {
    let lease = entities::branch_lease::Entity::find()
        .filter(entities::branch_lease::Column::TaskRunId.eq(run.id.clone()))
        .one(tx)
        .await?
        .context("task branch lease not found")?;
    if lease.expected_head != expected_head
        || lease.branch != run.branch
        || lease.git_common_dir != run.git_common_dir
    {
        bail!("task run and branch lease drifted before terminalization");
    }
    Ok(())
}

async fn delete_lease(tx: &sea_orm::DatabaseTransaction, task_run_id: &str) -> Result<()> {
    let deleted = entities::branch_lease::Entity::delete_many()
        .filter(entities::branch_lease::Column::TaskRunId.eq(task_run_id.to_string()))
        .exec(tx)
        .await?;
    if deleted.rows_affected != 1 {
        bail!("terminalization must release exactly one task branch lease");
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
