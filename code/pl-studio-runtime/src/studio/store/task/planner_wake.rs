use anyhow::{Context, Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::entity as entities;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    ExecutorContinuationState, ReviewScope, ReviewVerdict, TaskPlannerWakeRequest,
    TaskPlannerWakeSource, ThreadExecutionStatus,
};

use super::review::review_round_state;
use super::work_unit::work_unit_state;

impl StudioStore {
    pub(crate) async fn list_pending_task_planner_wakes(
        &self,
    ) -> Result<Vec<TaskPlannerWakeRequest>> {
        let mut wakes = self.pending_executor_terminal_wakes().await?;
        for run in self.list_active_task_runs().await? {
            let latest_review = entities::review_round::Entity::find()
                .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
                .filter(
                    entities::review_round::Column::StateKind.ne(ReviewVerdict::Pending.as_str()),
                )
                .order_by_desc(entities::review_round::Column::Round)
                .one(&self.db)
                .await?;
            let Some(round) = latest_review else {
                continue;
            };
            let reviewer_status = review_round_state(&round)?.reviewer_status();
            if matches!(
                reviewer_status,
                ThreadExecutionStatus::Queued | ThreadExecutionStatus::Running
            ) {
                continue;
            }
            wakes.push(TaskPlannerWakeRequest {
                task_run_id: run.id.clone(),
                root_thread_id: run.root_thread_id.clone(),
                source: TaskPlannerWakeSource::Review {
                    review_round_id: round.id,
                    scope: ReviewScope::from_str(&round.scope)
                        .context("invalid stored review scope")?,
                },
            });
        }
        let mut pending = Vec::with_capacity(wakes.len());
        for wake in wakes {
            if !self.task_planner_wake_was_delivered(&wake).await? {
                pending.push(wake);
            }
        }
        Ok(pending)
    }

    pub(crate) async fn task_planner_wake_was_delivered(
        &self,
        wake: &TaskPlannerWakeRequest,
    ) -> Result<bool> {
        let Some(input) = entities::thread_input::Entity::find_by_id(wake.mail_id())
            .one(&self.db)
            .await?
        else {
            return Ok(false);
        };
        if input.thread_id != wake.root_thread_id {
            bail!("Task Planner wake mail belongs to another Thread");
        }
        match input.state.as_str() {
            "queued" | "claimed" | "active" | "consumed" => Ok(true),
            state => bail!("Task Planner wake mail has unknown state {state}"),
        }
    }

    async fn pending_executor_terminal_wakes(&self) -> Result<Vec<TaskPlannerWakeRequest>> {
        let units = entities::work_unit::Entity::find()
            .order_by_asc(entities::work_unit::Column::UpdatedAt)
            .order_by_asc(entities::work_unit::Column::Id)
            .all(&self.db)
            .await?;
        let mut wakes = Vec::with_capacity(units.len());
        for unit in units {
            let state = work_unit_state(&unit)?;
            let progress = state.progress();
            let continuation_state = progress.continuation_state;
            if !matches!(
                continuation_state,
                ExecutorContinuationState::PlannerWakePending
                    | ExecutorContinuationState::NeedsAttention
            ) {
                continue;
            }
            if continuation_state == ExecutorContinuationState::NeedsAttention
                && (state.execution_status() != ThreadExecutionStatus::BudgetLimited
                    || progress.budget_limit.is_none())
            {
                continue;
            }
            let Some(run) = self.read_task_run(&unit.task_run_id).await? else {
                bail!("executor terminal wake task run not found");
            };
            if run.kind().is_terminal() {
                continue;
            }
            wakes.push(TaskPlannerWakeRequest {
                task_run_id: run.id.clone(),
                root_thread_id: run.root_thread_id.clone(),
                source: TaskPlannerWakeSource::ExecutorTerminal {
                    work_unit_id: unit.id,
                    executor_thread_id: unit
                        .executor_thread_id
                        .context("executor terminal wake has no executor Thread")?,
                    source_turn_id: progress
                        .continuation_source_turn_id
                        .clone()
                        .context("executor terminal wake has no source Turn")?,
                },
            });
        }
        Ok(wakes)
    }
}
