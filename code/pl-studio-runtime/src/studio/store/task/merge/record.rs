use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter,
    QueryOrder, TransactionTrait,
};

use super::super::work_completion::work_completion_record;
use super::super::work_unit::{apply_work_unit_command, work_unit_state};
use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    MergeCleanupState, MergeMethod, MergeRecord, RecordTaskMerge, TaskRunStateKind,
    WorkCompletionKind, WorkCompletionStatus, WorkUnitCommand, WorkUnitStateKind,
};

impl StudioStore {
    pub(crate) async fn record_task_merge(&self, input: RecordTaskMerge) -> Result<MergeRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let runs = entities::task_run::Entity::find()
                .filter(entities::task_run::Column::RootThreadId.eq(input.thread_id.clone()))
                .filter(
                    entities::task_run::Column::StateKind.eq(TaskRunStateKind::Working.as_str()),
                )
                .all(&tx)
                .await?;
            let run = match runs.as_slice() {
                [run] => run.clone(),
                [] => bail!("merging TaskRun not found"),
                _ => bail!("multiple merging TaskRuns found"),
            };
            let work_unit = entities::work_unit::Entity::find_by_id(input.work_unit_id.clone())
                .one(&tx)
                .await?
                .context("merge candidate WorkUnit not found")?;
            let completion =
                entities::work_completion::Entity::find_by_id(input.completion_id.clone())
                    .one(&tx)
                    .await?
                    .context("merge candidate Completion not found")?;
            let executor = entities::thread::Entity::find_by_id(input.executor_agent_id.clone())
                .one(&tx)
                .await?
                .context("executor canonical Thread not found")?;
            let executor_state: pl_core::AgentState = serde_json::from_str(&executor.state_json)?;
            if executor.role != "executor"
                || !matches!(executor_state, pl_core::AgentState::Closed(_))
            {
                bail!("executor must remain canonically closed during merge accounting");
            }
            let work_unit_state = work_unit_state(&work_unit)?;
            let completion_record = work_completion_record(completion.clone())?;
            if work_unit.task_run_id != run.id
                || work_unit.executor_thread_id.as_deref() != Some(input.executor_agent_id.as_str())
                || work_unit_state.kind() != WorkUnitStateKind::ReviewPassed
                || completion.task_run_id != run.id
                || completion.work_unit_id != work_unit.id
                || completion.executor_agent_id != input.executor_agent_id
                || completion.revision != i32::try_from(input.completion_revision)?
                || completion_record.kind() != WorkCompletionKind::Delivery
                || completion_record.status() != WorkCompletionStatus::Approved
            {
                bail!("approved Completion changed before merge accounting");
            }
            let delivery_head = completion_record
                .head_commit()
                .map(str::to_string)
                .context("approved delivery Completion has no head commit")?;
            if let Some(existing) = entities::merge_record::Entity::find()
                .filter(entities::merge_record::Column::TaskRunId.eq(run.id.clone()))
                .filter(entities::merge_record::Column::CompletionId.eq(completion.id.clone()))
                .one(&tx)
                .await?
            {
                if existing.work_unit_id != work_unit.id
                    || existing.completion_revision != i32::try_from(input.completion_revision)?
                    || existing.executor_agent_id != input.executor_agent_id
                    || existing.expected_previous_head != input.expected_previous_head
                    || existing.resulting_head != input.resulting_head
                    || existing.delivery_head != delivery_head
                    || existing.method != input.method.as_str()
                    || existing.summary != input.summary
                {
                    bail!("executor Completion already has a different recorded merge");
                }
                return merge_record(existing);
            }
            if let Some(previous) = entities::merge_record::Entity::find()
                .filter(entities::merge_record::Column::TaskRunId.eq(run.id.clone()))
                .order_by_desc(entities::merge_record::Column::CreatedAt)
                .order_by_desc(entities::merge_record::Column::Id)
                .one(&tx)
                .await?
                && previous.resulting_head != input.expected_previous_head
            {
                bail!("merge ledger expectedPreviousHead does not match the prior resultingHead");
            }

            let now = unix_seconds();
            let merge = entities::merge_record::ActiveModel {
                id: Set(new_id("merge")),
                task_run_id: Set(run.id.clone()),
                work_unit_id: Set(work_unit.id.clone()),
                completion_id: Set(completion.id),
                completion_revision: Set(i32::try_from(input.completion_revision)?),
                executor_agent_id: Set(input.executor_agent_id),
                expected_previous_head: Set(input.expected_previous_head.clone()),
                resulting_head: Set(input.resulting_head.clone()),
                delivery_head: Set(delivery_head),
                method: Set(input.method.as_str().to_string()),
                summary: Set(input.summary),
                cleanup_state_json: Set(serde_json::to_string(&MergeCleanupState::pending())?),
                cleanup_state_kind: NotSet,
                revision: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await?;

            apply_work_unit_command(
                &tx,
                work_unit,
                WorkUnitCommand::CompleteMerge {
                    merge_record_id: merge.id.clone(),
                },
            )
            .await?;

            super::super::compare_and_swap_task_run(&tx, &run, None)
                .await?
                .context("TaskRun merge accounting lost its revision CAS")?;

            merge_record(merge)
        }
        .await;
        match result {
            Ok(record) => {
                tx.commit().await?;
                Ok(record)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn list_merge_records(&self, task_run_id: &str) -> Result<Vec<MergeRecord>> {
        entities::merge_record::Entity::find()
            .filter(entities::merge_record::Column::TaskRunId.eq(task_run_id.to_string()))
            .order_by_asc(entities::merge_record::Column::CreatedAt)
            .order_by_asc(entities::merge_record::Column::Id)
            .all(&self.db)
            .await?
            .into_iter()
            .map(merge_record)
            .collect()
    }
}

pub(crate) fn merge_record(model: entities::merge_record::Model) -> Result<MergeRecord> {
    let cleanup: MergeCleanupState = serde_json::from_str(&model.cleanup_state_json)
        .context("invalid stored merge cleanup state JSON")?;
    if cleanup.kind().as_str() != model.cleanup_state_kind {
        bail!(
            "stored merge cleanup discriminator mismatch: JSON is {}, generated column is {}",
            cleanup.kind().as_str(),
            model.cleanup_state_kind
        );
    }
    Ok(MergeRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        work_unit_id: model.work_unit_id,
        completion_id: model.completion_id,
        completion_revision: u32::try_from(model.completion_revision)?,
        executor_agent_id: model.executor_agent_id,
        expected_previous_head: model.expected_previous_head,
        resulting_head: model.resulting_head,
        delivery_head: model.delivery_head,
        method: MergeMethod::from_str(&model.method)
            .with_context(|| format!("invalid merge method: {}", model.method))?,
        summary: model.summary,
        cleanup,
        revision: u64::try_from(model.revision).context("merge revision is negative")?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
