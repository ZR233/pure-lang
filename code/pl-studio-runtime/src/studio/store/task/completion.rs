use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};
use std::collections::HashSet;

use crate::StudioIntegratedReviewGate;
use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    ReviewRoundStateKind, ReviewScope, TaskCommand, TaskFailureKind, TaskOutcome, TaskReviewGate,
    TaskRun, TaskRunStateKind, TaskStopOrigin, TaskStopReason, WorkUnitCompletionOutcome,
    WorkUnitStateKind,
};

use super::review::review_round_state;
use super::work_completion::work_completion_record;
use super::work_unit::work_unit_record;

#[derive(Debug, thiserror::Error)]
#[error("task root thread still has {total} pending interactions: {preview:?}")]
pub(in crate::studio) struct PendingTaskInteractions {
    total: usize,
    preview: Vec<String>,
}

impl StudioStore {
    /// 保存不可变停止事件并推进执行代次；主状态保持不变。
    pub(crate) async fn request_task_stop(
        &self,
        task_run_id: &str,
        origin: TaskStopOrigin,
        reason: &TaskStopReason,
    ) -> Result<TaskRun> {
        let tx = self.db.begin().await?;
        let model = entities::task_run::Entity::find_by_id(task_run_id.to_string())
            .one(&tx)
            .await?
            .context("task run not found while requesting stop")?;
        let run = super::task_run_record(model.clone())?;
        if run.kind().is_terminal() {
            bail!("completed TaskRun cannot be stopped");
        }
        let decision = run.decide(TaskCommand::Stop)?;
        let generation = decision.next_state.generation();
        entities::task_stop_event::ActiveModel {
            id: Set(new_id("task-stop")),
            task_run_id: Set(run.id.clone()),
            generation: Set(i64::try_from(generation)?),
            origin: Set(origin.as_str().to_string()),
            reason: Set(reason.as_str().to_string()),
            source_turn_id: Set(None),
            created_at: Set(unix_seconds()),
        }
        .insert(&tx)
        .await?;
        let updated = super::compare_and_swap_task_run(&tx, &model, Some(&decision.next_state))
            .await?
            .context("TaskRun stop generation update lost its revision CAS")?;
        tx.commit().await?;
        super::task_run_record(updated)
    }

    pub(crate) async fn complete_task(
        &self,
        thread_id: &str,
        gate: &StudioIntegratedReviewGate,
        summary: &str,
        expected_revision: u64,
        expected_generation: u64,
    ) -> Result<TaskRun> {
        let summary = summary.trim();
        if summary.is_empty() {
            bail!("task completion summary must not be empty");
        }
        let tx = self.db.begin().await?;
        let model = active_run_for_session(&tx, thread_id).await?;
        let run = super::task_run_record(model.clone())?;
        ensure_version(&run, expected_revision, expected_generation)?;
        let review_gate = validate_completion_children(&tx, &model, run.kind(), gate).await?;
        validate_no_pending_interactions(&tx, &run.root_thread_id).await?;
        validate_no_pending_todo(&tx, &run.root_thread_id).await?;
        let outcome = TaskOutcome::Succeeded {
            summary: summary.to_string(),
            completed_at: unix_seconds(),
            review_gate,
        };
        let updated =
            super::apply_task_command(&tx, model, TaskCommand::Complete { outcome }).await?;
        tx.commit().await?;
        super::task_run_record(updated)
    }

    pub(crate) async fn fail_task(
        &self,
        thread_id: &str,
        summary: &str,
        evidence: &str,
        cause: &str,
        expected_revision: u64,
        expected_generation: u64,
    ) -> Result<TaskRun> {
        let (summary, evidence, cause) = (summary.trim(), evidence.trim(), cause.trim());
        if summary.is_empty() || evidence.is_empty() || cause.is_empty() {
            bail!("failed task completion requires non-empty summary, evidence, and cause");
        }
        let tx = self.db.begin().await?;
        let model = active_run_for_session(&tx, thread_id).await?;
        let run = super::task_run_record(model.clone())?;
        ensure_version(&run, expected_revision, expected_generation)?;
        settle_children_for_failure(&tx, &run, cause).await?;
        let outcome = TaskOutcome::Failed {
            kind: TaskFailureKind::UnableToProceed,
            summary: summary.to_string(),
            evidence: evidence.to_string(),
            cause: cause.to_string(),
            completed_at: unix_seconds(),
        };
        let updated =
            super::apply_task_command(&tx, model, TaskCommand::Complete { outcome }).await?;
        tx.commit().await?;
        super::task_run_record(updated)
    }
}

fn ensure_version(run: &TaskRun, revision: u64, generation: u64) -> Result<()> {
    if run.revision != revision || run.generation() != generation {
        bail!(
            "task version changed: expected revision {revision}/generation {generation}, actual {}/{}",
            run.revision,
            run.generation()
        );
    }
    Ok(())
}

async fn settle_children_for_failure(
    tx: &sea_orm::DatabaseTransaction,
    run: &TaskRun,
    cause: &str,
) -> Result<()> {
    for unit in entities::work_unit::Entity::find()
        .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
        .all(tx)
        .await?
    {
        let record = work_unit_record(unit.clone())?;
        if !record.kind().is_terminal() {
            super::work_unit::apply_work_unit_command(
                tx,
                unit,
                crate::studio::task_coordinator::WorkUnitCommand::FailExecution {
                    operation_id: format!("task-terminal:{}", run.id),
                    detail: cause.to_string(),
                    disposition: crate::studio::task_coordinator::TaskWorktreeDisposition::Protect,
                },
            )
            .await?;
        }
    }
    for round in entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
        .filter(entities::review_round::Column::StateKind.is_in([
            ReviewRoundStateKind::PendingDispatch.as_str(),
            ReviewRoundStateKind::Dispatched.as_str(),
            ReviewRoundStateKind::Running.as_str(),
        ]))
        .all(tx)
        .await?
    {
        let state = review_round_state(&round)?;
        let next = state
            .decide(
                &round.id,
                crate::studio::task_coordinator::ReviewRoundCommand::Fail {
                    reviewer_thread_id: state.reviewer_thread_id().map(str::to_string),
                    error: cause.to_string(),
                    summary: cause.to_string(),
                },
            )?
            .next_state();
        super::review::update_review_round_state(tx, round, next).await?;
    }
    Ok(())
}

async fn validate_no_pending_interactions(
    tx: &sea_orm::DatabaseTransaction,
    root_thread_id: &str,
) -> Result<()> {
    const PREVIEW_LIMIT: usize = 8;
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
        .map(|interaction| format!("{}/{}", interaction.thread_id, interaction.id))
        .collect();
    Err(PendingTaskInteractions { total, preview }.into())
}

async fn validate_no_pending_todo(
    tx: &sea_orm::DatabaseTransaction,
    root_thread_id: &str,
) -> Result<()> {
    let Some(row) = entities::thread_session_state::Entity::find_by_id(root_thread_id.to_string())
        .one(tx)
        .await?
    else {
        return Ok(());
    };
    let state: pl_protocol::AgentWorkingState =
        serde_json::from_str(&row.state_json).context("task planner working state is invalid")?;
    let Some(section) = state
        .sections
        .iter()
        .find(|section| section.id.as_str() == pl_core::CURRENT_TODO_SECTION_ID)
    else {
        return Ok(());
    };
    let todo: pl_protocol::TodoListSnapshot =
        serde_json::from_str(&section.content).context("task planner todo is invalid")?;
    let pending = todo
        .items
        .iter()
        .filter(|item| item.status != pl_protocol::TodoStatus::Completed)
        .map(|item| item.step.as_str())
        .take(8)
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return Ok(());
    }
    bail!(
        "task still has unfinished todo items: {}",
        pending.join(", ")
    )
}

async fn validate_completion_children(
    tx: &sea_orm::DatabaseTransaction,
    run: &entities::task_run::Model,
    state: TaskRunStateKind,
    gate: &StudioIntegratedReviewGate,
) -> Result<TaskReviewGate> {
    if !matches!(
        state,
        TaskRunStateKind::Working | TaskRunStateKind::Reviewing
    ) {
        bail!("successful completion requires working or reviewing state");
    }
    let units = entities::work_unit::Entity::find()
        .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
        .all(tx)
        .await?;
    let superseded = units
        .iter()
        .filter_map(|unit| unit.supersedes_work_unit_id.as_deref())
        .collect::<HashSet<_>>();
    if units
        .iter()
        .filter(|unit| !superseded.contains(unit.id.as_str()))
        .any(|unit| {
            work_unit_record(unit.clone())
                .map(|record| record.kind() != WorkUnitStateKind::Completed)
                .unwrap_or(true)
        })
    {
        bail!("all current work units must be merged or noDelivery before completion");
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
        bail!("all review rounds must be settled before completion");
    }
    match gate {
        StudioIntegratedReviewGate::Required { .. } => bail!("integrated review is still required"),
        StudioIntegratedReviewGate::SatisfiedByReview {
            review_round_id,
            reviewed_head,
        } => {
            if state != TaskRunStateKind::Reviewing {
                bail!("integrated review gate no longer matches task state");
            }
            let review = reviews
                .iter()
                .find(|review| review.id == *review_round_id)
                .context("integrated review round disappeared")?;
            if review.scope != ReviewScope::Integrated.as_str()
                || review.state_kind != ReviewRoundStateKind::Passed.as_str()
                || review.reviewed_head != *reviewed_head
            {
                bail!("integrated review gate no longer identifies a passing review");
            }
            Ok(TaskReviewGate::IntegratedReview {
                review_round_id: review_round_id.clone(),
            })
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
                bail!("no-delivery exemption no longer matches task children");
            }
            Ok(TaskReviewGate::NotRequiredNoDelivery)
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
            Ok(TaskReviewGate::NotRequiredSingleExecutor {
                work_unit_id: work_unit_id.clone(),
            })
        }
    }
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
        bail!("single-executor exemption requires exactly one work unit");
    };
    let [merge] = merges else {
        bail!("single-executor exemption requires exactly one merge record");
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
        bail!("single-executor exemption identity changed before completion");
    }
    if reviews
        .iter()
        .any(|review| review.scope == ReviewScope::Integrated.as_str())
    {
        bail!("single-executor exemption cannot follow an integrated review");
    }
    let completion = entities::work_completion::Entity::find_by_id(merge.completion_id.clone())
        .one(tx)
        .await?
        .context("approved completion disappeared")?;
    let completion_record = work_completion_record(completion.clone())?;
    if completion.task_run_id != run.id
        || completion.work_unit_id != unit.id
        || completion.revision != i32::try_from(completion_revision)?
        || completion_record.kind().as_str() != "delivery"
        || completion_record.status().as_str() != "approved"
    {
        bail!("approved completion changed before completion");
    }
    let passing = reviews
        .iter()
        .filter(|review| {
            review.scope == ReviewScope::Delivery.as_str()
                && review.state_kind == ReviewRoundStateKind::Passed.as_str()
        })
        .collect::<Vec<_>>();
    let [review] = passing.as_slice() else {
        bail!("single-executor exemption requires one passing delivery review");
    };
    if review.work_unit_id.as_deref() != Some(unit.id.as_str())
        || review.completion_id.as_deref() != Some(completion.id.as_str())
        || review.completion_revision != Some(i32::try_from(completion_revision)?)
    {
        bail!("passing delivery review changed before completion");
    }
    Ok(())
}

async fn active_run_for_session(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<entities::task_run::Model> {
    let runs = entities::task_run::Entity::find()
        .filter(entities::task_run::Column::RootThreadId.eq(thread_id.to_string()))
        .filter(entities::task_run::Column::StateKind.ne(TaskRunStateKind::Completed.as_str()))
        .all(tx)
        .await?;
    match runs.as_slice() {
        [run] => Ok(run.clone()),
        [] => bail!("unfinished TaskRun not found"),
        _ => bail!("multiple unfinished TaskRuns found"),
    }
}
