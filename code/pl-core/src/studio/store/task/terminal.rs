use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::agent::{AgentLifecycleProjection, AgentTerminalStateChange};
use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{AgentOutcomeStatus, TaskRunPhase, WorkUnitStatus};

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

    pub(crate) async fn record_terminal_agent_state(
        &self,
        session_id: &str,
        change: &AgentTerminalStateChange,
    ) -> Result<Option<AgentLifecycleProjection>> {
        let Some(target) = terminal_target(&change.role, change.status) else {
            return Ok(None);
        };
        let tx = self.db.begin().await?;
        let result = async {
            let Some(run) = entities::task_run::Entity::find()
                .filter(entities::task_run::Column::SessionId.eq(session_id.to_string()))
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
            else {
                return Ok(None);
            };
            let Some(outcome) = entities::agent_outcome::Entity::find()
                .filter(entities::agent_outcome::Column::TaskRunId.eq(run.id.clone()))
                .filter(entities::agent_outcome::Column::AgentId.eq(change.agent_id.clone()))
                .order_by_desc(entities::agent_outcome::Column::UpdatedAt)
                .order_by_desc(entities::agent_outcome::Column::Id)
                .one(&tx)
                .await?
            else {
                return Ok(None);
            };
            if outcome.role != change.role {
                bail!("terminal agent role does not match durable outcome");
            }
            let current = AgentOutcomeStatus::from_str(&outcome.status)
                .with_context(|| format!("invalid agent outcome status: {}", outcome.status))?;
            if current == target.outcome
                || is_durable_terminal(current)
                || outcome.delivery_json.is_some()
            {
                return Ok(Some(projection_from_outcome(outcome)?));
            }

            let now = unix_seconds();
            let mut active_outcome: entities::agent_outcome::ActiveModel = outcome.clone().into();
            active_outcome.status = Set(target.outcome.as_str().to_string());
            active_outcome.summary = Set(change.summary.clone());
            active_outcome.error = Set(change.error.clone().or_else(|| target.default_error()));
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
                if work_unit.status != WorkUnitStatus::Delivered.as_str() {
                    let mut active_work_unit: entities::work_unit::ActiveModel = work_unit.into();
                    active_work_unit.status = Set(target.work_unit.as_str().to_string());
                    active_work_unit.updated_at = Set(now);
                    active_work_unit.update(&tx).await?;
                }
            }
            Ok(Some(projection_from_outcome(outcome)?))
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
