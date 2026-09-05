//! Thread submission 阶段提交记录的追加与分页查询。

use pl_core::{AgentSubmissionPage, AgentSubmissionRecord, ThreadCommit, ThreadId};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};

use crate::PureError;
use crate::studio::StudioStore;
use crate::studio::entity::thread_submission;

use super::{i64_from_u64, store_error};

pub(super) async fn list_thread_submissions(
    store: &StudioStore,
    thread_id: &ThreadId,
    offset: usize,
    limit: usize,
) -> Result<AgentSubmissionPage, PureError> {
    let thread_id = thread_id.to_string();
    let total = thread_submission::Entity::find()
        .filter(thread_submission::Column::ThreadId.eq(thread_id.clone()))
        .count(store.database())
        .await
        .map_err(store_error)?;
    let limit = limit.max(1);
    let rows = thread_submission::Entity::find()
        .filter(thread_submission::Column::ThreadId.eq(thread_id))
        .order_by_asc(thread_submission::Column::Ordinal)
        .offset(offset as u64)
        .limit(limit as u64)
        .all(store.database())
        .await
        .map_err(store_error)?;
    let items = rows
        .into_iter()
        .map(AgentSubmissionRecord::try_from)
        .collect::<Result<Vec<_>, PureError>>()?;
    let returned = items.len();
    let total_usize = total as usize;
    Ok(AgentSubmissionPage {
        items,
        offset,
        limit,
        total: total_usize,
        has_more: offset + returned < total_usize,
    })
}

/// 在同一事务内追加一条 durable 阶段提交记录（report_progress 触发）。
pub(super) async fn persist_submission(
    tx: &sea_orm::DatabaseTransaction,
    commit: &ThreadCommit,
) -> Result<(), PureError> {
    let Some(submission) = commit.facts.submission.as_ref() else {
        return Ok(());
    };
    let thread_id = commit.agent_id.to_string();
    let next_ordinal = next_submission_ordinal(tx, &thread_id).await?;
    let stage = crate::studio::agent_host::events::progress_stage_label(submission.report.stage)
        .to_string();
    let active = thread_submission::ActiveModel {
        id: Set(crate::studio::ids::new_id("thread_submission")),
        thread_id: Set(thread_id),
        ordinal: Set(next_ordinal),
        stage: Set(stage),
        summary: Set(submission.report.summary.clone()),
        next_step: Set(submission.report.next_step.clone()),
        detail: Set(submission.detail.clone()),
        revision: Set(i64_from_u64(submission.report.revision)?),
        created_at: Set(submission.created_at),
    };
    active.insert(tx).await.map_err(store_error)?;
    Ok(())
}

async fn next_submission_ordinal(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<i64, PureError> {
    let max = thread_submission::Entity::find()
        .filter(thread_submission::Column::ThreadId.eq(thread_id))
        .all(tx)
        .await
        .map_err(store_error)?
        .into_iter()
        .map(|model| model.ordinal)
        .max();
    Ok(max.map_or(0, |ordinal| ordinal.saturating_add(1)))
}
