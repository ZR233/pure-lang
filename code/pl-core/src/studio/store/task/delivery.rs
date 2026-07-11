use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentDelivery, AgentOutcomeStatus, DeliveryScope, DeliveryScopeResolution, TaskRunPhase,
    WorkUnitStatus,
};

use super::outcome::agent_outcome_record;
use super::task_run_record;
use super::work_unit::work_unit_record;

impl StudioStore {
    pub(crate) async fn resolve_active_delivery_scope(
        &self,
        agent_id: &str,
        worktree_path: &str,
        branch: &str,
    ) -> Result<Option<DeliveryScopeResolution>> {
        let work_units = entities::work_unit::Entity::find()
            .filter(entities::work_unit::Column::AgentId.eq(agent_id.to_string()))
            .all(&self.db)
            .await?;
        let mut matching_scopes = Vec::new();
        let mut fallback_scopes = Vec::new();
        for work_unit in work_units {
            let Some(outcome) = entities::agent_outcome::Entity::find()
                .filter(entities::agent_outcome::Column::WorkUnitId.eq(work_unit.id.clone()))
                .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
                .one(&self.db)
                .await?
            else {
                continue;
            };
            let Some(run) = entities::task_run::Entity::find_by_id(work_unit.task_run_id.clone())
                .filter(entities::task_run::Column::Phase.is_not_in([
                    TaskRunPhase::Completed.as_str(),
                    TaskRunPhase::Blocked.as_str(),
                    TaskRunPhase::Failed.as_str(),
                    TaskRunPhase::Cancelled.as_str(),
                ]))
                .one(&self.db)
                .await?
            else {
                continue;
            };
            let matches_caller =
                work_unit.worktree_path == worktree_path && work_unit.branch == branch;
            let scope = DeliveryScope {
                run: task_run_record(run)?,
                work_unit: work_unit_record(work_unit)?,
                outcome: agent_outcome_record(outcome)?,
            };
            if matches_caller {
                matching_scopes.push(scope);
            } else {
                fallback_scopes.push(scope);
            }
        }
        match matching_scopes.len() {
            0 => {}
            1 => {
                return Ok(matching_scopes.pop().map(DeliveryScopeResolution::Resolved));
            }
            _ => bail!("ambiguous active delivery scope for executor worktree"),
        }
        match fallback_scopes.len() {
            0 => {}
            1 => {
                return Ok(fallback_scopes.pop().map(DeliveryScopeResolution::Resolved));
            }
            _ => bail!("ambiguous active delivery scope for executor worktree"),
        }

        let outcomes = entities::agent_outcome::Entity::find()
            .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
            .all(&self.db)
            .await?;
        let mut missing_work_units = Vec::new();
        for outcome in outcomes {
            let has_active_run =
                entities::task_run::Entity::find_by_id(outcome.task_run_id.clone())
                    .filter(entities::task_run::Column::Phase.is_not_in([
                        TaskRunPhase::Completed.as_str(),
                        TaskRunPhase::Blocked.as_str(),
                        TaskRunPhase::Failed.as_str(),
                        TaskRunPhase::Cancelled.as_str(),
                    ]))
                    .one(&self.db)
                    .await?
                    .is_some();
            if !has_active_run {
                continue;
            }
            let has_work_unit = match outcome.work_unit_id.as_deref() {
                Some(work_unit_id) => entities::work_unit::Entity::find_by_id(work_unit_id)
                    .one(&self.db)
                    .await?
                    .is_some(),
                None => false,
            };
            if !has_work_unit {
                missing_work_units.push(agent_outcome_record(outcome)?);
            }
        }
        match missing_work_units.len() {
            0 => Ok(None),
            1 => Ok(missing_work_units
                .pop()
                .map(DeliveryScopeResolution::MissingWorkUnit)),
            _ => bail!("ambiguous active delivery scope for executor worktree"),
        }
    }

    pub(crate) async fn complete_agent_delivery(
        &self,
        outcome_id: &str,
        work_unit_id: &str,
        delivery: AgentDelivery,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let (outcome, work_unit) = load_delivery_pair(&tx, outcome_id, work_unit_id).await?;
            ensure_delivery_is_open(&outcome, Some(&work_unit))?;
            let now = unix_seconds();

            let mut outcome: entities::agent_outcome::ActiveModel = outcome.into();
            outcome.status = Set(AgentOutcomeStatus::Completed.as_str().to_string());
            outcome.summary = Set(Some(delivery.verification_summary.clone()));
            outcome.error = Set(None);
            outcome.delivery_json = Set(Some(serde_json::to_string(&delivery)?));
            outcome.terminal_observed = Set(0);
            outcome.updated_at = Set(now);
            outcome.update(&tx).await?;

            let mut work_unit: entities::work_unit::ActiveModel = work_unit.into();
            work_unit.status = Set(WorkUnitStatus::Delivered.as_str().to_string());
            work_unit.updated_at = Set(now);
            work_unit.update(&tx).await?;
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn mark_agent_delivery_waiting(
        &self,
        outcome_id: &str,
        work_unit_id: Option<&str>,
        error: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let outcome = entities::agent_outcome::Entity::find_by_id(outcome_id.to_string())
                .one(&tx)
                .await?
                .context("agent outcome not found")?;
            let work_unit = match work_unit_id {
                Some(work_unit_id) => Some(
                    entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
                        .one(&tx)
                        .await?
                        .context("work unit not found")?,
                ),
                None => None,
            };
            validate_delivery_link(&outcome, work_unit.as_ref())?;
            ensure_delivery_is_open(&outcome, work_unit.as_ref())?;
            let now = unix_seconds();

            let mut outcome: entities::agent_outcome::ActiveModel = outcome.into();
            outcome.status = Set(AgentOutcomeStatus::WaitingForDelivery.as_str().to_string());
            outcome.error = Set(Some(error.to_string()));
            outcome.updated_at = Set(now);
            outcome.update(&tx).await?;

            if let Some(work_unit) = work_unit {
                let mut work_unit: entities::work_unit::ActiveModel = work_unit.into();
                work_unit.status = Set(WorkUnitStatus::WaitingForDelivery.as_str().to_string());
                work_unit.updated_at = Set(now);
                work_unit.update(&tx).await?;
            }
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }
}

async fn load_delivery_pair(
    tx: &sea_orm::DatabaseTransaction,
    outcome_id: &str,
    work_unit_id: &str,
) -> Result<(entities::agent_outcome::Model, entities::work_unit::Model)> {
    let outcome = entities::agent_outcome::Entity::find_by_id(outcome_id.to_string())
        .one(tx)
        .await?
        .context("agent outcome not found")?;
    let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id.to_string())
        .one(tx)
        .await?
        .context("work unit not found")?;
    validate_delivery_link(&outcome, Some(&work_unit))?;
    Ok((outcome, work_unit))
}

fn validate_delivery_link(
    outcome: &entities::agent_outcome::Model,
    work_unit: Option<&entities::work_unit::Model>,
) -> Result<()> {
    if let Some(work_unit) = work_unit
        && (outcome.work_unit_id.as_deref() != Some(work_unit.id.as_str())
            || outcome.task_run_id != work_unit.task_run_id
            || work_unit.agent_id.as_deref() != Some(outcome.agent_id.as_str()))
    {
        bail!("agent outcome and work unit do not describe the same delivery");
    }
    Ok(())
}

fn ensure_delivery_is_open(
    outcome: &entities::agent_outcome::Model,
    work_unit: Option<&entities::work_unit::Model>,
) -> Result<()> {
    if outcome.status == AgentOutcomeStatus::Completed.as_str()
        || work_unit.is_some_and(|unit| unit.status == WorkUnitStatus::Delivered.as_str())
    {
        bail!("delivery is already finalized");
    }
    if !matches!(
        AgentOutcomeStatus::from_str(&outcome.status),
        Some(AgentOutcomeStatus::Running | AgentOutcomeStatus::WaitingForDelivery)
    ) {
        bail!("agent outcome is not accepting a delivery");
    }
    if let Some(work_unit) = work_unit
        && !matches!(
            WorkUnitStatus::from_str(&work_unit.status),
            Some(WorkUnitStatus::Running | WorkUnitStatus::WaitingForDelivery)
        )
    {
        bail!("work unit is not accepting a delivery");
    }
    Ok(())
}

async fn finish_transaction(tx: sea_orm::DatabaseTransaction, result: Result<()>) -> Result<()> {
    match result {
        Ok(()) => tx.commit().await.map_err(Into::into),
        Err(error) => {
            tx.rollback().await?;
            Err(error)
        }
    }
}
