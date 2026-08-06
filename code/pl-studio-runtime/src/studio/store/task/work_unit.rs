use anyhow::{Context, Result};
#[cfg(test)]
use sea_orm::{ActiveModelTrait, ActiveValue::Set};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::entity as entities;
#[cfg(test)]
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
#[cfg(test)]
use crate::studio::task_coordinator::CreateWorkUnit;
use crate::studio::task_coordinator::{
    TaskWorktreeDisposition, ThreadExecutionStatus, WorkUnitRecord, WorkUnitStatus,
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
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
