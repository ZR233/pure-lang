use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentOutcomeStatus, StudioAgentOutcomeProjection, StudioAgentTerminalChange, TaskRunPhase,
    TerminalAgentStateRecording, WorkUnitStatus,
};

use super::outcome::agent_outcome_record;

impl StudioStore {
    pub(crate) async fn cancel_executor_for_discard(
        &self,
        session_id: &str,
        work_unit_id: &str,
        agent_id: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
                .one(&tx)
                .await?
                .context("executor work unit not found")?;
            let outcome = entities::agent_outcome::Entity::find()
                .filter(
                    entities::agent_outcome::Column::WorkUnitId.eq(Some(work_unit_id.to_string())),
                )
                .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
                .one(&tx)
                .await?
                .context("executor outcome not found")?;
            let run = entities::task_run::Entity::find_by_id(work_unit.task_run_id.clone())
                .one(&tx)
                .await?
                .context("executor task run not found")?;
            if run.session_id != session_id
                || outcome.task_run_id != run.id
                || outcome.work_unit_id.as_deref() != Some(work_unit.id.as_str())
                || work_unit.agent_id.as_deref() != Some(agent_id)
                || outcome.role != "executor"
            {
                bail!("executor discard lifecycle identity does not match durable assignment");
            }

            let work_status = WorkUnitStatus::from_str(&work_unit.status)
                .with_context(|| format!("invalid work unit status: {}", work_unit.status))?;
            let outcome_status = AgentOutcomeStatus::from_str(&outcome.status)
                .with_context(|| format!("invalid agent outcome status: {}", outcome.status))?;
            if work_status == WorkUnitStatus::Merged
                && outcome_status == AgentOutcomeStatus::Completed
            {
                return Ok(());
            }
            if work_status == WorkUnitStatus::Delivered
                && outcome_status == AgentOutcomeStatus::Completed
            {
                bail!("delivered executor must be handled by task_merge_agent before close");
            }
            if matches!(
                work_status,
                WorkUnitStatus::Failed | WorkUnitStatus::Cancelled
            ) && matches!(
                outcome_status,
                AgentOutcomeStatus::Failed | AgentOutcomeStatus::Cancelled
            ) {
                return Ok(());
            }
            let active_pair = matches!(
                (work_status, outcome_status),
                (WorkUnitStatus::Pending, AgentOutcomeStatus::Queued)
                    | (WorkUnitStatus::Running, AgentOutcomeStatus::Running)
                    | (
                        WorkUnitStatus::WaitingForDelivery,
                        AgentOutcomeStatus::WaitingForDelivery
                    )
            );
            if !active_pair {
                bail!(
                    "executor discard lifecycle state mismatch: workUnit={}, outcome={}",
                    work_unit.status,
                    outcome.status
                );
            }

            let now = unix_seconds();
            let mut active_work_unit: entities::work_unit::ActiveModel = work_unit.into();
            active_work_unit.status = Set(WorkUnitStatus::Cancelled.as_str().to_string());
            active_work_unit.updated_at = Set(now);
            active_work_unit.update(&tx).await?;

            let mut active_outcome: entities::agent_outcome::ActiveModel = outcome.into();
            active_outcome.status = Set(AgentOutcomeStatus::Cancelled.as_str().to_string());
            active_outcome.error = Set(Some("executor discarded by planner".to_string()));
            active_outcome.terminal_observed = Set(1);
            active_outcome.updated_at = Set(now);
            active_outcome.update(&tx).await?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => {
                tx.commit().await?;
                Ok(())
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn record_terminal_agent_state(
        &self,
        session_id: &str,
        change: &StudioAgentTerminalChange,
    ) -> Result<TerminalAgentStateRecording> {
        let target = terminal_target(&change.role, change.outcome);
        let tx = self.db.begin().await?;
        let result = async {
            let task_run_ids = entities::task_run::Entity::find()
                .filter(entities::task_run::Column::SessionId.eq(session_id.to_string()))
                .all(&tx)
                .await?
                .into_iter()
                .map(|run| run.id)
                .collect::<Vec<_>>();
            if task_run_ids.is_empty() {
                return Ok(TerminalAgentStateRecording::Unhandled);
            }
            let Some(outcome) = entities::agent_outcome::Entity::find()
                .filter(entities::agent_outcome::Column::AgentId.eq(change.agent_id.clone()))
                .filter(entities::agent_outcome::Column::TaskRunId.is_in(task_run_ids))
                .order_by_desc(entities::agent_outcome::Column::UpdatedAt)
                .order_by_desc(entities::agent_outcome::Column::Id)
                .one(&tx)
                .await?
            else {
                return Ok(TerminalAgentStateRecording::Unhandled);
            };
            let run = entities::task_run::Entity::find_by_id(outcome.task_run_id.clone())
                .one(&tx)
                .await?
                .context("terminal agent task run not found")?;
            let phase = TaskRunPhase::from_str(&run.phase)
                .with_context(|| format!("invalid stored task phase: {}", run.phase))?;
            if outcome.role != change.role {
                bail!("terminal agent role does not match durable outcome");
            }
            if phase == TaskRunPhase::Blocked
                && run.status_message.as_deref().is_some_and(|message| {
                    message.starts_with("terminal agent state persistence failed:")
                })
            {
                return Ok(TerminalAgentStateRecording::Suppressed);
            }
            if matches!(
                phase,
                TaskRunPhase::Completed
                    | TaskRunPhase::Blocked
                    | TaskRunPhase::Failed
                    | TaskRunPhase::Cancelled
            ) {
                return Ok(TerminalAgentStateRecording::Projected(
                    projection_from_outcome(outcome)?,
                ));
            }
            let current = AgentOutcomeStatus::from_str(&outcome.status)
                .with_context(|| format!("invalid agent outcome status: {}", outcome.status))?;
            if outcome.terminal_observed != 0 {
                return Ok(TerminalAgentStateRecording::Projected(
                    projection_from_outcome(outcome)?,
                ));
            }

            let now = unix_seconds();
            let mut active_outcome: entities::agent_outcome::ActiveModel = outcome.clone().into();
            if current != target.outcome
                && !is_durable_terminal(current)
                && outcome.delivery_json.is_none()
            {
                active_outcome.status = Set(target.outcome.as_str().to_string());
                active_outcome.summary = Set(change.summary.clone());
                active_outcome.error = Set(change.error.clone().or_else(|| target.default_error()));
            }
            active_outcome.terminal_observed = Set(1);
            active_outcome.updated_at = Set(now);
            let mut outcome = active_outcome.update(&tx).await?;

            if let Some(work_unit_id) = outcome.work_unit_id.as_deref() {
                let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
                    .one(&tx)
                    .await?
                    .context("agent outcome work unit not found")?;
                if work_unit.task_run_id != outcome.task_run_id
                    || work_unit.agent_id.as_deref() != Some(outcome.agent_id.as_str())
                {
                    bail!("terminal outcome and work unit do not match");
                }
                if !matches!(
                    WorkUnitStatus::from_str(&work_unit.status),
                    Some(WorkUnitStatus::Delivered | WorkUnitStatus::Merged)
                ) {
                    let mut active_work_unit: entities::work_unit::ActiveModel = work_unit.into();
                    active_work_unit.status = Set(target.work_unit.as_str().to_string());
                    active_work_unit.updated_at = Set(now);
                    active_work_unit.update(&tx).await?;
                }
            }
            if outcome.role == "reviewer" {
                let rounds = entities::review_round::Entity::find()
                    .filter(
                        entities::review_round::Column::TaskRunId.eq(outcome.task_run_id.clone()),
                    )
                    .filter(
                        entities::review_round::Column::ReviewerAgentId
                            .eq(Some(outcome.agent_id.clone())),
                    )
                    .filter(
                        entities::review_round::Column::Status
                            .eq(crate::studio::task_coordinator::ReviewVerdict::Pending.as_str()),
                    )
                    .all(&tx)
                    .await?;
                let pending = match rounds.as_slice() {
                    [] => None,
                    [round] => Some(round.clone()),
                    _ => bail!("reviewer has multiple pending review rounds"),
                };
                if let Some(round) = pending {
                    let reason = change.error.clone().unwrap_or_else(|| {
                        "reviewer terminated without a successful review_exit".to_string()
                    });
                    let mut outcome_active: entities::agent_outcome::ActiveModel = outcome.into();
                    outcome_active.status = Set(AgentOutcomeStatus::Failed.as_str().to_string());
                    outcome_active.error = Set(Some(reason.clone()));
                    outcome_active.updated_at = Set(now);
                    outcome = outcome_active.update(&tx).await?;
                    let mut round_active: entities::review_round::ActiveModel =
                        round.clone().into();
                    round_active.status =
                        Set(crate::studio::task_coordinator::ReviewVerdict::Failed
                            .as_str()
                            .to_string());
                    round_active.summary = Set(Some(reason.clone()));
                    round_active.updated_at = Set(now);
                    round_active.update(&tx).await?;
                    let mut run_active: entities::task_run::ActiveModel = run.into();
                    run_active.phase = Set(if round.round == 1 {
                        TaskRunPhase::Implementing.as_str().to_string()
                    } else {
                        TaskRunPhase::Reworking.as_str().to_string()
                    });
                    run_active.status_message = Set(Some(reason));
                    run_active.updated_at = Set(now);
                    run_active.update(&tx).await?;
                }
            }
            Ok(TerminalAgentStateRecording::Changed {
                task_run_id: outcome.task_run_id.clone(),
                projection: projection_from_outcome(outcome)?,
            })
        }
        .await;
        match result {
            Ok(projection) => {
                tx.commit().await?;
                Ok(projection)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }
}

struct TerminalTarget {
    outcome: AgentOutcomeStatus,
    work_unit: WorkUnitStatus,
}

impl TerminalTarget {
    fn default_error(&self) -> Option<String> {
        (self.outcome == AgentOutcomeStatus::WaitingForDelivery)
            .then(|| "executor completed without a successful delivery".to_string())
    }
}

fn terminal_target(role: &str, outcome: pl_core::TurnOutcomeKind) -> TerminalTarget {
    match outcome {
        pl_core::TurnOutcomeKind::Completed if role == "executor" => TerminalTarget {
            outcome: AgentOutcomeStatus::WaitingForDelivery,
            work_unit: WorkUnitStatus::WaitingForDelivery,
        },
        pl_core::TurnOutcomeKind::Completed => TerminalTarget {
            outcome: AgentOutcomeStatus::Completed,
            work_unit: WorkUnitStatus::Delivered,
        },
        pl_core::TurnOutcomeKind::Failed | pl_core::TurnOutcomeKind::BudgetLimited => {
            TerminalTarget {
                outcome: AgentOutcomeStatus::Failed,
                work_unit: WorkUnitStatus::Failed,
            }
        }
        pl_core::TurnOutcomeKind::Cancelled => TerminalTarget {
            outcome: AgentOutcomeStatus::Cancelled,
            work_unit: WorkUnitStatus::Cancelled,
        },
    }
}

fn is_durable_terminal(status: AgentOutcomeStatus) -> bool {
    matches!(
        status,
        AgentOutcomeStatus::Completed | AgentOutcomeStatus::Failed | AgentOutcomeStatus::Cancelled
    )
}

fn projection_from_outcome(
    outcome: entities::agent_outcome::Model,
) -> Result<StudioAgentOutcomeProjection> {
    let outcome = agent_outcome_record(outcome)?;
    Ok(StudioAgentOutcomeProjection {
        status: outcome.status,
        summary: outcome.summary,
        error: outcome.error,
    })
}
