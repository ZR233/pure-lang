mod allocation;
mod completion;
mod discard;
mod failure;
mod merge;
mod planner_wake;
mod recovery;
mod review;
mod work_completion;
mod work_unit;

pub(in crate::studio) use completion::PendingTaskInteractions;

use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::StudioMode;
use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    BlockedRecovery, CreateTaskRun, FinalizedDesign, ProjectLeaseRecord, TaskCommand, TaskContext,
    TaskRun, TaskRunState, TaskRunStateKind, is_retryable_merge_recovery_message,
};

impl StudioStore {
    pub(crate) async fn create_task_run_with_lease(
        &self,
        input: CreateTaskRun,
    ) -> Result<(TaskRun, ProjectLeaseRecord)> {
        validate_create_task_run(&input)?;
        let Some(root_thread) = self.read_thread(&input.root_thread_id).await? else {
            bail!("task root Thread not found");
        };
        if root_thread.mode != StudioMode::Task {
            bail!("task coordinator requires a task mode root Thread");
        }

        let tx = self.db.begin().await?;
        let now = unix_seconds();
        let task_run_id = new_id("task-run");
        let state_json = serde_json::to_string(&TaskRunState::new())
            .context("failed to encode initial task state")?;
        let task_model = entities::task_run::ActiveModel {
            id: Set(task_run_id.clone()),
            project_id: Set(input.project_id.clone()),
            root_thread_id: Set(input.root_thread_id),
            plan: Set(input.plan),
            workspace_root: Set(input.workspace_root),
            state_json: Set(state_json),
            revision: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&tx)
        .await?;
        let lease_model = entities::project_lease::ActiveModel {
            id: Set(new_id("project-lease")),
            task_run_id: Set(task_run_id),
            project_id: Set(input.project_id),
            acquired_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&tx)
        .await
        .context("project is already leased by another task")?;
        tx.commit().await?;

        Ok((
            task_run_record(task_model)?,
            project_lease_record(lease_model),
        ))
    }

    pub(crate) async fn read_task_run(&self, task_run_id: &str) -> Result<Option<TaskRun>> {
        entities::task_run::Entity::find_by_id(task_run_id.to_string())
            .one(&self.db)
            .await?
            .map(task_run_record)
            .transpose()
    }

    pub(crate) async fn list_active_task_runs(&self) -> Result<Vec<TaskRun>> {
        let models = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::StateKind.is_not_in([
                TaskRunStateKind::Completed.as_str(),
                TaskRunStateKind::Failed.as_str(),
                TaskRunStateKind::Cancelled.as_str(),
            ]))
            .order_by_asc(entities::task_run::Column::CreatedAt)
            .order_by_asc(entities::task_run::Column::Id)
            .all(&self.db)
            .await?;
        models.into_iter().map(task_run_record).collect()
    }

    pub(crate) async fn list_retryable_blocked_merge_task_runs(&self) -> Result<Vec<TaskRun>> {
        let models = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::StateKind.eq(TaskRunStateKind::Blocked.as_str()))
            .order_by_asc(entities::task_run::Column::CreatedAt)
            .order_by_asc(entities::task_run::Column::Id)
            .all(&self.db)
            .await?;
        let runs = models
            .into_iter()
            .map(task_run_record)
            .collect::<Result<Vec<_>>>()?;
        Ok(runs
            .into_iter()
            .filter(|run| {
                run.status_message()
                    .is_some_and(is_retryable_merge_recovery_message)
            })
            .collect())
    }

    pub(crate) async fn list_task_runs_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<TaskRun>> {
        let thread_ids = self.list_project_thread_ids(project_id).await?;
        if thread_ids.is_empty() {
            return Ok(Vec::new());
        }
        let models = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::RootThreadId.is_in(thread_ids))
            .order_by_asc(entities::task_run::Column::CreatedAt)
            .order_by_asc(entities::task_run::Column::Id)
            .all(&self.db)
            .await?;
        models.into_iter().map(task_run_record).collect()
    }

    pub(crate) async fn read_active_task_run_for_root_thread(
        &self,
        root_thread_id: &str,
    ) -> Result<TaskRun> {
        self.find_active_task_run_for_root_thread(root_thread_id)
            .await?
            .context("active task run not found for this root Thread")
    }

    pub(crate) async fn find_latest_task_run_for_root_thread(
        &self,
        root_thread_id: &str,
    ) -> Result<Option<TaskRun>> {
        entities::task_run::Entity::find()
            .filter(entities::task_run::Column::RootThreadId.eq(root_thread_id.to_string()))
            .order_by_desc(entities::task_run::Column::CreatedAt)
            .order_by_desc(entities::task_run::Column::Id)
            .one(&self.db)
            .await?
            .map(task_run_record)
            .transpose()
    }

    pub(crate) async fn find_active_task_run_for_root_thread(
        &self,
        root_thread_id: &str,
    ) -> Result<Option<TaskRun>> {
        let models = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::RootThreadId.eq(root_thread_id.to_string()))
            .filter(entities::task_run::Column::StateKind.is_not_in([
                TaskRunStateKind::Completed.as_str(),
                TaskRunStateKind::Failed.as_str(),
                TaskRunStateKind::Cancelled.as_str(),
            ]))
            .order_by_asc(entities::task_run::Column::CreatedAt)
            .order_by_asc(entities::task_run::Column::Id)
            .all(&self.db)
            .await?;
        match models.as_slice() {
            [] => Ok(None),
            [model] => task_run_record(model.clone()).map(Some),
            _ => bail!("multiple active task runs found for this root Thread"),
        }
    }

    pub(crate) async fn read_project_lease(
        &self,
        task_run_id: &str,
    ) -> Result<Option<ProjectLeaseRecord>> {
        Ok(entities::project_lease::Entity::find()
            .filter(entities::project_lease::Column::TaskRunId.eq(task_run_id.to_string()))
            .one(&self.db)
            .await?
            .map(project_lease_record))
    }

    #[cfg(test)]
    pub(crate) async fn transition_task_run(
        &self,
        task_run_id: &str,
        next: TaskRunStateKind,
        status_message: Option<String>,
    ) -> Result<TaskRun> {
        self.transition_task_run_after_read(task_run_id, next, status_message, None)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn transition_task_run_after_read(
        &self,
        task_run_id: &str,
        next: TaskRunStateKind,
        status_message: Option<String>,
        read_barrier: Option<&tokio::sync::Barrier>,
    ) -> Result<TaskRun> {
        let current = entities::task_run::Entity::find_by_id(task_run_id.to_string())
            .one(&self.db)
            .await?
            .context("task run not found")?;
        let current_record = task_run_record(current.clone())?;
        let command = test_transition_command(&current_record, next, status_message)?;
        let decision = current_record.decide(command)?;
        if let Some(read_barrier) = read_barrier {
            read_barrier.wait().await;
        }
        let expected_revision = current.revision;
        let next_revision = expected_revision
            .checked_add(1)
            .context("task revision overflow")?;
        let active = entities::task_run::ActiveModel {
            state_json: Set(serde_json::to_string(&decision.next_state)?),
            revision: Set(next_revision),
            updated_at: Set(unix_seconds()),
            ..Default::default()
        };
        let updated = entities::task_run::Entity::update_many()
            .set(active)
            .filter(entities::task_run::Column::Id.eq(task_run_id.to_string()))
            .filter(entities::task_run::Column::Revision.eq(expected_revision))
            .exec(&self.db)
            .await?;
        if updated.rows_affected != 1 {
            bail!("task state changed concurrently");
        }
        self.read_task_run(task_run_id)
            .await?
            .context("task run disappeared after phase transition")
    }

    pub(crate) async fn finalize_task_design(
        &self,
        task_run_id: &str,
        summary: &str,
    ) -> Result<Option<TaskRun>> {
        let tx = self.db.begin().await?;
        let Some(task) = entities::task_run::Entity::find_by_id(task_run_id.to_string())
            .one(&tx)
            .await?
        else {
            tx.rollback().await?;
            return Ok(None);
        };
        let run = task_run_record(task.clone())?;
        if run.kind() != TaskRunStateKind::DesignUpdating {
            tx.rollback().await?;
            bail!(
                "task_finalize_design requires phase designUpdating; current phase is {}",
                run.kind().as_str()
            );
        }
        let lease = entities::project_lease::Entity::find()
            .filter(entities::project_lease::Column::TaskRunId.eq(task_run_id.to_string()))
            .one(&tx)
            .await?
            .context("task project lease not found")?;
        if lease.project_id != run.project_id {
            tx.rollback().await?;
            bail!("TaskRun and project lease no longer have the same owner");
        }

        let finalized_design = FinalizedDesign {
            summary: summary.to_string(),
        };
        let decision = run.decide(TaskCommand::FinalizeDesign(finalized_design))?;
        let Some(updated) =
            compare_and_swap_task_run(&tx, &task, Some(&decision.next_state)).await?
        else {
            tx.rollback().await?;
            return Ok(None);
        };
        tx.commit().await?;
        task_run_record(updated).map(Some)
    }

    #[cfg(test)]
    pub(crate) async fn release_project_lease(&self, task_run_id: &str) -> Result<()> {
        entities::project_lease::Entity::delete_many()
            .filter(entities::project_lease::Column::TaskRunId.eq(task_run_id.to_string()))
            .exec(&self.db)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
fn test_transition_command(
    run: &TaskRun,
    next: TaskRunStateKind,
    status_message: Option<String>,
) -> Result<TaskCommand> {
    let message = status_message.unwrap_or_else(|| "test transition".to_string());
    let command = match next {
        TaskRunStateKind::DesignUpdating => {
            bail!("a TaskRun cannot transition back to designUpdating")
        }
        TaskRunStateKind::Implementing if run.kind() == TaskRunStateKind::DesignUpdating => {
            TaskCommand::FinalizeDesign(FinalizedDesign { summary: message })
        }
        TaskRunStateKind::Implementing => TaskCommand::BeginImplementing,
        TaskRunStateKind::Merging => TaskCommand::BeginMerging {
            status_message: Some(message),
        },
        TaskRunStateKind::Reviewing => TaskCommand::BeginReviewing(
            crate::studio::task_coordinator::ReviewTarget::Integration {
                reviewed_head: "recorded-integration".to_string(),
            },
        ),
        TaskRunStateKind::Reworking => TaskCommand::BeginReworking {
            status_message: message,
        },
        TaskRunStateKind::Stopping => TaskCommand::RequestStop(
            (
                crate::studio::task_coordinator::TaskStopOrigin::PlannerDecision,
                crate::studio::task_coordinator::TaskStopReason::new(message)
                    .context("test stop reason must not be empty")?,
                unix_seconds(),
            )
                .into(),
        ),
        TaskRunStateKind::Blocked => TaskCommand::Block {
            recovery: if is_retryable_merge_recovery_message(&message) {
                BlockedRecovery::RetryMerge
            } else {
                BlockedRecovery::ManualOnly
            },
            message,
        },
        TaskRunStateKind::Completed => TaskCommand::Complete,
        TaskRunStateKind::Failed => TaskCommand::Fail {
            message,
            failure_id: None,
        },
        TaskRunStateKind::Cancelled => TaskCommand::Cancel {
            message,
            request: run.stop_request().cloned(),
        },
    };
    Ok(command)
}

fn validate_create_task_run(input: &CreateTaskRun) -> Result<()> {
    for (label, value) in [
        ("projectId", input.project_id.as_str()),
        ("rootThreadId", input.root_thread_id.as_str()),
        ("plan", input.plan.as_str()),
        ("workspaceRoot", input.workspace_root.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("{label} must not be empty");
        }
    }
    Ok(())
}

pub(super) fn task_run_record(model: entities::task_run::Model) -> Result<TaskRun> {
    let state: TaskRunState =
        serde_json::from_str(&model.state_json).context("invalid stored task state JSON")?;
    if state.kind().as_str() != model.state_kind {
        bail!(
            "stored task state discriminator mismatch: generated {}, decoded {}",
            model.state_kind,
            state.kind().as_str()
        );
    }
    Ok(TaskRun {
        context: TaskContext {
            id: model.id,
            project_id: model.project_id,
            root_thread_id: model.root_thread_id,
            plan: model.plan,
            workspace_root: model.workspace_root,
        },
        state,
        revision: u64::try_from(model.revision)
            .context("stored task revision must not be negative")?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

pub(super) async fn apply_task_command(
    tx: &sea_orm::DatabaseTransaction,
    model: entities::task_run::Model,
    command: TaskCommand,
) -> Result<entities::task_run::Model> {
    let run = task_run_record(model.clone())?;
    let decision = run.decide(command)?;
    compare_and_swap_task_run(tx, &model, Some(&decision.next_state))
        .await?
        .context("TaskRun state update lost its revision CAS")
}

pub(super) async fn compare_and_swap_task_run<C>(
    connection: &C,
    model: &entities::task_run::Model,
    next_state: Option<&TaskRunState>,
) -> Result<Option<entities::task_run::Model>>
where
    C: sea_orm::ConnectionTrait,
{
    let next_revision = model
        .revision
        .checked_add(1)
        .context("task revision overflow")?;
    let mut active = entities::task_run::ActiveModel {
        revision: Set(next_revision),
        updated_at: Set(unix_seconds()),
        ..Default::default()
    };
    if let Some(next_state) = next_state {
        active.state_json = Set(serde_json::to_string(next_state)?);
    }
    let result = entities::task_run::Entity::update_many()
        .set(active)
        .filter(entities::task_run::Column::Id.eq(model.id.clone()))
        .filter(entities::task_run::Column::Revision.eq(model.revision))
        .exec(connection)
        .await?;
    if result.rows_affected != 1 {
        return Ok(None);
    }
    entities::task_run::Entity::find_by_id(model.id.clone())
        .one(connection)
        .await
        .context("failed to reload TaskRun after revision CAS")
}

fn project_lease_record(model: entities::project_lease::Model) -> ProjectLeaseRecord {
    ProjectLeaseRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        project_id: model.project_id,
        acquired_at: model.acquired_at,
        updated_at: model.updated_at,
    }
}

pub(super) async fn write_task_terminal_fact(
    tx: &sea_orm::DatabaseTransaction,
    run: entities::task_run::Model,
    next_state: TaskRunStateKind,
    status_message: Option<String>,
    expected_generation: Option<u64>,
) -> Result<entities::task_run::Model> {
    if !next_state.is_terminal() && next_state != TaskRunStateKind::Blocked {
        bail!("task terminal fact requires blocked or a terminal state");
    }
    let current = task_run_record(run.clone())?;
    let task_generation = current.state.generation();
    if expected_generation.is_some_and(|expected| expected != task_generation) {
        bail!("task terminal fact belongs to another generation");
    }
    if current.kind().is_terminal() {
        if current.kind() == next_state {
            return Ok(run);
        }
        bail!("task already has a different terminal state");
    }
    let message = status_message.unwrap_or_else(|| next_state.as_str().to_string());
    let command = match next_state {
        TaskRunStateKind::Blocked => TaskCommand::Block {
            recovery: if is_retryable_merge_recovery_message(&message) {
                BlockedRecovery::RetryMerge
            } else {
                BlockedRecovery::ManualOnly
            },
            message,
        },
        TaskRunStateKind::Completed => TaskCommand::Complete,
        TaskRunStateKind::Failed => TaskCommand::Fail {
            message,
            failure_id: None,
        },
        TaskRunStateKind::Cancelled => TaskCommand::Cancel {
            message,
            request: current.stop_request().cloned(),
        },
        TaskRunStateKind::DesignUpdating
        | TaskRunStateKind::Implementing
        | TaskRunStateKind::Merging
        | TaskRunStateKind::Reviewing
        | TaskRunStateKind::Reworking
        | TaskRunStateKind::Stopping => unreachable!("checked above"),
    };
    let decision = current.decide(command)?;
    compare_and_swap_task_run(tx, &run, Some(&decision.next_state))
        .await?
        .context("TaskRun terminal update lost its revision CAS")
}

async fn delete_blocked_project_lease(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
) -> Result<()> {
    entities::project_lease::Entity::delete_many()
        .filter(entities::project_lease::Column::TaskRunId.eq(task_run_id.to_string()))
        .exec(tx)
        .await?;
    Ok(())
}
