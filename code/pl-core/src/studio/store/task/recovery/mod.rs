use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentOutcomeStatus, RestartAgentReconciliation, TaskWorktreeCreationState,
    TaskWorktreeOwnerResource, TaskWorktreeOwnerSnapshot, WorkUnitStatus,
};

use super::{outcome::agent_outcome_record, task_run_record, work_unit::work_unit_record};

const RESTART_DIAGNOSTIC: &str = "agent interrupted by application restart";
const RESTART_BEFORE_CREATE_DIAGNOSTIC: &str =
    "agent interrupted by application restart before worktree creation";

impl StudioStore {
    pub(crate) async fn list_all_task_worktree_owners(
        &self,
    ) -> Result<Vec<TaskWorktreeOwnerSnapshot>> {
        let mut common_dirs = entities::task_run::Entity::find()
            .all(&self.db)
            .await?
            .into_iter()
            .map(|run| run.git_common_dir)
            .collect::<Vec<_>>();
        common_dirs.sort();
        common_dirs.dedup();
        let mut owners = Vec::new();
        for common_dir in common_dirs {
            owners.extend(
                self.list_task_worktree_owners_by_git_common_dir(&common_dir)
                    .await?,
            );
        }
        Ok(owners)
    }

    pub(crate) async fn list_task_worktree_owners_by_git_common_dir(
        &self,
        git_common_dir: &str,
    ) -> Result<Vec<TaskWorktreeOwnerSnapshot>> {
        let runs = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::GitCommonDir.eq(git_common_dir.to_string()))
            .all(&self.db)
            .await?;
        self.task_worktree_owners_for_runs(runs).await
    }

    async fn task_worktree_owners_for_runs(
        &self,
        runs: Vec<entities::task_run::Model>,
    ) -> Result<Vec<TaskWorktreeOwnerSnapshot>> {
        let mut snapshots = Vec::with_capacity(runs.len());
        for run in runs {
            let work_units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
                .all(&self.db)
                .await?
                .into_iter()
                .map(work_unit_record)
                .collect::<Result<Vec<_>>>()?;
            let outcomes = entities::agent_outcome::Entity::find()
                .filter(entities::agent_outcome::Column::TaskRunId.eq(run.id.clone()))
                .all(&self.db)
                .await?
                .into_iter()
                .map(agent_outcome_record)
                .collect::<Result<Vec<_>>>()?;
            let resources = work_units
                .into_iter()
                .map(|work_unit| {
                    let outcome = outcomes
                        .iter()
                        .find(|outcome| {
                            outcome.work_unit_id.as_deref() == Some(work_unit.id.as_str())
                        })
                        .cloned();
                    let creation_state = if outcome.as_ref().is_some_and(|outcome| {
                        outcome.error.as_deref() == Some(RESTART_BEFORE_CREATE_DIAGNOSTIC)
                    }) {
                        TaskWorktreeCreationState::UncreatedBeforeRestart
                    } else {
                        TaskWorktreeCreationState::MustExist
                    };
                    TaskWorktreeOwnerResource {
                        work_unit,
                        outcome,
                        creation_state,
                    }
                })
                .collect();
            snapshots.push(TaskWorktreeOwnerSnapshot {
                run: task_run_record(run)?,
                resources,
            });
        }
        Ok(snapshots)
    }

    pub(crate) async fn reconcile_task_agents_after_restart(
        &self,
        task_run_id: &str,
    ) -> Result<RestartAgentReconciliation> {
        let tx = self.db.begin().await?;
        let result = async {
            entities::task_run::Entity::find_by_id(task_run_id.to_string())
                .one(&tx)
                .await?
                .context("task run not found during agent restart reconciliation")?;
            let work_units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id.to_string()))
                .all(&tx)
                .await?;
            let outcomes = entities::agent_outcome::Entity::find()
                .filter(entities::agent_outcome::Column::TaskRunId.eq(task_run_id.to_string()))
                .all(&tx)
                .await?;

            validate_pairs(task_run_id, &work_units, &outcomes)?;

            let pending_work_units = work_units
                .iter()
                .filter(|unit| unit.status == WorkUnitStatus::Pending.as_str())
                .map(|unit| unit.id.clone())
                .collect::<std::collections::HashSet<_>>();
            let now = unix_seconds();
            let mut summary = RestartAgentReconciliation::default();
            for work_unit in work_units {
                let status = WorkUnitStatus::from_str(&work_unit.status)
                    .with_context(|| format!("invalid work unit status: {}", work_unit.status))?;
                if is_transient_work_unit(status) {
                    let mut active: entities::work_unit::ActiveModel = work_unit.into();
                    active.status = Set(WorkUnitStatus::Cancelled.as_str().to_string());
                    active.updated_at = Set(now);
                    active.update(&tx).await?;
                    summary.cancelled_work_units += 1;
                }
            }
            for outcome in outcomes {
                let status = AgentOutcomeStatus::from_str(&outcome.status)
                    .with_context(|| format!("invalid agent outcome status: {}", outcome.status))?;
                let cancel = is_transient_outcome(status);
                let already_observed = outcome.terminal_observed != 0;
                if cancel || !already_observed {
                    let before_create = outcome
                        .work_unit_id
                        .as_deref()
                        .is_some_and(|id| pending_work_units.contains(id))
                        && status == AgentOutcomeStatus::Queued;
                    let mut active: entities::agent_outcome::ActiveModel = outcome.into();
                    if cancel {
                        active.status = Set(AgentOutcomeStatus::Cancelled.as_str().to_string());
                        active.error = Set(Some(
                            if before_create {
                                RESTART_BEFORE_CREATE_DIAGNOSTIC
                            } else {
                                RESTART_DIAGNOSTIC
                            }
                            .to_string(),
                        ));
                        summary.cancelled_outcomes += 1;
                    }
                    active.terminal_observed = Set(1);
                    active.updated_at = Set(now);
                    active.update(&tx).await?;
                }
            }
            Ok(summary)
        }
        .await;
        match result {
            Ok(summary) => {
                tx.commit().await?;
                Ok(summary)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }
}

fn validate_pairs(
    task_run_id: &str,
    work_units: &[entities::work_unit::Model],
    outcomes: &[entities::agent_outcome::Model],
) -> Result<()> {
    let units_by_id = work_units
        .iter()
        .map(|unit| (unit.id.as_str(), unit))
        .collect::<HashMap<_, _>>();
    let mut outcomes_by_unit = HashMap::new();
    for outcome in outcomes {
        if outcome.task_run_id != task_run_id {
            bail!("agent outcome belongs to another task run");
        }
        let Some(work_unit_id) = outcome.work_unit_id.as_deref() else {
            if outcome.role == "executor" {
                bail!("executor outcome has no work unit");
            }
            continue;
        };
        let unit = units_by_id
            .get(work_unit_id)
            .context("agent outcome work unit does not exist")?;
        if unit.task_run_id != task_run_id
            || unit.agent_id.as_deref() != Some(outcome.agent_id.as_str())
            || unit.attempt != outcome.attempt
            || outcome.role != "executor"
        {
            bail!("agent outcome and work unit do not match");
        }
        if outcomes_by_unit.insert(work_unit_id, outcome).is_some() {
            bail!("work unit has multiple agent outcomes");
        }
        validate_status_pair(unit, outcome)?;
    }
    for unit in work_units {
        if unit.task_run_id != task_run_id {
            bail!("work unit belongs to another task run");
        }
        if unit.agent_id.is_none() || !outcomes_by_unit.contains_key(unit.id.as_str()) {
            bail!("work unit and agent outcome do not match");
        }
    }
    Ok(())
}

fn validate_status_pair(
    unit: &entities::work_unit::Model,
    outcome: &entities::agent_outcome::Model,
) -> Result<()> {
    let unit_status = WorkUnitStatus::from_str(&unit.status)
        .with_context(|| format!("invalid work unit status: {}", unit.status))?;
    let outcome_status = AgentOutcomeStatus::from_str(&outcome.status)
        .with_context(|| format!("invalid agent outcome status: {}", outcome.status))?;
    let valid = matches!(
        (unit_status, outcome_status),
        (WorkUnitStatus::Pending, AgentOutcomeStatus::Queued)
            | (WorkUnitStatus::Running, AgentOutcomeStatus::Running)
            | (
                WorkUnitStatus::WaitingForDelivery,
                AgentOutcomeStatus::WaitingForDelivery
            )
            | (WorkUnitStatus::Delivered, AgentOutcomeStatus::Completed)
            | (WorkUnitStatus::Merged, AgentOutcomeStatus::Completed)
            | (WorkUnitStatus::Failed, AgentOutcomeStatus::Failed)
            | (WorkUnitStatus::Cancelled, AgentOutcomeStatus::Cancelled)
    );
    if !valid {
        bail!("agent outcome and work unit statuses do not match");
    }
    if matches!(
        unit_status,
        WorkUnitStatus::Delivered | WorkUnitStatus::Merged
    ) && outcome.delivery_json.is_none()
    {
        bail!("delivered work unit has no durable delivery");
    }
    Ok(())
}

fn is_transient_work_unit(status: WorkUnitStatus) -> bool {
    matches!(
        status,
        WorkUnitStatus::Pending | WorkUnitStatus::Running | WorkUnitStatus::WaitingForDelivery
    )
}

fn is_transient_outcome(status: AgentOutcomeStatus) -> bool {
    matches!(
        status,
        AgentOutcomeStatus::Queued
            | AgentOutcomeStatus::Running
            | AgentOutcomeStatus::WaitingForDelivery
    )
}
