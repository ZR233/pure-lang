use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait, sea_query::Expr,
};

use super::task_run_record;
use super::work_unit::{apply_work_unit_command, work_unit_record, work_unit_state};
use crate::agent::worktree::git_compatible_path;
use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AllocateExecutor, BlockedRecovery, ExecutorAllocation, TaskCommand, TaskRunStateKind,
    TaskSpawnFailure, WorkUnit, WorkUnitCommand, WorkUnitState, WorkUnitStateKind,
};

const MAX_ACTIVE_EXECUTORS: usize = 4;

enum ExecutorAllocationTransition {
    Activate,
    Fail(Box<TaskSpawnFailure>),
}

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
                base_commit: Set("HEAD".to_string()),
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
            ExecutorAllocationTransition::Activate,
        )
        .await
    }

    pub(crate) async fn record_executor_spawn_failure(
        &self,
        work_unit_id: &str,
        agent_id: &str,
        failure: TaskSpawnFailure,
    ) -> Result<()> {
        self.update_executor_allocation(
            work_unit_id,
            agent_id,
            ExecutorAllocationTransition::Fail(Box::new(failure)),
        )
        .await
    }

    pub(crate) async fn record_executor_worktree_base(
        &self,
        work_unit_id: &str,
        agent_id: &str,
        actual_base_commit: &str,
    ) -> Result<WorkUnit> {
        let actual_base_commit = actual_base_commit.trim();
        if actual_base_commit.is_empty() {
            bail!("executor worktree resolved an empty base commit");
        }
        let tx = self.db.begin().await?;
        let model = entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
            .filter(entities::work_unit::Column::ExecutorThreadId.eq(agent_id.to_string()))
            .one(&tx)
            .await?
            .context("executor work unit not found while recording worktree base")?;
        let state = work_unit_state(&model)?;
        if state.kind() != WorkUnitStateKind::Pending {
            bail!("executor worktree base can only be recorded for a pending WorkUnit");
        }
        if model.base_commit == actual_base_commit {
            let work_unit = work_unit_record(model)?;
            tx.commit().await?;
            return Ok(work_unit);
        }
        if model.base_commit != "HEAD" {
            bail!("executor WorkUnit base commit changed before worktree creation completed");
        }
        let next_revision = model
            .revision
            .checked_add(1)
            .context("WorkUnit revision overflow")?;
        let update = entities::work_unit::Entity::update_many()
            .col_expr(
                entities::work_unit::Column::BaseCommit,
                Expr::value(actual_base_commit.to_string()),
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
            .exec(&tx)
            .await?;
        if update.rows_affected != 1 {
            bail!("WorkUnit base commit update lost its revision CAS");
        }
        let updated = entities::work_unit::Entity::find_by_id(model.id)
            .one(&tx)
            .await?
            .context("WorkUnit disappeared after base commit update")?;
        let work_unit = work_unit_record(updated)?;
        tx.commit().await?;
        Ok(work_unit)
    }

    async fn update_executor_allocation(
        &self,
        work_unit_id: &str,
        agent_id: &str,
        transition: ExecutorAllocationTransition,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
            .filter(entities::work_unit::Column::ExecutorThreadId.eq(agent_id.to_string()))
            .one(&tx)
            .await?
            .context("executor work unit not found")?;
        let command = match &transition {
            ExecutorAllocationTransition::Activate => WorkUnitCommand::Activate,
            ExecutorAllocationTransition::Fail(failure) => WorkUnitCommand::FailSpawn {
                failure: Box::new(failure.as_ref().clone()),
            },
        };
        apply_work_unit_command(&tx, work_unit, command).await?;
        if let ExecutorAllocationTransition::Fail(failure) = &transition
            && failure.needs_attention()
        {
            let run = entities::task_run::Entity::find_by_id(
                failure
                    .task_run_id
                    .as_deref()
                    .context("spawn failure omitted its TaskRun owner")?
                    .to_string(),
            )
            .one(&tx)
            .await?
            .context("TaskRun not found while blocking failed executor allocation")?;
            let record = task_run_record(run.clone())?;
            if !record.kind().is_terminal() && record.kind() != TaskRunStateKind::Blocked {
                super::apply_task_command(
                    &tx,
                    run,
                    TaskCommand::Block {
                        message: failure.message.clone(),
                        recovery: BlockedRecovery::ManualOnly,
                    },
                )
                .await?;
                super::delete_blocked_project_lease(&tx, &record.id).await?;
            }
        }
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
        status,
        "pending" | "running" | "waitingReview" | "reviewPassed" | "changesRequired" | "paused"
    )
}
