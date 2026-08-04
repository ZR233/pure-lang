use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentOutcomeStatus, RestartAgentReconciliation, ReviewScope, ReviewVerdict, TaskRunPhase,
    TaskWorktreeCleanupState, TaskWorktreeCreationState, TaskWorktreeOwnerResource,
    TaskWorktreeOwnerSnapshot, WorkCompletionStatus, WorkUnitStatus,
};

use super::{
    merge::parse_required_evidence, outcome::agent_outcome_record, task_run_record,
    work_completion::work_completion_record, work_unit::work_unit_record,
};

const RESTART_DIAGNOSTIC: &str = "agent interrupted by application restart";
const RESTART_BEFORE_CREATE_DIAGNOSTIC: &str =
    "agent interrupted by application restart before worktree creation";
const REVIEW_RESTART_DIAGNOSTIC: &str =
    "reviewer interrupted by application restart before review_exit";

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
            let merges = entities::merge_record::Entity::find()
                .filter(entities::merge_record::Column::TaskRunId.eq(run.id.clone()))
                .all(&self.db)
                .await?;
            let completions = entities::work_completion::Entity::find()
                .filter(entities::work_completion::Column::TaskRunId.eq(run.id.clone()))
                .all(&self.db)
                .await?;
            let resources = work_units
                .into_iter()
                .map(|work_unit| -> Result<_> {
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
                    let completion = completions
                        .iter()
                        .filter(|completion| completion.work_unit_id == work_unit.id)
                        .max_by_key(|completion| completion.revision)
                        .cloned()
                        .map(work_completion_record)
                        .transpose()?;
                    let cleanup_state = merge_cleanup_state(&work_unit, &merges)?;
                    Ok(TaskWorktreeOwnerResource {
                        work_unit,
                        outcome,
                        completion,
                        creation_state,
                        cleanup_state,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
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
            let run = entities::task_run::Entity::find_by_id(task_run_id.to_string())
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

            let now = unix_seconds();
            reconcile_pending_reviews_after_restart(&tx, &run, &work_units, &outcomes, now).await?;
            let pending_work_units = work_units
                .iter()
                .filter(|unit| unit.status == WorkUnitStatus::Pending.as_str())
                .map(|unit| unit.id.clone())
                .collect::<std::collections::HashSet<_>>();
            let mut summary = RestartAgentReconciliation::default();
            for work_unit in work_units {
                let status = WorkUnitStatus::from_str(&work_unit.status)
                    .with_context(|| format!("invalid work unit status: {}", work_unit.status))?;
                if status == WorkUnitStatus::Pending {
                    let mut active: entities::work_unit::ActiveModel = work_unit.into();
                    active.status = Set(WorkUnitStatus::Cancelled.as_str().to_string());
                    active.updated_at = Set(now);
                    active.update(&tx).await?;
                    summary.cancelled_work_units += 1;
                } else if status == WorkUnitStatus::Running {
                    let mut active: entities::work_unit::ActiveModel = work_unit.into();
                    active.status = Set(WorkUnitStatus::AwaitingCompletion.as_str().to_string());
                    active.updated_at = Set(now);
                    active.update(&tx).await?;
                }
            }
            for outcome in outcomes {
                let status = AgentOutcomeStatus::from_str(&outcome.status)
                    .with_context(|| format!("invalid agent outcome status: {}", outcome.status))?;
                let cancel = is_transient_outcome(status);
                if cancel {
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

async fn reconcile_pending_reviews_after_restart(
    tx: &sea_orm::DatabaseTransaction,
    run: &entities::task_run::Model,
    work_units: &[entities::work_unit::Model],
    outcomes: &[entities::agent_outcome::Model],
    now: i64,
) -> Result<()> {
    let rounds = entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
        .filter(entities::review_round::Column::Status.eq(ReviewVerdict::Pending.as_str()))
        .all(tx)
        .await?;
    for round in rounds {
        validate_restart_reviewer(&round, outcomes)?;
        match ReviewScope::from_str(&round.scope).context("invalid stored review scope")? {
            ReviewScope::Delivery => {
                restore_delivery_review_after_restart(tx, &round, work_units, now).await?;
            }
            ReviewScope::Integrated => {
                if round.work_unit_id.is_some()
                    || round.completion_id.is_some()
                    || round.completion_revision.is_some()
                    || round.reviewed_head != run.expected_head
                    || run.phase != TaskRunPhase::Reviewing.as_str()
                {
                    bail!("pending integrated review does not match the task run");
                }
                let mut active: entities::task_run::ActiveModel = run.clone().into();
                active.phase = Set(TaskRunPhase::Reworking.as_str().to_string());
                active.status_message = Set(Some(REVIEW_RESTART_DIAGNOSTIC.to_string()));
                active.updated_at = Set(now);
                active.update(tx).await?;
            }
        }
        let mut active: entities::review_round::ActiveModel = round.into();
        active.status = Set(ReviewVerdict::Failed.as_str().to_string());
        active.summary = Set(Some(REVIEW_RESTART_DIAGNOSTIC.to_string()));
        active.updated_at = Set(now);
        active.update(tx).await?;
    }
    Ok(())
}

fn validate_restart_reviewer(
    round: &entities::review_round::Model,
    outcomes: &[entities::agent_outcome::Model],
) -> Result<()> {
    let Some(reviewer_agent_id) = round.reviewer_agent_id.as_deref() else {
        return Ok(());
    };
    let matching = outcomes
        .iter()
        .filter(|outcome| outcome.agent_id == reviewer_agent_id)
        .collect::<Vec<_>>();
    let outcome = match matching.as_slice() {
        [outcome] => *outcome,
        [] => bail!("pending review has no reviewer outcome"),
        _ => bail!("pending review has multiple reviewer outcomes"),
    };
    if outcome.task_run_id != round.task_run_id
        || outcome.work_unit_id != round.work_unit_id
        || outcome.role != "reviewer"
        || outcome.owner_path != "/root"
        || outcome.initiated_by != "planner"
        || outcome.requested_by_call_id != round.requested_by_call_id
        || outcome.attempt != round.round
        || !matches!(
            AgentOutcomeStatus::from_str(&outcome.status),
            Some(AgentOutcomeStatus::Queued | AgentOutcomeStatus::Running)
        )
    {
        bail!("pending review and reviewer outcome do not match");
    }
    Ok(())
}

async fn restore_delivery_review_after_restart(
    tx: &sea_orm::DatabaseTransaction,
    round: &entities::review_round::Model,
    work_units: &[entities::work_unit::Model],
    now: i64,
) -> Result<()> {
    let work_unit_id = round
        .work_unit_id
        .as_deref()
        .context("pending delivery review has no work unit")?;
    let completion_id = round
        .completion_id
        .as_deref()
        .context("pending delivery review has no completion")?;
    let completion_revision = round
        .completion_revision
        .context("pending delivery review has no completion revision")?;
    let work_unit = work_units
        .iter()
        .find(|unit| unit.id == work_unit_id)
        .context("pending delivery review work unit not found")?;
    let completion = entities::work_completion::Entity::find_by_id(completion_id)
        .one(tx)
        .await?
        .context("pending delivery review completion not found")?;
    let reviewed_head = completion
        .head_commit
        .as_deref()
        .unwrap_or(work_unit.base_commit.as_str());
    if work_unit.status != WorkUnitStatus::Reviewing.as_str()
        || completion.task_run_id != round.task_run_id
        || completion.work_unit_id != work_unit.id
        || completion.revision != completion_revision
        || completion.status != WorkCompletionStatus::ReadyForReview.as_str()
        || round.reviewed_head != reviewed_head
    {
        bail!("pending delivery review target does not match its completion");
    }
    let mut active: entities::work_unit::ActiveModel = work_unit.clone().into();
    active.status = Set(WorkUnitStatus::ReadyForReview.as_str().to_string());
    active.updated_at = Set(now);
    active.update(tx).await?;
    Ok(())
}

fn merge_cleanup_state(
    work_unit: &crate::studio::task_coordinator::WorkUnitRecord,
    merges: &[entities::merge_record::Model],
) -> Result<TaskWorktreeCleanupState> {
    if work_unit.status != WorkUnitStatus::Merged {
        return Ok(TaskWorktreeCleanupState::NotMerged);
    }
    let mut matches = Vec::new();
    for merge in merges {
        if merge.status != crate::studio::task_coordinator::MergeStatus::Merged.as_str() {
            continue;
        }
        let evidence = parse_required_evidence(merge.verification_json.as_deref())?;
        if evidence.work_unit_id == work_unit.id {
            matches.push((merge, evidence));
        }
    }
    let (merge, evidence) = match matches.as_slice() {
        [(merge, evidence)] => (*merge, evidence),
        [] => bail!("merged work unit has no accepted merge evidence"),
        _ => bail!("merged work unit has ambiguous accepted merge evidence"),
    };
    match evidence
        .cleanup
        .as_ref()
        .map(|cleanup| cleanup.status.as_str())
    {
        Some("discarded" | "alreadyAbsent") => Ok(TaskWorktreeCleanupState::Cleanup),
        None | Some("attempting") => Ok(TaskWorktreeCleanupState::Replay {
            merge_id: merge.id.clone(),
        }),
        Some("failed" | "deferred") => Ok(TaskWorktreeCleanupState::Protect),
        Some(status) => bail!("accepted merge has unknown cleanup status `{status}`"),
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
        if outcome.role != "executor" {
            continue;
        }
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
                WorkUnitStatus::AwaitingCompletion,
                AgentOutcomeStatus::Completed
            )
            | (
                WorkUnitStatus::AwaitingCompletion,
                AgentOutcomeStatus::Failed
            )
            | (
                WorkUnitStatus::AwaitingCompletion,
                AgentOutcomeStatus::Cancelled
            )
            | (
                WorkUnitStatus::ReadyForReview,
                AgentOutcomeStatus::Completed
            )
            | (WorkUnitStatus::Reviewing, AgentOutcomeStatus::Completed)
            | (
                WorkUnitStatus::ChangesRequested,
                AgentOutcomeStatus::Completed
            )
            | (WorkUnitStatus::Approved, AgentOutcomeStatus::Completed)
            | (WorkUnitStatus::Merging, AgentOutcomeStatus::Completed)
            | (WorkUnitStatus::Merged, AgentOutcomeStatus::Completed)
            | (WorkUnitStatus::NoDelivery, AgentOutcomeStatus::Completed)
            | (WorkUnitStatus::Failed, AgentOutcomeStatus::Failed)
            | (WorkUnitStatus::Cancelled, AgentOutcomeStatus::Cancelled)
    );
    if !valid {
        bail!("agent outcome and work unit statuses do not match");
    }
    Ok(())
}

fn is_transient_outcome(status: AgentOutcomeStatus) -> bool {
    matches!(
        status,
        AgentOutcomeStatus::Queued | AgentOutcomeStatus::Running
    )
}
