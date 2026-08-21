use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use super::task_run_record;
use super::work_unit::{update_work_unit_state, work_unit_record, work_unit_state};
use crate::agent::worktree::git_compatible_path;
use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AllocateExecutor, ExecutorAllocation, TaskRunStateKind, ThreadExecutionStatus, WorkUnitState,
    WorkUnitStatus,
};

const MAX_ACTIVE_EXECUTORS: usize = 4;

impl StudioStore {
    pub(crate) async fn allocate_executor(
        &self,
        input: AllocateExecutor,
    ) -> Result<ExecutorAllocation> {
        let AllocateExecutor {
            thread_id,
            title,
            mut scope_hints,
            agent_id,
            requested_by_call_id,
        } = input;
        let title = normalize_executor_title(&title)?;
        scope_hints.sort();
        scope_hints.dedup();
        let tx = self.db.begin().await?;
        let run_model = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::RootThreadId.eq(thread_id))
            .filter(entities::task_run::Column::StateKind.is_not_in([
                TaskRunStateKind::Completed.as_str(),
                TaskRunStateKind::Failed.as_str(),
                TaskRunStateKind::Cancelled.as_str(),
            ]))
            .order_by_desc(entities::task_run::Column::UpdatedAt)
            .order_by_desc(entities::task_run::Column::Id)
            .one(&tx)
            .await?
            .context("active task run not found for this session")?;
        let run = task_run_record(run_model)?;
        if run.is_stop_requested() {
            bail!("executor allocation is not allowed after task stop was requested");
        }
        if !matches!(
            run.kind(),
            TaskRunStateKind::Implementing | TaskRunStateKind::Reworking
        ) {
            bail!("executor allocation requires task phase implementing or reworking");
        }
        let existing = entities::work_unit::Entity::find()
            .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
            .all(&tx)
            .await?;
        let scope_hints_json = serde_json::to_string(&scope_hints)?;
        if let Some(existing_unit) = existing
            .iter()
            .find(|unit| unit.requested_by_call_id == requested_by_call_id)
        {
            if existing_unit.executor_thread_id.as_deref() != Some(agent_id.as_str())
                || normalize_executor_title(&existing_unit.title)? != title
                || !stored_scope_matches(existing_unit, &scope_hints)?
            {
                bail!("task executor call id is already owned by a different allocation");
            }
            let work_unit = work_unit_record(existing_unit.clone())?;
            tx.commit().await?;
            return Ok(ExecutorAllocation {
                run,
                work_unit,
                reused: true,
            });
        }
        if run.kind() == TaskRunStateKind::Implementing {
            for existing_unit in existing
                .iter()
                .filter(|unit| is_active_work_unit(&unit.state_kind))
            {
                if normalize_executor_title(&existing_unit.title)? == title
                    && stored_scope_matches(existing_unit, &scope_hints)?
                {
                    let work_unit = work_unit_record(existing_unit.clone())?;
                    tx.commit().await?;
                    return Ok(ExecutorAllocation {
                        run,
                        work_unit,
                        reused: true,
                    });
                }
            }
        }
        let active = existing
            .iter()
            .filter(|unit| is_active_work_unit(&unit.state_kind))
            .collect::<Vec<_>>();
        if active.len() >= MAX_ACTIVE_EXECUTORS {
            bail!("task executor concurrency limit reached: at most 4 active executors");
        }
        let mut previous_attempt = 0;
        for existing_unit in &existing {
            if stored_scope_matches(existing_unit, &scope_hints)? {
                previous_attempt = previous_attempt.max(existing_unit.attempt.max(0) as u32);
            }
        }
        let attempt = previous_attempt
            .checked_add(1)
            .context("executor attempt overflow")?;
        let attempt_i32 =
            i32::try_from(attempt).context("executor attempt exceeds storage range")?;

        let now = unix_seconds();
        let work_unit_id = new_id("work-unit");
        let worktree_path = git_compatible_path(
            std::path::Path::new(&run.workspace_root)
                .join(".pure")
                .join("worktrees")
                .join(&run.id)
                .join(&agent_id),
        )
        .to_string_lossy()
        .to_string();
        let branch = format!("pure-task-{}-{agent_id}", run.id);
        let work_unit = work_unit_record(
            entities::work_unit::ActiveModel {
                id: Set(work_unit_id.clone()),
                task_run_id: Set(run.id.clone()),
                title: Set(title),
                scope_hints_json: Set(scope_hints_json),
                base_commit: Set(run.expected_head.clone()),
                worktree_path: Set(worktree_path),
                branch: Set(branch),
                attempt: Set(attempt_i32),
                executor_thread_id: Set(Some(agent_id)),
                requested_by_call_id: Set(requested_by_call_id),
                state_json: Set(serde_json::to_string(&WorkUnitState::pending())?),
                revision: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(&tx)
            .await?,
        )?;
        tx.commit().await?;
        Ok(ExecutorAllocation {
            run,
            work_unit,
            reused: false,
        })
    }

    pub(crate) async fn activate_executor(&self, work_unit_id: &str, agent_id: &str) -> Result<()> {
        self.update_executor_allocation(
            work_unit_id,
            agent_id,
            WorkUnitStatus::Running,
            ThreadExecutionStatus::Running,
            None,
        )
        .await
    }

    pub(crate) async fn fail_executor(
        &self,
        work_unit_id: &str,
        agent_id: &str,
        error: &str,
    ) -> Result<()> {
        self.update_executor_allocation(
            work_unit_id,
            agent_id,
            WorkUnitStatus::Failed,
            ThreadExecutionStatus::Failed,
            Some(error.to_string()),
        )
        .await
    }

    async fn update_executor_allocation(
        &self,
        work_unit_id: &str,
        agent_id: &str,
        work_unit_status: WorkUnitStatus,
        execution_status: ThreadExecutionStatus,
        error: Option<String>,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
            .filter(entities::work_unit::Column::ExecutorThreadId.eq(agent_id.to_string()))
            .one(&tx)
            .await?
            .context("executor work unit not found")?;
        let state = work_unit_state(&work_unit)?;
        let mut progress = state.into_progress();
        progress.execution_error = error;
        update_work_unit_state(&tx, work_unit, work_unit_status, execution_status, progress)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

fn normalize_executor_title(title: &str) -> Result<String> {
    let normalized = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        bail!("task executor title must not be empty");
    }
    Ok(normalized)
}

fn stored_scope_matches(
    work_unit: &entities::work_unit::Model,
    expected: &[String],
) -> Result<bool> {
    let mut stored = serde_json::from_str::<Vec<String>>(&work_unit.scope_hints_json)
        .context("invalid executor scope hints")?;
    stored.sort();
    stored.dedup();
    Ok(stored == expected)
}

fn is_active_work_unit(status: &str) -> bool {
    matches!(
        WorkUnitStatus::from_str(status),
        Some(
            WorkUnitStatus::Pending
                | WorkUnitStatus::Running
                | WorkUnitStatus::AwaitingCompletion
                | WorkUnitStatus::ReadyForReview
                | WorkUnitStatus::Reviewing
                | WorkUnitStatus::ChangesRequested
                | WorkUnitStatus::Approved
        )
    )
}
