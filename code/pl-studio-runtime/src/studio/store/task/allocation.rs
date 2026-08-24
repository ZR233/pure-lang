use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use super::task_run_record;
use super::work_unit::work_unit_record;
use crate::agent::worktree::git_compatible_path;
use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AllocateExecutor, ExecutorAllocation, TaskRunStateKind, WorkUnitState,
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
            .filter(entities::task_run::Column::StateKind.ne(TaskRunStateKind::Completed.as_str()))
            .order_by_desc(entities::task_run::Column::UpdatedAt)
            .order_by_desc(entities::task_run::Column::Id)
            .one(&tx)
            .await?
            .context("active task run not found for this session")?;
        let run = task_run_record(run_model.clone())?;
        if run.kind() != TaskRunStateKind::Working {
            bail!("executor allocation requires working state");
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
        if run.kind() == TaskRunStateKind::Working {
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
        let mut previous = None;
        for unit in &existing {
            if normalize_executor_title(&unit.title)? == title
                && stored_scope_matches(unit, &scope_hints)?
                && previous.is_none_or(|current: &entities::work_unit::Model| {
                    (unit.attempt, &unit.id) > (current.attempt, &current.id)
                })
            {
                previous = Some(unit);
            }
        }
        let previous_attempt = previous.map_or(0, |unit| unit.attempt.max(0) as u32);
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
                base_commit: Set("HEAD".to_string()),
                worktree_path: Set(worktree_path),
                branch: Set(branch),
                attempt: Set(attempt_i32),
                supersedes_work_unit_id: Set(previous.map(|unit| unit.id.clone())),
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
        super::compare_and_swap_task_run(&tx, &run_model, None)
            .await?
            .context("TaskRun executor allocation lost its revision CAS")?;
        tx.commit().await?;
        Ok(ExecutorAllocation {
            run,
            work_unit,
            reused: false,
        })
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
        status,
        "pending" | "running" | "waitingReview" | "reviewPassed" | "changesRequired" | "paused"
    )
}
