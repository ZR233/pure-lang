use anyhow::{Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use crate::studio::entity as entities;
use crate::studio::task_coordinator::{ReviewVerdict, TaskRunStateKind};

use super::super::task_run_record;

pub(super) async fn active_implementation_run(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<entities::task_run::Model> {
    let run = active_nonterminal_run(tx, thread_id).await?;
    let record = task_run_record(run.clone())?;
    if !matches!(
        record.kind(),
        TaskRunStateKind::Implementing | TaskRunStateKind::Reworking
    ) || record.is_stop_requested()
    {
        bail!("review request requires implementing or reworking");
    }
    Ok(run)
}

pub(super) async fn active_nonterminal_run(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<entities::task_run::Model> {
    let runs = entities::task_run::Entity::find()
        .filter(entities::task_run::Column::RootThreadId.eq(thread_id.to_string()))
        .filter(entities::task_run::Column::StateKind.is_not_in([
            TaskRunStateKind::Completed.as_str(),
            TaskRunStateKind::Failed.as_str(),
            TaskRunStateKind::Cancelled.as_str(),
        ]))
        .all(tx)
        .await?;
    match runs.as_slice() {
        [run] => Ok(run.clone()),
        [] => bail!("active task run not found"),
        _ => bail!("multiple active task runs found"),
    }
}

pub(super) async fn ensure_no_pending_review(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
) -> Result<()> {
    if entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
        .filter(entities::review_round::Column::StateKind.eq(ReviewVerdict::Pending.as_str()))
        .one(tx)
        .await?
        .is_some()
    {
        bail!("task already has an active reviewer");
    }
    Ok(())
}

pub(super) async fn ensure_review_call_unused(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
    requested_by_call_id: &str,
) -> Result<()> {
    if entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
        .filter(
            entities::review_round::Column::RequestedByCallId.eq(requested_by_call_id.to_string()),
        )
        .one(tx)
        .await?
        .is_some()
    {
        bail!("provider call already authorized a review");
    }
    Ok(())
}

pub(super) async fn ensure_no_pending_delivery_review(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
    work_unit_id: &str,
) -> Result<()> {
    if entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
        .filter(entities::review_round::Column::WorkUnitId.eq(Some(work_unit_id.to_string())))
        .filter(entities::review_round::Column::StateKind.eq(ReviewVerdict::Pending.as_str()))
        .one(tx)
        .await?
        .is_some()
    {
        bail!("work unit already has an active reviewer");
    }
    Ok(())
}

pub(super) async fn pending_review_by_call(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
    requested_by_call_id: &str,
) -> Result<entities::review_round::Model> {
    let rounds = entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
        .filter(
            entities::review_round::Column::RequestedByCallId.eq(requested_by_call_id.to_string()),
        )
        .filter(entities::review_round::Column::StateKind.eq(ReviewVerdict::Pending.as_str()))
        .all(tx)
        .await?;
    match rounds.as_slice() {
        [round] => Ok(round.clone()),
        [] => bail!("pending reviewer authorization not found"),
        _ => bail!("provider call authorized multiple reviews"),
    }
}

pub(super) async fn pending_review_for_reviewer(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
    reviewer_agent_id: &str,
) -> Result<entities::review_round::Model> {
    let rounds = entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
        .filter(entities::review_round::Column::ReviewerThreadId.eq(reviewer_agent_id.to_string()))
        .filter(entities::review_round::Column::StateKind.eq(ReviewVerdict::Pending.as_str()))
        .all(tx)
        .await?;
    match rounds.as_slice() {
        [round] => Ok(round.clone()),
        [] => bail!("pending review not found for reviewer"),
        _ => bail!("reviewer owns multiple pending reviews"),
    }
}

pub(super) async fn finish_transaction<T>(
    tx: sea_orm::DatabaseTransaction,
    result: Result<T>,
) -> Result<T> {
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
