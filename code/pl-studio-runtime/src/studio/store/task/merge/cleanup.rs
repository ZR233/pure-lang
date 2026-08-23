use anyhow::{Context, Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, sea_query::Expr};

use super::super::task_run_record;
use super::super::work_completion::{delivery_from_completion, work_completion_record};
use super::super::work_unit::work_unit_record;
use super::merge_record;
use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    MergeCleanupCommand, MergeCleanupResult, MergeCleanupState, MergeRecord, TaskMergeScope,
    WorkCompletionStatus, WorkUnitCompletionOutcome, WorkUnitStateKind,
};

impl StudioStore {
    pub(crate) async fn read_accepted_merge_scope(&self, merge_id: &str) -> Result<TaskMergeScope> {
        let merge = entities::merge_record::Entity::find_by_id(merge_id.to_string())
            .one(&self.db)
            .await?
            .context("recorded merge not found")?;
        let run = entities::task_run::Entity::find_by_id(merge.task_run_id.clone())
            .one(&self.db)
            .await?
            .context("recorded merge task run not found")?;
        let work_unit = entities::work_unit::Entity::find_by_id(merge.work_unit_id.clone())
            .one(&self.db)
            .await?
            .context("recorded merge work unit not found")?;
        let completion = entities::work_completion::Entity::find_by_id(merge.completion_id.clone())
            .one(&self.db)
            .await?
            .context("recorded merge completion not found")?;
        let work_unit_record = work_unit_record(work_unit.clone())?;
        let completion_record = work_completion_record(completion.clone())?;
        if work_unit.task_run_id != run.id
            || work_unit.executor_thread_id.as_deref() != Some(merge.executor_agent_id.as_str())
            || work_unit_record.kind() != WorkUnitStateKind::Completed
            || !matches!(
                work_unit_record.completion_outcome(),
                Some(WorkUnitCompletionOutcome::Merged { merge_record_id })
                    if merge_record_id == &merge.id
            )
            || completion.task_run_id != run.id
            || completion.work_unit_id != work_unit.id
            || completion.executor_agent_id != merge.executor_agent_id
            || completion.revision != merge.completion_revision
            || completion_record.status() != WorkCompletionStatus::Approved
        {
            bail!("recorded merge work unit and completion identity drifted");
        }
        let completion = completion_record;
        let delivery = delivery_from_completion(&completion)?;
        if delivery.head_commit != merge.delivery_head {
            bail!("recorded merge delivery head drifted");
        }
        Ok(TaskMergeScope {
            run: task_run_record(run)?,
            work_unit: work_unit_record,
            completion,
            delivery,
            merge: merge_record(merge)?,
        })
    }

    pub(crate) async fn record_merge_cleanup_attempting(
        &self,
        merge_id: &str,
    ) -> Result<MergeRecord> {
        let model = entities::merge_record::Entity::find_by_id(merge_id.to_string())
            .one(&self.db)
            .await?
            .context("merge record not found")?;
        let record = merge_record(model.clone())?;
        if record.cleanup.is_complete()
            || matches!(record.cleanup, MergeCleanupState::Attempting(_))
        {
            return Ok(record);
        }
        let now = unix_seconds();
        let decision = record.decide_cleanup(
            record.revision,
            MergeCleanupCommand::Attempt {
                operation_id: format!("merge-cleanup:{}:{}", record.id, record.revision + 1),
                started_at: now,
            },
        )?;
        persist_cleanup_decision(&self.db, model, decision, now).await
    }

    pub(crate) async fn record_merge_cleanup(
        &self,
        merge_id: &str,
        operation_id: &str,
        result: MergeCleanupResult,
    ) -> Result<MergeRecord> {
        let model = entities::merge_record::Entity::find_by_id(merge_id.to_string())
            .one(&self.db)
            .await?
            .context("merge record not found")?;
        let record = merge_record(model.clone())?;
        let now = unix_seconds();
        let decision = record.decide_cleanup(
            record.revision,
            MergeCleanupCommand::Complete {
                operation_id: operation_id.to_string(),
                completed_at: now,
                result,
            },
        )?;
        persist_cleanup_decision(&self.db, model, decision, now).await
    }
}

async fn persist_cleanup_decision<C>(
    connection: &C,
    model: entities::merge_record::Model,
    decision: crate::studio::task_coordinator::MergeCleanupTransitionDecision,
    now: i64,
) -> Result<MergeRecord>
where
    C: sea_orm::ConnectionTrait,
{
    if !decision.changed() {
        return merge_record(model);
    }
    let state = decision.next_state();
    let result = entities::merge_record::Entity::update_many()
        .col_expr(
            entities::merge_record::Column::CleanupStateJson,
            Expr::value(serde_json::to_string(&state)?),
        )
        .col_expr(
            entities::merge_record::Column::Revision,
            Expr::value(model.revision.saturating_add(1)),
        )
        .col_expr(entities::merge_record::Column::UpdatedAt, Expr::value(now))
        .filter(entities::merge_record::Column::Id.eq(model.id.clone()))
        .filter(entities::merge_record::Column::Revision.eq(model.revision))
        .exec(connection)
        .await?;
    if result.rows_affected != 1 {
        bail!("merge cleanup update lost its revision CAS");
    }
    let model = entities::merge_record::Entity::find_by_id(model.id)
        .one(connection)
        .await?
        .context("merge record disappeared after cleanup update")?;
    merge_record(model)
}
