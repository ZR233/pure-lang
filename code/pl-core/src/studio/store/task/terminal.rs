use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::agent::{AgentLifecycleProjection, AgentTerminalStateChange};
use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentOutcomeStatus, TaskRunPhase, TerminalAgentStateRecording, WorkUnitStatus,
};

use super::outcome::agent_outcome_record;

impl StudioStore {
    pub(crate) async fn project_agent_lifecycle(
        &self,
        lifecycle_token: &str,
        role: &str,
    ) -> Result<Option<AgentLifecycleProjection>> {
        let query = entities::agent_outcome::Entity::find();
        let outcome = match role {
            "executor" => {
                query
                    .filter(
                        entities::agent_outcome::Column::WorkUnitId
                            .eq(Some(lifecycle_token.to_string())),
                    )
                    .one(&self.db)
                    .await?
            }
            "explorer" => {
                entities::agent_outcome::Entity::find_by_id(lifecycle_token.to_string())
                    .one(&self.db)
                    .await?
            }
            _ => None,
        };
        outcome.map(projection_from_outcome).transpose()
    }

    pub(crate) async fn project_agent_activity(
        &self,
        session_id: &str,
        agent_id: &str,
    ) -> Result<Option<AgentLifecycleProjection>> {
        let task_run_ids = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::SessionId.eq(session_id.to_string()))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|run| run.id)
            .collect::<Vec<_>>();
        if task_run_ids.is_empty() {
            return Ok(None);
        }
        let Some(outcome) = entities::agent_outcome::Entity::find()
            .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
            .filter(entities::agent_outcome::Column::TaskRunId.is_in(task_run_ids))
            .order_by_desc(entities::agent_outcome::Column::UpdatedAt)
            .order_by_desc(entities::agent_outcome::Column::Id)
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };
        projection_from_outcome(outcome).map(Some)
    }

    pub(crate) async fn record_terminal_agent_state(
        &self,
        session_id: &str,
        change: &AgentTerminalStateChange,
    ) -> Result<TerminalAgentStateRecording> {
        let Some(target) = terminal_target(&change.role, change.status) else {
            return Ok(TerminalAgentStateRecording::Unhandled);
        };
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
            let outcome = active_outcome.update(&tx).await?;

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

fn terminal_target(role: &str, status: pl_protocol::AgentStatus) -> Option<TerminalTarget> {
    match status {
        pl_protocol::AgentStatus::Completed if role == "executor" => Some(TerminalTarget {
            outcome: AgentOutcomeStatus::WaitingForDelivery,
            work_unit: WorkUnitStatus::WaitingForDelivery,
        }),
        pl_protocol::AgentStatus::Completed => Some(TerminalTarget {
            outcome: AgentOutcomeStatus::Completed,
            work_unit: WorkUnitStatus::Delivered,
        }),
        pl_protocol::AgentStatus::Errored => Some(TerminalTarget {
            outcome: AgentOutcomeStatus::Failed,
            work_unit: WorkUnitStatus::Failed,
        }),
        pl_protocol::AgentStatus::Interrupted | pl_protocol::AgentStatus::Shutdown => {
            Some(TerminalTarget {
                outcome: AgentOutcomeStatus::Cancelled,
                work_unit: WorkUnitStatus::Cancelled,
            })
        }
        pl_protocol::AgentStatus::Queued
        | pl_protocol::AgentStatus::Running
        | pl_protocol::AgentStatus::Waiting
        | pl_protocol::AgentStatus::NotFound => None,
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
) -> Result<AgentLifecycleProjection> {
    let outcome = agent_outcome_record(outcome)?;
    let status = match outcome.status {
        AgentOutcomeStatus::Queued => pl_protocol::AgentStatus::Queued,
        AgentOutcomeStatus::Running => pl_protocol::AgentStatus::Running,
        AgentOutcomeStatus::WaitingForDelivery => pl_protocol::AgentStatus::Waiting,
        AgentOutcomeStatus::Completed => pl_protocol::AgentStatus::Completed,
        AgentOutcomeStatus::Failed => pl_protocol::AgentStatus::Errored,
        AgentOutcomeStatus::Cancelled => pl_protocol::AgentStatus::Interrupted,
    };
    Ok(AgentLifecycleProjection::new(
        status,
        outcome.summary,
        outcome.error,
    ))
}
