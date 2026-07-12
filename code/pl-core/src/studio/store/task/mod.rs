mod allocation;
mod completion;
mod continuation;
#[cfg(test)]
pub(crate) use continuation::ContinuationSnapshotTestBarrier;
mod delivery;
mod merge;
mod outcome;
mod recovery;
mod review;
mod terminal;
mod work_unit;

use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    BranchLeaseRecord, CreateTaskRun, TaskRunPhase, TaskRunRecord,
};

impl StudioStore {
    pub(crate) async fn create_task_run_with_lease(
        &self,
        input: CreateTaskRun,
    ) -> Result<(TaskRunRecord, BranchLeaseRecord)> {
        validate_create_task_run(&input)?;
        let Some(session) = self.read_session(&input.session_id).await? else {
            bail!("task session not found or uses a legacy mode");
        };
        if session.mode != "task" {
            bail!("task coordinator requires a task mode session");
        }

        let tx = self.db.begin().await?;
        let now = unix_seconds();
        let task_run_id = new_id("task-run");
        let task_model = entities::task_run::ActiveModel {
            id: Set(task_run_id.clone()),
            session_id: Set(input.session_id),
            phase: Set(input.phase.as_str().to_string()),
            plan: Set(input.plan),
            workspace_root: Set(input.workspace_root),
            git_common_dir: Set(input.git_common_dir.clone()),
            branch: Set(input.branch.clone()),
            base_commit: Set(input.head_commit.clone()),
            expected_head: Set(input.head_commit.clone()),
            design_commit: Set(None),
            status_message: Set(None),
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

    pub(crate) async fn read_active_task_run_for_session(
        &self,
        session_id: &str,
    ) -> Result<TaskRunRecord> {
        self.find_active_task_run_for_session(session_id)
            .await?
            .context("active task run not found for this session")
    }

    pub(crate) async fn find_latest_task_run_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<TaskRunRecord>> {
        entities::task_run::Entity::find()
            .filter(entities::task_run::Column::SessionId.eq(session_id.to_string()))
            .order_by_desc(entities::task_run::Column::CreatedAt)
            .order_by_desc(entities::task_run::Column::Id)
            .one(&self.db)
            .await?
            .map(task_run_record)
            .transpose()
    }

    pub(crate) async fn find_active_task_run_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<TaskRunRecord>> {
        let models = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::SessionId.eq(session_id.to_string()))
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
            _ => bail!("multiple active task runs found for this session"),
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
        let current = entities::task_run::Entity::find_by_id(task_run_id.to_string())
            .one(&self.db)
            .await?
            .context("task run not found")?;
        let current_phase = TaskRunPhase::from_str(&current.phase)
            .with_context(|| format!("invalid stored task phase: {}", current.phase))?;
        if !current_phase.can_transition_to(next) {
            bail!(
                "invalid task phase transition: {} -> {}",
                current_phase.as_str(),
                next.as_str()
            );
        }
        let mut active: entities::task_run::ActiveModel = current.into();
        active.phase = Set(next.as_str().to_string());
        active.status_message = Set(status_message);
        active.updated_at = Set(unix_seconds());
        task_run_record(active.update(&self.db).await?)
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
            | TaskRunPhase::ResolvingConflict
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
        ("sessionId", input.session_id.as_str()),
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
    Ok(TaskRunRecord {
        id: model.id,
        session_id: model.session_id,
        phase,
        plan: model.plan,
        workspace_root: model.workspace_root,
        git_common_dir: model.git_common_dir,
        branch: model.branch,
        base_commit: model.base_commit,
        expected_head: model.expected_head,
        design_commit: model.design_commit,
        status_message: model.status_message,
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
