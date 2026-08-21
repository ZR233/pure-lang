use anyhow::{Context, Result};
use pl_protocol::AgentWorkingState;
#[cfg(test)]
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, sea_query::Expr,
};

use crate::studio::entity as entities;
#[cfg(test)]
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
#[cfg(test)]
use crate::studio::task_coordinator::CreateWorkUnit;
use crate::studio::task_coordinator::{
    ExecutorContinuationRequest, ExecutorContinuationState, TASK_EXECUTOR_HANDOFF_SECTION_ID,
    TaskExecutorHandoff, ThreadExecutionStatus, WorkUnitContext, WorkUnitProgress, WorkUnitRecord,
    WorkUnitState, WorkUnitStatus, decode_work_unit_state,
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
                scope_hints_json: Set(serde_json::to_string(&input.scope_hints)?),
                base_commit: Set(input.base_commit),
                worktree_path: Set(input.worktree_path),
                branch: Set(input.branch),
                attempt: Set(input.attempt as i32),
                executor_thread_id: Set(None),
                requested_by_call_id: Set(String::new()),
                state_json: Set(serde_json::to_string(&WorkUnitState::pending())?),
                revision: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
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
        let state = work_unit_state(&model)?;
        let execution = default_execution_status(status);
        let state = WorkUnitState::from_parts(status, execution, state.into_progress())?;
        let next_revision = model
            .revision
            .checked_add(1)
            .context("WorkUnit revision overflow")?;
        let result = entities::work_unit::Entity::update_many()
            .col_expr(
                entities::work_unit::Column::ExecutorThreadId,
                Expr::value(executor_thread_id),
            )
            .col_expr(
                entities::work_unit::Column::StateJson,
                Expr::value(serde_json::to_string(&state)?),
            )
            .col_expr(
                entities::work_unit::Column::Revision,
                Expr::value(next_revision),
            )
            .col_expr(
                entities::work_unit::Column::UpdatedAt,
                Expr::value(unix_seconds()),
            )
            .filter(entities::work_unit::Column::Id.eq(model.id.clone()))
            .filter(entities::work_unit::Column::Revision.eq(model.revision))
            .exec(&self.db)
            .await?;
        if result.rows_affected != 1 {
            anyhow::bail!("WorkUnit test update lost its revision CAS");
        }
        work_unit_record(
            entities::work_unit::Entity::find_by_id(model.id)
                .one(&self.db)
                .await?
                .context("WorkUnit disappeared after test update")?,
        )
    }

    #[cfg(test)]
    pub(crate) async fn update_work_unit_state_for_test(
        &self,
        work_unit_id: &str,
        status: WorkUnitStatus,
        execution: ThreadExecutionStatus,
        progress: WorkUnitProgress,
    ) -> Result<WorkUnitRecord> {
        let model = entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
            .one(&self.db)
            .await?
            .context("work unit not found")?;
        work_unit_record(
            update_work_unit_state(&self.db, model, status, execution, progress).await?,
        )
    }

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

    pub(crate) async fn read_work_unit_handoff(
        &self,
        work_unit_id: &str,
    ) -> Result<Option<(WorkUnitRecord, TaskExecutorHandoff)>> {
        let Some(work_unit) = self.read_work_unit(work_unit_id).await? else {
            return Ok(None);
        };
        let executor_thread_id = work_unit
            .executor_thread_id
            .as_deref()
            .context("executor work unit has no executor Thread identity")?;
        let row =
            entities::thread_session_state::Entity::find_by_id(executor_thread_id.to_string())
                .one(&self.db)
                .await?
                .context("executor session state is missing")?;
        let state =
            AgentWorkingState::try_from(row).map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let sections = state
            .sections
            .iter()
            .filter(|section| section.id.as_str() == TASK_EXECUTOR_HANDOFF_SECTION_ID)
            .collect::<Vec<_>>();
        let section = match sections.as_slice() {
            [section] => *section,
            [] => anyhow::bail!("executor session has no Task handoff"),
            _ => anyhow::bail!("executor session has duplicate Task handoff sections"),
        };
        let handoff = TaskExecutorHandoff::from_context_section(section)?;
        let run = self
            .read_task_run(&work_unit.task_run_id)
            .await?
            .context("Task run for executor handoff is missing")?;
        handoff.validate_owner(&run, &work_unit, executor_thread_id)?;
        Ok(Some((work_unit, handoff)))
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
        let state = work_unit_state(&work_unit)?;
        let mut progress = state.into_progress();
        progress.execution_error = Some(error.to_string());
        progress.continuation_state = ExecutorContinuationState::NeedsAttention;
        update_work_unit_state(
            &self.db,
            work_unit,
            WorkUnitStatus::NeedsAttention,
            ThreadExecutionStatus::BudgetLimited,
            progress,
        )
        .await?;
        Ok(())
    }

    pub(crate) async fn list_pending_executor_continuations(
        &self,
    ) -> Result<Vec<ExecutorContinuationRequest>> {
        let units = entities::work_unit::Entity::find()
            .order_by_asc(entities::work_unit::Column::UpdatedAt)
            .order_by_asc(entities::work_unit::Column::Id)
            .all(&self.db)
            .await?;
        let mut requests = Vec::new();
        for unit in units {
            let state = work_unit_state(&unit)?;
            let progress = state.progress();
            if progress.continuation_state != ExecutorContinuationState::PendingStart {
                continue;
            }
            requests.push(ExecutorContinuationRequest {
                agent_id: unit
                    .executor_thread_id
                    .context("pending executor continuation has no executor Thread")?,
                work_unit_id: unit.id,
                source_turn_id: progress
                    .continuation_source_turn_id
                    .clone()
                    .context("pending executor continuation has no source Turn")?,
                slice_count: progress.budget_slice_count,
            });
        }
        Ok(requests)
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
        let state = work_unit_state(&unit)?;
        if unit.executor_thread_id.as_deref() != Some(continuation.agent_id.as_str())
            || state.progress().continuation_source_turn_id.as_deref()
                != Some(continuation.source_turn_id.as_str())
            || state.progress().continuation_state != ExecutorContinuationState::PendingStart
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
    let state = work_unit_state(&model)?;
    Ok(WorkUnitRecord {
        context: WorkUnitContext {
            id: model.id,
            task_run_id: model.task_run_id,
            title: model.title,
            scope_hints: serde_json::from_str(&model.scope_hints_json)?,
            base_commit: model.base_commit,
            worktree_path: model.worktree_path,
            branch: model.branch,
            attempt: u32::try_from(model.attempt).context("work unit attempt is negative")?,
            executor_thread_id: model.executor_thread_id,
            requested_by_call_id: model.requested_by_call_id,
        },
        state,
        revision: u64::try_from(model.revision).context("work unit revision is negative")?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

pub(super) fn work_unit_state(model: &entities::work_unit::Model) -> Result<WorkUnitState> {
    let state = decode_work_unit_state(&model.state_json)?;
    if state.status().as_str() != model.state_kind {
        anyhow::bail!(
            "stored WorkUnit state discriminator mismatch: JSON is {}, generated column is {}",
            state.status().as_str(),
            model.state_kind
        );
    }
    Ok(state)
}

pub(super) async fn update_work_unit_state<C>(
    connection: &C,
    model: entities::work_unit::Model,
    status: WorkUnitStatus,
    execution: ThreadExecutionStatus,
    progress: WorkUnitProgress,
) -> Result<entities::work_unit::Model>
where
    C: ConnectionTrait,
{
    let next_state = WorkUnitState::from_parts(status, execution, progress)?;
    let next_revision = model
        .revision
        .checked_add(1)
        .context("WorkUnit revision overflow")?;
    let result = entities::work_unit::Entity::update_many()
        .col_expr(
            entities::work_unit::Column::StateJson,
            Expr::value(serde_json::to_string(&next_state)?),
        )
        .col_expr(
            entities::work_unit::Column::Revision,
            Expr::value(next_revision),
        )
        .col_expr(
            entities::work_unit::Column::UpdatedAt,
            Expr::value(crate::studio::ids::unix_seconds()),
        )
        .filter(entities::work_unit::Column::Id.eq(model.id.clone()))
        .filter(entities::work_unit::Column::Revision.eq(model.revision))
        .exec(connection)
        .await?;
    if result.rows_affected != 1 {
        anyhow::bail!("WorkUnit state update lost its revision CAS");
    }
    entities::work_unit::Entity::find_by_id(model.id)
        .one(connection)
        .await?
        .context("WorkUnit disappeared after state update")
}

#[cfg(test)]
fn default_execution_status(status: WorkUnitStatus) -> ThreadExecutionStatus {
    match status {
        WorkUnitStatus::Pending => ThreadExecutionStatus::Queued,
        WorkUnitStatus::Running => ThreadExecutionStatus::Running,
        WorkUnitStatus::AwaitingCompletion
        | WorkUnitStatus::ReadyForReview
        | WorkUnitStatus::Reviewing
        | WorkUnitStatus::ChangesRequested
        | WorkUnitStatus::Approved
        | WorkUnitStatus::Merged
        | WorkUnitStatus::NoDelivery => ThreadExecutionStatus::Completed,
        WorkUnitStatus::NeedsAttention => ThreadExecutionStatus::BudgetLimited,
        WorkUnitStatus::Failed => ThreadExecutionStatus::Failed,
        WorkUnitStatus::Cancelled => ThreadExecutionStatus::Cancelled,
    }
}
