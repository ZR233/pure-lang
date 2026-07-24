use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait,
    QueryFilter, Statement, TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentOutcomeStatus, CompletionContract, DeliveryRecoveryClaim, DeliveryRecoveryDispatch,
    WorkUnitStatus,
};

impl StudioStore {
    pub(crate) async fn fail_executor_without_delivery(
        &self,
        task_run_id: &str,
        agent_id: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let outcome = entities::agent_outcome::Entity::find()
                .filter(entities::agent_outcome::Column::TaskRunId.eq(task_run_id.to_string()))
                .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
                .filter(
                    entities::agent_outcome::Column::Status
                        .eq(AgentOutcomeStatus::WaitingForDelivery.as_str()),
                )
                .one(&tx)
                .await?
                .context("waiting executor outcome not found")?;
            if outcome.delivery_json.is_some() {
                return Ok(());
            }
            let work_unit_id = outcome
                .work_unit_id
                .clone()
                .context("waiting executor outcome has no work unit")?;
            let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id)
                .one(&tx)
                .await?
                .context("waiting executor work unit not found")?;
            let now = unix_seconds();
            let mut outcome_active: entities::agent_outcome::ActiveModel = outcome.into();
            outcome_active.status = Set(AgentOutcomeStatus::Failed.as_str().to_string());
            outcome_active.error = Set(Some(
                "executor completed without delivery, worktree changes, or a new commit"
                    .to_string(),
            ));
            outcome_active.terminal_observed = Set(1);
            outcome_active.updated_at = Set(now);
            outcome_active.update(&tx).await?;
            let mut work_unit_active: entities::work_unit::ActiveModel = work_unit.into();
            work_unit_active.status = Set(WorkUnitStatus::Failed.as_str().to_string());
            work_unit_active.updated_at = Set(now);
            work_unit_active.update(&tx).await?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => tx.commit().await.map_err(Into::into),
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn claim_delivery_recovery(
        &self,
        task_run_id: &str,
        agent_id: &str,
    ) -> Result<Option<DeliveryRecoveryClaim>> {
        let tx = self.db.begin().await?;
        let result = async {
            let Some(outcome) = entities::agent_outcome::Entity::find()
                .filter(entities::agent_outcome::Column::TaskRunId.eq(task_run_id.to_string()))
                .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
                .filter(
                    entities::agent_outcome::Column::Status
                        .eq(AgentOutcomeStatus::WaitingForDelivery.as_str()),
                )
                .one(&tx)
                .await?
            else {
                return Ok(None);
            };
            if outcome.delivery_json.is_some() {
                return Ok(None);
            }
            let contract = outcome
                .completion_contract_json
                .as_deref()
                .context("executor delivery completion contract is missing")
                .and_then(|json| {
                    serde_json::from_str::<CompletionContract>(json).map_err(Into::into)
                })?;
            let CompletionContract::DeliveryRequired {
                task_run_id: contract_task_run_id,
                work_unit_id,
                recovery_limit,
            } = contract;
            if contract_task_run_id != outcome.task_run_id
                || outcome.work_unit_id.as_deref() != Some(work_unit_id.as_str())
            {
                bail!("executor delivery completion contract identity does not match outcome");
            }
            let recovery_count = outcome.delivery_recovery_count.max(0) as u32;
            let replay_pending_dispatch = recovery_count > 0
                && outcome.terminal_observed == 0
                && recovery_count <= recovery_limit;
            if recovery_count >= recovery_limit && !replay_pending_dispatch {
                return Ok(None);
            }
            let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id.clone())
                .one(&tx)
                .await?
                .context("executor delivery recovery work unit not found")?;
            if work_unit.task_run_id != outcome.task_run_id
                || work_unit.agent_id.as_deref() != Some(outcome.agent_id.as_str())
                || WorkUnitStatus::from_str(&work_unit.status)
                    != Some(WorkUnitStatus::WaitingForDelivery)
            {
                bail!("executor delivery recovery work unit does not match waiting outcome");
            }

            let next_count = if replay_pending_dispatch {
                recovery_count
            } else {
                recovery_count.saturating_add(1)
            };
            let now = unix_seconds();
            let outcome_id = outcome.id.clone();
            if !replay_pending_dispatch {
                let mut active: entities::agent_outcome::ActiveModel = outcome.into();
                active.delivery_recovery_count = Set(next_count as i32);
                active.terminal_observed = Set(0);
                active.updated_at = Set(now);
                active.update(&tx).await?;
            }
            Ok(Some(DeliveryRecoveryClaim {
                task_run_id: task_run_id.to_string(),
                outcome_id,
                work_unit_id,
                agent_id: agent_id.to_string(),
                recovery_count: next_count,
            }))
        }
        .await;
        match result {
            Ok(claim) => {
                tx.commit().await?;
                Ok(claim)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn delivery_recovery_dispatch(
        &self,
        claim: &DeliveryRecoveryClaim,
    ) -> Result<Option<DeliveryRecoveryDispatch>> {
        let row = self
            .db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT status, reason
                 FROM agent_turns
                 WHERE agent_id = ?
                   AND json_extract(metadata_json, '$.deliveryRecoveryDispatchId') = ?
                 LIMIT 1",
                [claim.agent_id.clone().into(), claim.dispatch_id().into()],
            ))
            .await?;
        row.map(|row| {
            let status: String = row.try_get("", "status")?;
            let reason: Option<String> = row.try_get("", "reason")?;
            let dispatch = match status.as_str() {
                "queued" | "running" | "waiting_tool" | "waiting_interaction" => {
                    DeliveryRecoveryDispatch::Pending
                }
                "completed" => DeliveryRecoveryDispatch::Terminal {
                    outcome: pl_core::TurnOutcomeKind::Completed,
                    reason,
                },
                "failed" => DeliveryRecoveryDispatch::Terminal {
                    outcome: pl_core::TurnOutcomeKind::Failed,
                    reason,
                },
                "cancelled" => DeliveryRecoveryDispatch::Terminal {
                    outcome: pl_core::TurnOutcomeKind::Cancelled,
                    reason,
                },
                "budget_limited" => DeliveryRecoveryDispatch::Terminal {
                    outcome: pl_core::TurnOutcomeKind::BudgetLimited,
                    reason,
                },
                _ => bail!("invalid delivery recovery turn status: {status}"),
            };
            Ok(dispatch)
        })
        .transpose()
    }

    pub(crate) async fn fail_delivery_recovery(
        &self,
        claim: &DeliveryRecoveryClaim,
        error: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let outcome = entities::agent_outcome::Entity::find_by_id(claim.outcome_id.clone())
                .one(&tx)
                .await?
                .context("executor delivery recovery outcome not found")?;
            if outcome.task_run_id != claim.task_run_id
                || outcome.work_unit_id.as_deref() != Some(claim.work_unit_id.as_str())
                || outcome.agent_id != claim.agent_id
                || outcome.delivery_recovery_count.max(0) as u32 != claim.recovery_count
            {
                bail!("executor delivery recovery claim no longer matches durable outcome");
            }
            if outcome.delivery_json.is_some() {
                return Ok(());
            }
            let work_unit = entities::work_unit::Entity::find_by_id(claim.work_unit_id.clone())
                .one(&tx)
                .await?
                .context("executor delivery recovery work unit not found")?;
            let now = unix_seconds();
            let mut outcome_active: entities::agent_outcome::ActiveModel = outcome.into();
            outcome_active.status = Set(AgentOutcomeStatus::Failed.as_str().to_string());
            outcome_active.error = Set(Some(error.to_string()));
            outcome_active.terminal_observed = Set(1);
            outcome_active.updated_at = Set(now);
            outcome_active.update(&tx).await?;

            let mut work_unit_active: entities::work_unit::ActiveModel = work_unit.into();
            work_unit_active.status = Set(WorkUnitStatus::Failed.as_str().to_string());
            work_unit_active.updated_at = Set(now);
            work_unit_active.update(&tx).await?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => tx.commit().await.map_err(Into::into),
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }
}
