use anyhow::{Context, Result};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::studio::entity as entities;
#[cfg(test)]
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
#[cfg(test)]
use crate::studio::task_coordinator::CreateWorkUnit;
use crate::studio::task_coordinator::{
    ExecutorContinuationRequest, ExecutorContinuationState, TaskWorktreeDisposition,
    ThreadExecutionStatus, WorkUnitRecord, WorkUnitStatus,
};

impl StudioStore {
    #[cfg(test)]
    pub(crate) async fn create_work_unit(&self, input: CreateWorkUnit) -> Result<WorkUnitRecord> {
        let now = unix_seconds();
        work_unit_record(
            entities::work_unit::ActiveModel {
                id: Set(new_id("work-unit")),
                task_run_id: Set(input.task_run_id),
                title: Set(input.title),
                status: Set(WorkUnitStatus::Pending.as_str().to_string()),
                scope_hints_json: Set(serde_json::to_string(&input.scope_hints)?),
                base_commit: Set(input.base_commit),
                worktree_path: Set(input.worktree_path),
                branch: Set(input.branch),
                worktree_disposition: Set(TaskWorktreeDisposition::Protect.as_str().to_string()),
                attempt: Set(input.attempt as i32),
                executor_thread_id: Set(None),
                requested_by_call_id: Set(String::new()),
                execution_status: Set(ThreadExecutionStatus::Queued.as_str().to_string()),
                execution_summary: Set(None),
                execution_error: Set(None),
                budget_limit_json: Set(None),
                budget_slice_count: Set(1),
                continuation_state: Set(ExecutorContinuationState::None.as_str().to_string()),
                continuation_source_turn_id: Set(None),
                continuation_revision: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&self.db)
            .await?,
        )
    }

    #[cfg(test)]
    pub(crate) async fn update_work_unit(
        &self,
        work_unit_id: &str,
        status: WorkUnitStatus,
        executor_thread_id: Option<String>,
    ) -> Result<WorkUnitRecord> {
        let model = entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
            .one(&self.db)
            .await?
            .context("work unit not found")?;
        let mut active: entities::work_unit::ActiveModel = model.into();
        active.status = Set(status.as_str().to_string());
        active.executor_thread_id = Set(executor_thread_id);
        active.updated_at = Set(unix_seconds());
        work_unit_record(active.update(&self.db).await?)
    }

    #[cfg(test)]
    pub(crate) async fn read_work_unit(
        &self,
        work_unit_id: &str,
    ) -> Result<Option<WorkUnitRecord>> {
        entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
            .one(&self.db)
            .await?
            .map(work_unit_record)
            .transpose()
    }

    pub(crate) async fn list_work_units(&self, task_run_id: &str) -> Result<Vec<WorkUnitRecord>> {
        entities::work_unit::Entity::find()
            .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id.to_string()))
            .order_by_asc(entities::work_unit::Column::CreatedAt)
            .order_by_asc(entities::work_unit::Column::Id)
            .all(&self.db)
            .await?
            .into_iter()
            .map(work_unit_record)
            .collect()
    }

    pub(crate) async fn find_work_unit_for_executor(
        &self,
        executor_agent_id: &str,
    ) -> Result<Option<WorkUnitRecord>> {
        let work_units = entities::work_unit::Entity::find()
            .filter(entities::work_unit::Column::ExecutorThreadId.eq(executor_agent_id.to_string()))
            .all(&self.db)
            .await?;
        match work_units.as_slice() {
            [] => Ok(None),
            [work_unit] => work_unit_record(work_unit.clone()).map(Some),
            _ => anyhow::bail!("executor Thread owns multiple work units"),
        }
    }

    pub(crate) async fn mark_executor_handoff_needs_attention(
        &self,
        executor_agent_id: &str,
        error: &str,
    ) -> Result<()> {
        let work_unit = entities::work_unit::Entity::find()
            .filter(entities::work_unit::Column::ExecutorThreadId.eq(executor_agent_id.to_string()))
            .one(&self.db)
            .await?
            .context("executor work unit not found")?;
        let mut active: entities::work_unit::ActiveModel = work_unit.into();
        active.status = Set(WorkUnitStatus::NeedsAttention.as_str().to_string());
        active.execution_status = Set(ThreadExecutionStatus::Failed.as_str().to_string());
        active.execution_error = Set(Some(error.to_string()));
        active.continuation_state = Set(ExecutorContinuationState::NeedsAttention
            .as_str()
            .to_string());
        active.updated_at = Set(crate::studio::ids::unix_seconds());
        active.update(&self.db).await?;
        Ok(())
    }

    pub(crate) async fn list_pending_executor_continuations(
        &self,
    ) -> Result<Vec<ExecutorContinuationRequest>> {
        entities::work_unit::Entity::find()
            .filter(
                entities::work_unit::Column::ContinuationState
                    .eq(ExecutorContinuationState::PendingStart.as_str()),
            )
            .order_by_asc(entities::work_unit::Column::UpdatedAt)
            .order_by_asc(entities::work_unit::Column::Id)
            .all(&self.db)
            .await?
            .into_iter()
            .map(|unit| {
                Ok(ExecutorContinuationRequest {
                    agent_id: unit
                        .executor_thread_id
                        .context("pending executor continuation has no executor Thread")?,
                    work_unit_id: unit.id,
                    source_turn_id: unit
                        .continuation_source_turn_id
                        .context("pending executor continuation has no source Turn")?,
                    slice_count: u32::try_from(unit.budget_slice_count)
                        .context("pending executor continuation has invalid slice count")?,
                })
            })
            .collect()
    }

    pub(crate) async fn executor_continuation_turn_id(
        &self,
        continuation: &ExecutorContinuationRequest,
    ) -> Result<Option<String>> {
        let Some(unit) = entities::work_unit::Entity::find_by_id(&continuation.work_unit_id)
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        if unit.executor_thread_id.as_deref() != Some(continuation.agent_id.as_str())
            || unit.continuation_source_turn_id.as_deref()
                != Some(continuation.source_turn_id.as_str())
            || unit.continuation_state != ExecutorContinuationState::PendingStart.as_str()
        {
            return Ok(None);
        }
        let Some(input) = entities::thread_input::Entity::find_by_id(continuation.mail_id())
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        if input.thread_id != continuation.agent_id {
            anyhow::bail!("executor continuation mail belongs to another Thread");
        }
        match input.state.as_str() {
            "queued" => Ok(None),
            "claimed" | "active" | "consumed" => Ok(input.claimed_turn_id.or(Some(input.turn_id))),
            state => anyhow::bail!("executor continuation mail has unknown state {state}"),
        }
    }
}

pub(super) fn work_unit_record(model: entities::work_unit::Model) -> Result<WorkUnitRecord> {
    Ok(WorkUnitRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        title: model.title,
        status: WorkUnitStatus::from_str(&model.status)
            .with_context(|| format!("invalid work unit status: {}", model.status))?,
        scope_hints: serde_json::from_str(&model.scope_hints_json)?,
        base_commit: model.base_commit,
        worktree_path: model.worktree_path,
        branch: model.branch,
        worktree_disposition: TaskWorktreeDisposition::from_str(&model.worktree_disposition)
            .with_context(|| {
                format!(
                    "invalid task worktree disposition: {}",
                    model.worktree_disposition
                )
            })?,
        attempt: model.attempt as u32,
        executor_thread_id: model.executor_thread_id,
        requested_by_call_id: model.requested_by_call_id,
        execution_status: ThreadExecutionStatus::from_str(&model.execution_status).with_context(
            || {
                format!(
                    "invalid Thread execution status: {}",
                    model.execution_status
                )
            },
        )?,
        execution_summary: model.execution_summary,
        execution_error: model.execution_error,
        budget_limit: model
            .budget_limit_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
        budget_slice_count: u32::try_from(model.budget_slice_count)
            .context("budget slice count is negative")?,
        continuation_state: ExecutorContinuationState::from_str(&model.continuation_state)
            .with_context(|| {
                format!(
                    "invalid executor continuation state: {}",
                    model.continuation_state
                )
            })?,
        continuation_source_turn_id: model.continuation_source_turn_id,
        continuation_revision: u64::try_from(model.continuation_revision)
            .context("continuation revision is negative")?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
