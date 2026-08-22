use anyhow::{Context, Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::entity as entities;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    ExecutorContinuationStateKind, ReviewRoundStateKind, ReviewScope, TaskPlannerWakeRequest,
    TaskPlannerWakeSource,
};

use super::review::review_round_state;
use super::work_unit::work_unit_record;

impl StudioStore {
    pub(crate) async fn list_pending_task_planner_wakes(
        &self,
    ) -> Result<Vec<TaskPlannerWakeRequest>> {
        let mut wakes = self.pending_executor_terminal_wakes().await?;
        for run in self.list_active_task_runs().await? {
            let latest_review = entities::review_round::Entity::find()
                .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
                .filter(entities::review_round::Column::StateKind.is_not_in([
                    ReviewRoundStateKind::PendingDispatch.as_str(),
                    ReviewRoundStateKind::Dispatched.as_str(),
                    ReviewRoundStateKind::Running.as_str(),
                ]))
                .order_by_desc(entities::review_round::Column::Round)
                .one(&self.db)
                .await?;
            let Some(round) = latest_review else {
                continue;
            };
            if review_round_state(&round)?.kind().is_active() {
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
        Ok(matches!(
            input.state_kind.as_str(),
            "pending" | "claimed" | "consumed"
        ))
    }

    async fn pending_executor_terminal_wakes(&self) -> Result<Vec<TaskPlannerWakeRequest>> {
        let units = entities::work_unit::Entity::find()
            .order_by_asc(entities::work_unit::Column::UpdatedAt)
            .order_by_asc(entities::work_unit::Column::Id)
            .all(&self.db)
            .await?;
        let mut wakes = Vec::with_capacity(units.len());
        for unit in units {
            let record = work_unit_record(unit.clone())?;
            let continuation_state = record.continuation_state();
            if !matches!(
                continuation_state,
                ExecutorContinuationStateKind::PlannerWakePending
                    | ExecutorContinuationStateKind::NeedsAttention
            ) {
                continue;
            }
            if continuation_state == ExecutorContinuationStateKind::NeedsAttention
                && record.budget_limit().is_none()
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
                    source_turn_id: record
                        .continuation_source_turn_id()
                        .map(str::to_string)
                        .context("executor terminal wake has no source Turn")?,
                },
            });
        }
        Ok(wakes)
    }
}
