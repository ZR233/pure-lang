use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use super::outcome::agent_outcome_record;
use super::task_run_record;
use super::work_unit::work_unit_record;
use crate::agent::worktree::git_compatible_path;
use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentOutcomeStatus, AllocateExecutor, ExecutorAllocation, TaskRunPhase,
    TaskWorktreeDisposition, WorkUnitStatus, owned_paths_overlap,
};

const MAX_ACTIVE_EXECUTORS: usize = 4;

impl StudioStore {
    pub(crate) async fn allocate_executor(
        &self,
        input: AllocateExecutor,
    ) -> Result<ExecutorAllocation> {
        let tx = self.db.begin().await?;
        let run_model = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::SessionId.eq(input.session_id))
            .filter(entities::task_run::Column::Phase.is_not_in([
                TaskRunPhase::Completed.as_str(),
                TaskRunPhase::Blocked.as_str(),
                TaskRunPhase::Failed.as_str(),
                TaskRunPhase::Cancelled.as_str(),
            ]))
            .order_by_desc(entities::task_run::Column::UpdatedAt)
            .order_by_desc(entities::task_run::Column::Id)
            .one(&tx)
            .await?
            .context("active task run not found for this session")?;
        let run = task_run_record(run_model)?;
        if run.stop_requested {
            bail!("executor allocation is not allowed after task stop was requested");
        }
        if !matches!(
            run.phase,
            TaskRunPhase::Implementing | TaskRunPhase::Reworking
        ) {
            bail!("executor allocation requires task phase implementing or reworking");
        }
        let existing = entities::work_unit::Entity::find()
            .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
            .all(&tx)
            .await?;
        let active = existing
            .iter()
            .filter(|unit| is_active_work_unit(&unit.status))
            .collect::<Vec<_>>();
        if active.len() >= MAX_ACTIVE_EXECUTORS {
            bail!("task executor concurrency limit reached: at most 4 active executors");
        }
        for unit in &active {
            let active_paths = serde_json::from_str::<Vec<String>>(&unit.owned_paths_json)?;
            if owned_paths_overlap(&input.owned_paths, &active_paths)? {
                bail!("ownedPaths overlap active work unit {}", unit.id);
            }
        }
        let owned_paths_json = serde_json::to_string(&input.owned_paths)?;
        let attempt = existing
            .iter()
            .filter(|unit| unit.owned_paths_json == owned_paths_json)
            .map(|unit| unit.attempt.max(0) as u32)
            .max()
            .unwrap_or(0)
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
                .join(&input.agent_id),
        )
        .to_string_lossy()
        .to_string();
        let branch = format!("pure-task-{}-{}", run.id, input.agent_id);
        let work_unit = work_unit_record(
            entities::work_unit::ActiveModel {
                id: Set(work_unit_id.clone()),
                task_run_id: Set(run.id.clone()),
                title: Set(input.title),
                status: Set(WorkUnitStatus::Pending.as_str().to_string()),
                owned_paths_json: Set(owned_paths_json),
                base_commit: Set(run.expected_head.clone()),
                worktree_path: Set(worktree_path),
                branch: Set(branch),
                worktree_disposition: Set(TaskWorktreeDisposition::Protect.as_str().to_string()),
                attempt: Set(attempt_i32),
                agent_id: Set(Some(input.agent_id.clone())),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await?,
        )?;
        let outcome = agent_outcome_record(
            entities::agent_outcome::ActiveModel {
                id: Set(new_id("agent-outcome")),
                task_run_id: Set(run.id.clone()),
                work_unit_id: Set(Some(work_unit_id.clone())),
                agent_id: Set(input.agent_id),
                owner_path: Set(input.owner_path),
                initiated_by: Set("planner".to_string()),
                requested_by_call_id: Set(input.requested_by_call_id),
                role: Set("executor".to_string()),
                status: Set(AgentOutcomeStatus::Queued.as_str().to_string()),
                attempt: Set(attempt_i32),
                summary: Set(None),
                error: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await?,
        )?;
        tx.commit().await?;
        Ok(ExecutorAllocation {
            run,
            work_unit,
            outcome,
        })
    }

    pub(crate) async fn activate_executor(&self, work_unit_id: &str, agent_id: &str) -> Result<()> {
        self.update_executor_allocation(
            work_unit_id,
            agent_id,
            WorkUnitStatus::Running,
            AgentOutcomeStatus::Running,
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
            AgentOutcomeStatus::Failed,
            Some(error.to_string()),
        )
        .await
    }

    async fn update_executor_allocation(
        &self,
        work_unit_id: &str,
        agent_id: &str,
        work_unit_status: WorkUnitStatus,
        outcome_status: AgentOutcomeStatus,
        error: Option<String>,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let outcome = entities::agent_outcome::Entity::find()
            .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
            .filter(entities::agent_outcome::Column::WorkUnitId.eq(Some(work_unit_id.to_string())))
            .one(&tx)
            .await?
            .context("executor outcome not found")?;
        let stored_work_unit_id = outcome
            .work_unit_id
            .clone()
            .context("executor outcome has no work unit")?;
        let work_unit = entities::work_unit::Entity::find_by_id(stored_work_unit_id)
            .one(&tx)
            .await?
            .context("executor work unit not found")?;
        let now = unix_seconds();
        let mut active_work_unit: entities::work_unit::ActiveModel = work_unit.into();
        active_work_unit.status = Set(work_unit_status.as_str().to_string());
        active_work_unit.updated_at = Set(now);
        active_work_unit.update(&tx).await?;
        let mut active_outcome: entities::agent_outcome::ActiveModel = outcome.into();
        active_outcome.status = Set(outcome_status.as_str().to_string());
        active_outcome.error = Set(error);
        active_outcome.updated_at = Set(now);
        active_outcome.update(&tx).await?;
        tx.commit().await?;
        Ok(())
    }
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
                | WorkUnitStatus::Merging
        )
    )
}
