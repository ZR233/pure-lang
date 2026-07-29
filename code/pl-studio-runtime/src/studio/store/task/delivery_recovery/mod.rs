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
    DeliveryRecoveryFailureRecording, TaskRunPhase, WorkUnitStatus,
};

impl StudioStore {
    pub(crate) async fn fail_executor_without_delivery(
        &self,
        task_run_id: &str,
        agent_id: &str,
        expected_task_generation: u64,
    ) -> Result<DeliveryRecoveryFailureRecording> {
        let Some(outcome) = entities::agent_outcome::Entity::find()
            .filter(entities::agent_outcome::Column::TaskRunId.eq(task_run_id.to_string()))
            .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
            .filter(
                entities::agent_outcome::Column::Status
                    .eq(AgentOutcomeStatus::WaitingForDelivery.as_str()),
            )
            .one(&self.db)
            .await?
        else {
            return Ok(DeliveryRecoveryFailureRecording::Suppressed);
        };
        if outcome.role != "executor" {
            bail!("delivery recovery failure can only update an executor outcome");
        }
        if outcome.delivery_json.is_some() {
            return Ok(DeliveryRecoveryFailureRecording::Suppressed);
        }
        let work_unit_id = outcome
            .work_unit_id
            .clone()
            .context("waiting executor outcome has no work unit")?;
        let tx = self.db.begin().await?;
        let result = async {
            let now = unix_seconds();
            let updated = tx
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "UPDATE agent_outcomes
                     SET status = ?, error = ?, terminal_observed = 1, updated_at = ?
                     WHERE id = ?
                       AND task_run_id = ?
                       AND agent_id = ?
                       AND role = 'executor'
                       AND status = ?
                       AND delivery_json IS NULL
                       AND EXISTS (
                           SELECT 1
                           FROM task_runs
                           WHERE task_runs.id = agent_outcomes.task_run_id
                             AND task_runs.task_generation = ?
                             AND task_runs.terminal_generation IS NULL
                             AND task_runs.phase NOT IN (?, ?, ?, ?)
                       )",
                    [
                        AgentOutcomeStatus::Failed.as_str().into(),
                        "executor completed without delivery, worktree changes, or a new commit"
                            .into(),
                        now.into(),
                        outcome.id.clone().into(),
                        task_run_id.into(),
                        agent_id.into(),
                        AgentOutcomeStatus::WaitingForDelivery.as_str().into(),
                        i64::try_from(expected_task_generation)?.into(),
                        TaskRunPhase::Completed.as_str().into(),
                        TaskRunPhase::Blocked.as_str().into(),
                        TaskRunPhase::Failed.as_str().into(),
                        TaskRunPhase::Cancelled.as_str().into(),
                    ],
                ))
                .await?;
            if updated.rows_affected() == 0 {
                return Ok(DeliveryRecoveryFailureRecording::Suppressed);
            }
            let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id)
                .one(&tx)
                .await?
                .context("waiting executor work unit not found")?;
            if work_unit.task_run_id != task_run_id
                || work_unit.agent_id.as_deref() != Some(agent_id)
                || WorkUnitStatus::from_str(&work_unit.status)
                    != Some(WorkUnitStatus::WaitingForDelivery)
            {
                bail!("executor delivery recovery work unit no longer matches waiting outcome");
            }
            let mut work_unit_active: entities::work_unit::ActiveModel = work_unit.into();
            work_unit_active.status = Set(WorkUnitStatus::Failed.as_str().to_string());
            work_unit_active.updated_at = Set(now);
            work_unit_active.update(&tx).await?;
            Ok(DeliveryRecoveryFailureRecording::Recorded)
        }
        .await;
        match result {
            Ok(recording) => {
                tx.commit().await?;
                Ok(recording)
            }
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
            let run = entities::task_run::Entity::find_by_id(outcome.task_run_id.clone())
                .one(&tx)
                .await?
                .context("executor delivery recovery task run not found")?;
            let phase = TaskRunPhase::from_str(&run.phase)
                .with_context(|| format!("invalid task phase: {}", run.phase))?;
            if phase.is_terminal() {
                return Ok(None);
            }
            if run.terminal_generation.is_some() {
                bail!("active delivery recovery task already has a terminal generation");
            }
            let task_generation = u64::try_from(run.task_generation)
                .context("task generation must not be negative")?;
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
            let recovery_limit = if run.stop_requested != 0 {
                recovery_limit.min(1)
            } else {
                recovery_limit
            };
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
                task_generation,
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
                   AND json_extract(metadata_json, '$.taskGeneration') = ?
                 LIMIT 1",
                [
                    claim.agent_id.clone().into(),
                    claim.dispatch_id().into(),
                    i64::try_from(claim.task_generation)?.into(),
                ],
            ))
            .await?;
        row.map(|row| {
            let status: String = row.try_get("", "status")?;
            let reason: Option<String> = row.try_get("", "reason")?;
            let dispatch = match status.as_str() {
                "queued"
                | "running"
                | "waiting_tool"
                | "waiting_interaction"
                | "waiting_agents" => DeliveryRecoveryDispatch::Pending,
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
            let run = entities::task_run::Entity::find_by_id(claim.task_run_id.clone())
                .one(&tx)
                .await?
                .context("executor delivery recovery task run not found")?;
            if u64::try_from(run.task_generation)? != claim.task_generation
                || run.terminal_generation.is_some()
                || TaskRunPhase::from_str(&run.phase).is_none_or(TaskRunPhase::is_terminal)
            {
                bail!("executor delivery recovery claim belongs to a stale task generation");
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

    pub(crate) async fn validate_delivery_recovery_claim(
        &self,
        claim: &DeliveryRecoveryClaim,
    ) -> Result<bool> {
        let Some(run) = entities::task_run::Entity::find_by_id(claim.task_run_id.clone())
            .one(&self.db)
            .await?
        else {
            return Ok(false);
        };
        if u64::try_from(run.task_generation)? != claim.task_generation
            || run.terminal_generation.is_some()
            || TaskRunPhase::from_str(&run.phase).is_none_or(TaskRunPhase::is_terminal)
        {
            return Ok(false);
        }
        let Some(outcome) = entities::agent_outcome::Entity::find_by_id(claim.outcome_id.clone())
            .one(&self.db)
            .await?
        else {
            return Ok(false);
        };
        Ok(outcome.task_run_id == claim.task_run_id
            && outcome.work_unit_id.as_deref() == Some(claim.work_unit_id.as_str())
            && outcome.agent_id == claim.agent_id
            && outcome.status == AgentOutcomeStatus::WaitingForDelivery.as_str()
            && outcome.delivery_json.is_none()
            && outcome.delivery_recovery_count.max(0) as u32 == claim.recovery_count)
    }
}
