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
    BlockedRecovery, ReviewRoundCommand, ReviewRoundStateKind, ReviewScope, TaskCommand, TaskRun,
    TaskRunStateKind, TaskStopOrigin, TaskStopReason, TaskWorktreeDisposition, WorkUnitCommand,
    WorkUnitCompletionOutcome, WorkUnitStateKind,
};

use super::review::{review_round_state, update_review_round_state};
use super::work_completion::work_completion_record;
use super::work_unit::{apply_work_unit_command, work_unit_record};

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
            if record.kind().is_terminal() {
                bail!("task stop no longer matches an active TaskRun");
            }
            validate_lease(&tx, &run).await?;
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
        expected_generation: u64,
    ) -> Result<TaskRun> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found while beginning stop")?;
            let record = super::task_run_record(run.clone())?;
            if record.kind().is_terminal() {
                bail!("task stop no longer matches an active TaskRun");
            }
            if !record.is_stop_requested() {
                bail!("task stop must be requested before entering stopping");
            }
            if record.generation() != expected_generation {
                bail!("task stop generation changed before entering stopping");
            }
            validate_lease(&tx, &run).await?;
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
            super::delete_blocked_project_lease(&tx, task_run_id).await?;
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
                || run.project_id != expected.project_id
                || record.status_message() != expected.status_message()
            {
                bail!("merge recovery state changed before retry");
            }
            let now = unix_seconds();
            entities::project_lease::ActiveModel {
                id: Set(new_id("project-lease")),
                task_run_id: Set(run.id.clone()),
                project_id: Set(run.project_id.clone()),
                acquired_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await
            .context("merge recovery could not reacquire the durable project lease")?;

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
            ) || record.design().is_none()
            {
                bail!("task completion requires a finalized design stage");
            }
            if record.is_stop_requested() {
                bail!("task completion is unavailable after stop was requested");
            }
            validate_lease(&tx, &run).await?;
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
                let record = work_unit_record(unit.clone())?;
                if !record.kind().is_terminal() {
                    apply_work_unit_command(
                        &tx,
                        unit,
                        WorkUnitCommand::Cancel {
                            operation_id: format!("task-stop:{task_run_id}:{expected_generation}"),
                            reason: reason.to_string(),
                            disposition: TaskWorktreeDisposition::CleanupRequested,
                        },
                    )
                    .await?;
                }
            }
            for round in entities::review_round::Entity::find()
                .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
                .filter(entities::review_round::Column::StateKind.is_in([
                    ReviewRoundStateKind::PendingDispatch.as_str(),
                    ReviewRoundStateKind::Dispatched.as_str(),
                    ReviewRoundStateKind::Running.as_str(),
                ]))
                .all(&tx)
                .await?
            {
                let current = review_round_state(&round)?;
                let state = current
                    .decide(
                        &round.id,
                        ReviewRoundCommand::Cancel {
                            reviewer_thread_id: current.reviewer_thread_id().map(str::to_string),
                            reason: reason.to_string(),
                            summary: reason.to_string(),
                        },
                    )?
                    .next_state();
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
            if phase != TaskRunStateKind::Stopping || !record.is_stop_requested() {
                bail!("task cancellation requires requested stopping phase");
            }
            if record.generation() != expected_generation {
                bail!("task stop generation changed before cancellation");
            }
            validate_lease(&tx, &run).await?;
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
        .filter(entities::interaction::Column::StateKind.eq("pending"))
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
                interaction.thread_id, interaction.id, interaction.interaction_kind
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
        work_unit_record(unit.clone())
            .map(|record| record.kind() != WorkUnitStateKind::Completed)
            .unwrap_or(true)
    }) {
        bail!("all executor deliveries must be terminal and consumed before completion");
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
            .map(|state| state.kind().is_active())
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
            if phase != TaskRunStateKind::Reviewing {
                bail!("integrated review gate no longer matches task phase")
            }
            let review = reviews
                .iter()
                .find(|review| review.id == *review_round_id)
                .context("integrated review gate round disappeared")?;
            if review.scope != ReviewScope::Integrated.as_str()
                || review.state_kind != ReviewRoundStateKind::Passed.as_str()
                || review.reviewed_head != *reviewed_head
            {
                bail!("integrated review gate no longer identifies a passing current review")
            }
        }
        StudioIntegratedReviewGate::NotRequiredNoDelivery => {
            if !merges.is_empty()
                || units.iter().any(|unit| {
                    work_unit_record(unit.clone())
                        .map(|record| {
                            !matches!(
                                record.completion_outcome(),
                                Some(WorkUnitCompletionOutcome::NoDelivery { .. })
                            )
                        })
                        .unwrap_or(true)
                })
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
    let unit_record = work_unit_record(unit.clone())?;
    if unit.id != work_unit_id
        || !matches!(
            unit_record.completion_outcome(),
            Some(WorkUnitCompletionOutcome::Merged { merge_record_id })
                if merge_record_id == &merge.id
        )
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
    let completion_record = work_completion_record(completion.clone())?;
    if completion.task_run_id != run.id
        || completion.work_unit_id != unit.id
        || completion.revision != i32::try_from(completion_revision)?
        || completion_record.kind().as_str() != "delivery"
        || completion_record.status().as_str() != "approved"
        || completion.base_commit != merge.expected_previous_head
        || completion_record.head_commit() != Some(merge.delivery_head.as_str())
    {
        bail!("single-executor approved completion changed before completion")
    }
    let passing_delivery_reviews = reviews
        .iter()
        .filter(|review| {
            review.scope == ReviewScope::Delivery.as_str()
                && review.state_kind == ReviewRoundStateKind::Passed.as_str()
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
) -> Result<()> {
    let lease = entities::project_lease::Entity::find()
        .filter(entities::project_lease::Column::TaskRunId.eq(run.id.clone()))
        .one(tx)
        .await?
        .context("task project lease not found")?;
    if lease.project_id != run.project_id {
        bail!("TaskRun and project lease drifted before terminalization");
    }
    Ok(())
}

async fn delete_lease(tx: &sea_orm::DatabaseTransaction, task_run_id: &str) -> Result<()> {
    let deleted = entities::project_lease::Entity::delete_many()
        .filter(entities::project_lease::Column::TaskRunId.eq(task_run_id.to_string()))
        .exec(tx)
        .await?;
    if deleted.rows_affected != 1 {
        bail!("terminalization must release exactly one task project lease");
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
