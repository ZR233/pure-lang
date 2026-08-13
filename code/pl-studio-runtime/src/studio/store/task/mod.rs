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

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    BranchLeaseRecord, CreateTaskRun, TaskRunPhase, TaskRunRecord, TaskStopOrigin, TaskStopReason,
    is_retryable_merge_recovery_message,
};

impl StudioStore {
    pub(crate) async fn create_task_run_with_lease(
        &self,
        input: CreateTaskRun,
    ) -> Result<(TaskRunRecord, BranchLeaseRecord)> {
        validate_create_task_run(&input)?;
        let Some(root_thread) = self.read_thread(&input.root_thread_id).await? else {
            bail!("task root Thread not found or uses a legacy mode");
        };
        if root_thread.mode != "task" {
            bail!("task coordinator requires a task mode root Thread");
        }

        let tx = self.db.begin().await?;
        let now = unix_seconds();
        let task_run_id = new_id("task-run");
        let task_model = entities::task_run::ActiveModel {
            id: Set(task_run_id.clone()),
            root_thread_id: Set(input.root_thread_id),
            phase: Set(input.phase.as_str().to_string()),
            plan: Set(input.plan),
            workspace_root: Set(input.workspace_root),
            git_common_dir: Set(input.git_common_dir.clone()),
            branch: Set(input.branch.clone()),
            base_commit: Set(input.head_commit.clone()),
            expected_head: Set(input.head_commit.clone()),
            design_commit: Set(None),
            status_message: Set(None),
            stop_requested: Set(0),
            stop_requested_origin: Set(None),
            stop_requested_reason: Set(None),
            stop_requested_at: Set(None),
            task_generation: Set(0),
            terminal_generation: Set(None),
            terminal_failure_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&tx)
        .await?;
        let lease_model = entities::branch_lease::ActiveModel {
            id: Set(new_id("branch-lease")),
            task_run_id: Set(task_run_id),
            git_common_dir: Set(input.git_common_dir),
            branch: Set(input.branch),
            expected_head: Set(input.head_commit),
            acquired_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&tx)
        .await
        .context("branch is already leased by another task")?;
        tx.commit().await?;

        Ok((
            task_run_record(task_model)?,
            branch_lease_record(lease_model),
        ))
    }

    pub(crate) async fn read_task_run(&self, task_run_id: &str) -> Result<Option<TaskRunRecord>> {
        entities::task_run::Entity::find_by_id(task_run_id.to_string())
            .one(&self.db)
            .await?
            .map(task_run_record)
            .transpose()
    }

    pub(crate) async fn list_active_task_runs(&self) -> Result<Vec<TaskRunRecord>> {
        let models = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::Phase.is_not_in([
                TaskRunPhase::Completed.as_str(),
                TaskRunPhase::Blocked.as_str(),
                TaskRunPhase::Failed.as_str(),
                TaskRunPhase::Cancelled.as_str(),
            ]))
            .order_by_asc(entities::task_run::Column::CreatedAt)
            .order_by_asc(entities::task_run::Column::Id)
            .all(&self.db)
            .await?;
        models.into_iter().map(task_run_record).collect()
    }

    pub(crate) async fn list_retryable_blocked_merge_task_runs(
        &self,
    ) -> Result<Vec<TaskRunRecord>> {
        let models = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::Phase.eq(TaskRunPhase::Blocked.as_str()))
            .order_by_asc(entities::task_run::Column::CreatedAt)
            .order_by_asc(entities::task_run::Column::Id)
            .all(&self.db)
            .await?;
        models
            .into_iter()
            .filter(|model| {
                model
                    .status_message
                    .as_deref()
                    .is_some_and(is_retryable_merge_recovery_message)
            })
            .map(task_run_record)
            .collect()
    }

    pub(crate) async fn list_task_runs_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<TaskRunRecord>> {
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
    ) -> Result<TaskRunRecord> {
        self.find_active_task_run_for_root_thread(root_thread_id)
            .await?
            .context("active task run not found for this root Thread")
    }

    pub(crate) async fn find_latest_task_run_for_root_thread(
        &self,
        root_thread_id: &str,
    ) -> Result<Option<TaskRunRecord>> {
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
    ) -> Result<Option<TaskRunRecord>> {
        let models = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::RootThreadId.eq(root_thread_id.to_string()))
            .filter(entities::task_run::Column::Phase.is_not_in([
                TaskRunPhase::Completed.as_str(),
                TaskRunPhase::Blocked.as_str(),
                TaskRunPhase::Failed.as_str(),
                TaskRunPhase::Cancelled.as_str(),
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

    pub(crate) async fn read_branch_lease(
        &self,
        task_run_id: &str,
    ) -> Result<Option<BranchLeaseRecord>> {
        Ok(entities::branch_lease::Entity::find()
            .filter(entities::branch_lease::Column::TaskRunId.eq(task_run_id.to_string()))
            .one(&self.db)
            .await?
            .map(branch_lease_record))
    }

    #[cfg(test)]
    pub(crate) async fn transition_task_run(
        &self,
        task_run_id: &str,
        next: TaskRunPhase,
        status_message: Option<String>,
    ) -> Result<TaskRunRecord> {
        self.transition_task_run_after_read(task_run_id, next, status_message, None)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn transition_task_run_after_read(
        &self,
        task_run_id: &str,
        next: TaskRunPhase,
        status_message: Option<String>,
        read_barrier: Option<&tokio::sync::Barrier>,
    ) -> Result<TaskRunRecord> {
        let current = entities::task_run::Entity::find_by_id(task_run_id.to_string())
            .one(&self.db)
            .await?
            .context("task run not found")?;
        let current_phase = TaskRunPhase::from_str(&current.phase)
            .with_context(|| format!("invalid stored task phase: {}", current.phase))?;
        if current_phase != next && !current_phase.can_transition_to(next) {
            bail!(
                "invalid task phase transition: {} -> {}",
                current_phase.as_str(),
                next.as_str()
            );
        }
        if let Some(read_barrier) = read_barrier {
            read_barrier.wait().await;
        }
        let terminal_generation = current.task_generation;
        let expected_phase = current.phase;
        let mut active = entities::task_run::ActiveModel {
            phase: Set(next.as_str().to_string()),
            status_message: Set(status_message),
            updated_at: Set(unix_seconds()),
            ..Default::default()
        };
        if next.is_terminal() {
            active.terminal_generation = Set(Some(terminal_generation));
        }
        let updated = entities::task_run::Entity::update_many()
            .set(active)
            .filter(entities::task_run::Column::Id.eq(task_run_id.to_string()))
            .filter(entities::task_run::Column::Phase.eq(expected_phase))
            .exec(&self.db)
            .await?;
        if updated.rows_affected != 1 {
            bail!("task phase changed concurrently");
        }
        self.read_task_run(task_run_id)
            .await?
            .context("task run disappeared after phase transition")
    }

    pub(crate) async fn compare_and_set_task_head(
        &self,
        task_run_id: &str,
        expected_head: &str,
        next_head: &str,
    ) -> Result<bool> {
        let tx = self.db.begin().await?;
        let Some(task) = entities::task_run::Entity::find_by_id(task_run_id.to_string())
            .one(&tx)
            .await?
        else {
            return Ok(false);
        };
        if task.expected_head != expected_head {
            return Ok(false);
        }
        let now = unix_seconds();
        let mut task_active: entities::task_run::ActiveModel = task.into();
        task_active.expected_head = Set(next_head.to_string());
        task_active.updated_at = Set(now);
        task_active.update(&tx).await?;

        let lease = entities::branch_lease::Entity::find()
            .filter(entities::branch_lease::Column::TaskRunId.eq(task_run_id.to_string()))
            .one(&tx)
            .await?
            .context("task branch lease not found")?;
        if lease.expected_head != expected_head {
            return Ok(false);
        }
        let mut lease_active: entities::branch_lease::ActiveModel = lease.into();
        lease_active.expected_head = Set(next_head.to_string());
        lease_active.updated_at = Set(now);
        lease_active.update(&tx).await?;
        tx.commit().await?;
        Ok(true)
    }

    pub(crate) async fn advance_task_design_head(
        &self,
        task_run_id: &str,
        expected_head: &str,
        design_commit: &str,
    ) -> Result<bool> {
        let tx = self.db.begin().await?;
        let Some(task) = entities::task_run::Entity::find_by_id(task_run_id.to_string())
            .one(&tx)
            .await?
        else {
            tx.rollback().await?;
            return Ok(false);
        };
        let phase = TaskRunPhase::from_str(&task.phase)
            .with_context(|| format!("invalid stored task phase: {}", task.phase))?;
        let next_phase = match phase {
            TaskRunPhase::DesignUpdating => TaskRunPhase::Implementing,
            TaskRunPhase::Implementing | TaskRunPhase::Reworking => phase,
            TaskRunPhase::Planning
            | TaskRunPhase::PendingConfirmation
            | TaskRunPhase::Merging
            | TaskRunPhase::Reviewing
            | TaskRunPhase::Stopping
            | TaskRunPhase::Completed
            | TaskRunPhase::Blocked
            | TaskRunPhase::Failed
            | TaskRunPhase::Cancelled => {
                tx.rollback().await?;
                bail!(
                    "task_update_design is not allowed during phase {}",
                    phase.as_str()
                );
            }
        };
        if task.expected_head != expected_head {
            tx.rollback().await?;
            return Ok(false);
        }

        let lease = entities::branch_lease::Entity::find()
            .filter(entities::branch_lease::Column::TaskRunId.eq(task_run_id.to_string()))
            .one(&tx)
            .await?
            .context("task branch lease not found")?;
        if lease.expected_head != expected_head {
            tx.rollback().await?;
            return Ok(false);
        }

        let now = unix_seconds();
        let mut task_active: entities::task_run::ActiveModel = task.into();
        task_active.expected_head = Set(design_commit.to_string());
        task_active.design_commit = Set(Some(design_commit.to_string()));
        task_active.phase = Set(next_phase.as_str().to_string());
        task_active.status_message = Set(None);
        task_active.updated_at = Set(now);
        task_active.update(&tx).await?;

        let mut lease_active: entities::branch_lease::ActiveModel = lease.into();
        lease_active.expected_head = Set(design_commit.to_string());
        lease_active.updated_at = Set(now);
        lease_active.update(&tx).await?;
        tx.commit().await?;
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) async fn release_branch_lease(&self, task_run_id: &str) -> Result<()> {
        entities::branch_lease::Entity::delete_many()
            .filter(entities::branch_lease::Column::TaskRunId.eq(task_run_id.to_string()))
            .exec(&self.db)
            .await?;
        Ok(())
    }
}

fn validate_create_task_run(input: &CreateTaskRun) -> Result<()> {
    for (label, value) in [
        ("rootThreadId", input.root_thread_id.as_str()),
        ("plan", input.plan.as_str()),
        ("workspaceRoot", input.workspace_root.as_str()),
        ("gitCommonDir", input.git_common_dir.as_str()),
        ("branch", input.branch.as_str()),
        ("headCommit", input.head_commit.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("{label} must not be empty");
        }
    }
    Ok(())
}

pub(super) fn task_run_record(model: entities::task_run::Model) -> Result<TaskRunRecord> {
    let phase = TaskRunPhase::from_str(&model.phase)
        .with_context(|| format!("invalid stored task phase: {}", model.phase))?;
    let stop_requested_origin = match model.stop_requested_origin.as_deref() {
        Some(value) => Some(
            TaskStopOrigin::from_str(value)
                .with_context(|| format!("invalid stored task stop origin: {value}"))?,
        ),
        None => None,
    };
    let task_generation = u64::try_from(model.task_generation)
        .context("stored task generation must not be negative")?;
    let terminal_generation = model
        .terminal_generation
        .map(u64::try_from)
        .transpose()
        .context("stored terminal generation must not be negative")?;
    Ok(TaskRunRecord {
        id: model.id,
        root_thread_id: model.root_thread_id,
        phase,
        plan: model.plan,
        workspace_root: model.workspace_root,
        git_common_dir: model.git_common_dir,
        branch: model.branch,
        base_commit: model.base_commit,
        expected_head: model.expected_head,
        design_commit: model.design_commit,
        status_message: model.status_message,
        stop_requested: model.stop_requested != 0,
        stop_requested_origin,
        stop_requested_reason: model.stop_requested_reason.map(TaskStopReason::from_stored),
        stop_requested_at: model.stop_requested_at,
        task_generation,
        terminal_generation,
        terminal_failure_id: model.terminal_failure_id,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

fn branch_lease_record(model: entities::branch_lease::Model) -> BranchLeaseRecord {
    BranchLeaseRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        git_common_dir: model.git_common_dir,
        branch: model.branch,
        expected_head: model.expected_head,
        acquired_at: model.acquired_at,
        updated_at: model.updated_at,
    }
}

pub(super) async fn write_task_terminal_fact(
    tx: &sea_orm::DatabaseTransaction,
    run: entities::task_run::Model,
    next_phase: TaskRunPhase,
    status_message: Option<String>,
    expected_generation: Option<u64>,
) -> Result<entities::task_run::Model> {
    if !next_phase.is_terminal() {
        bail!("task terminal fact requires a terminal phase");
    }
    let current_phase = TaskRunPhase::from_str(&run.phase)
        .with_context(|| format!("invalid stored task phase: {}", run.phase))?;
    let task_generation = u64::try_from(run.task_generation)
        .context("stored task generation must not be negative")?;
    if expected_generation.is_some_and(|expected| expected != task_generation) {
        bail!("task terminal fact belongs to another generation");
    }
    if let Some(terminal_generation) = run.terminal_generation {
        let terminal_generation = u64::try_from(terminal_generation)
            .context("stored terminal generation must not be negative")?;
        if current_phase == next_phase && terminal_generation == task_generation {
            return Ok(run);
        }
        bail!("task already has a terminal fact for another phase or generation");
    }
    if current_phase.is_terminal() {
        bail!("stored terminal task is missing its terminal generation");
    }
    if !current_phase.can_transition_to(next_phase) {
        bail!(
            "invalid task terminal transition: {} -> {}",
            current_phase.as_str(),
            next_phase.as_str()
        );
    }

    let mut active: entities::task_run::ActiveModel = run.into();
    active.phase = Set(next_phase.as_str().to_string());
    active.status_message = Set(status_message);
    active.terminal_generation = Set(Some(i64::try_from(task_generation)?));
    active.updated_at = Set(unix_seconds());
    Ok(active.update(tx).await?)
}

async fn delete_blocked_branch_lease(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
) -> Result<()> {
    entities::branch_lease::Entity::delete_many()
        .filter(entities::branch_lease::Column::TaskRunId.eq(task_run_id.to_string()))
        .exec(tx)
        .await?;
    Ok(())
}
