#[cfg(test)]
mod allocation;
mod discard;
pub(super) mod issue;
mod merge;
mod planner_wake;
mod recovery;
mod review;
mod work_completion;
mod work_unit;

use anyhow::{Context, Result, bail};
#[cfg(test)]
use sea_orm::{ActiveModelTrait, TransactionTrait};
use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

#[cfg(test)]
use crate::StudioMode;
use crate::studio::entity as entities;
#[cfg(test)]
use crate::studio::ids::new_id;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
#[cfg(test)]
use crate::studio::task_coordinator::CreateTaskRun;
use crate::studio::task_coordinator::{
    TaskCommand, TaskContext, TaskPlan, TaskRun, TaskRunState, TaskRunStateKind,
};

impl StudioStore {
    /// 在首个计划者执行前创建任务记录。该操作不访问项目版本库，也不获取项目租约。
    #[cfg(test)]
    pub(crate) async fn create_task_run(&self, input: CreateTaskRun) -> Result<TaskRun> {
        validate_create_task_run(&input)?;
        let root_thread = self
            .read_thread(&input.root_thread_id)
            .await?
            .context("task root Thread not found")?;
        if root_thread.mode != StudioMode::Task || root_thread.parent_thread_id.is_some() {
            bail!("task coordinator requires a task mode root Thread");
        }
        if self
            .find_active_task_run_for_root_thread(&input.root_thread_id)
            .await?
            .is_some()
        {
            bail!("root Thread already owns an unfinished TaskRun");
        }

        let now = unix_seconds();
        let model = entities::task_run::ActiveModel {
            id: Set(new_id("task-run")),
            project_id: Set(input.project_id),
            root_thread_id: Set(input.root_thread_id),
            request: Set(input.request.trim().to_string()),
            plan_json: Set(None),
            workspace_root: Set(input.workspace_root),
            state_json: Set(serde_json::to_string(&TaskRunState::new())?),
            revision: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&self.db)
        .await?;
        task_run_record(model)
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
            .filter(entities::task_run::Column::StateKind.ne(TaskRunStateKind::Completed.as_str()))
            .order_by_asc(entities::task_run::Column::CreatedAt)
            .order_by_asc(entities::task_run::Column::Id)
            .all(&self.db)
            .await?;
        models.into_iter().map(task_run_record).collect()
    }

    /// 项目内最近一个已完成的其它 TaskRun（跨 Thread 冷数据，单条回源）。
    pub(crate) async fn find_latest_completed_task_for_project(
        &self,
        project_id: &str,
        excluded_run_id: &str,
    ) -> Result<Option<TaskRun>> {
        let models = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::ProjectId.eq(project_id.to_string()))
            .filter(entities::task_run::Column::Id.ne(excluded_run_id.to_string()))
            .filter(entities::task_run::Column::StateKind.eq(TaskRunStateKind::Completed.as_str()))
            .order_by_desc(entities::task_run::Column::UpdatedAt)
            .order_by_desc(entities::task_run::Column::Id)
            .all(&self.db)
            .await?;
        // 六状态只有 Completed 是终态；state_kind 冗余列精确匹配。
        models.into_iter().map(task_run_record).next().transpose()
    }

    pub(crate) async fn list_task_runs_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<TaskRun>> {
        let models = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::ProjectId.eq(project_id.to_string()))
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
            .filter(entities::task_run::Column::StateKind.ne(TaskRunStateKind::Completed.as_str()))
            .order_by_asc(entities::task_run::Column::CreatedAt)
            .order_by_asc(entities::task_run::Column::Id)
            .all(&self.db)
            .await?;
        match models.as_slice() {
            [] => Ok(None),
            [model] => task_run_record(model.clone()).map(Some),
            _ => bail!("multiple unfinished task runs found for this root Thread"),
        }
    }

    #[cfg(test)]
    pub(crate) async fn submit_task_plan(
        &self,
        root_thread_id: &str,
        content: &str,
        requested_by_call_id: &str,
        expected_revision: u64,
        expected_generation: u64,
    ) -> Result<(TaskRun, crate::InteractionRequest)> {
        let content = content.trim();
        if content.is_empty() {
            bail!("task plan must not be empty");
        }
        let tx = self.db.begin().await?;
        let model = active_task_model(&tx, root_thread_id).await?;
        let run = task_run_record(model.clone())?;
        let interaction_id = format!("plan-confirmation-{}-{requested_by_call_id}", run.id);
        if let Some(existing) = entities::interaction::Entity::find_by_id(interaction_id.clone())
            .one(&tx)
            .await?
        {
            let interaction = crate::studio::mappers::interaction_record(existing)?;
            let same_content = matches!(
                &interaction.content,
                crate::InteractionContent::PlanConfirmation(plan) if plan.content() == content
            );
            if !same_content {
                bail!("requestedByCallId is already bound to a different plan");
            }
            tx.commit().await?;
            return Ok((run, interaction));
        }
        ensure_task_version(&run, expected_revision, expected_generation)?;
        if run.kind() != TaskRunStateKind::Planning {
            bail!(
                "submitPlan requires planning; current state is {}",
                run.kind().as_str()
            );
        }
        let plan_revision = run
            .plan
            .as_ref()
            .map_or(1, |plan| plan.revision.saturating_add(1));
        let plan = TaskPlan {
            content: content.to_string(),
            revision: plan_revision,
            submitted_at: unix_seconds(),
        };
        let decision = run.decide(TaskCommand::SubmitPlan { plan_revision })?;
        let updated =
            compare_and_swap_task_run_with_plan(&tx, &model, &decision.next_state, Some(&plan))
                .await?
                .context("TaskRun plan submission lost its revision CAS")?;
        let now = unix_seconds();
        let interaction = crate::InteractionRequest::plan_confirmation(
            interaction_id.clone(),
            crate::InteractionScope {
                thread_id: root_thread_id.to_string(),
                turn_id: requested_by_call_id.to_string(),
                item_id: Some(interaction_id.clone()),
                tool_id: Some(requested_by_call_id.to_string()),
                agent_path: Some(root_thread_id.to_string()),
            },
            format!("{}:{plan_revision}", run.id),
            content,
            now,
        );
        entities::interaction::ActiveModel {
            id: Set(interaction.interaction_id.clone()),
            thread_id: Set(interaction.scope.thread_id.clone()),
            turn_id: Set(interaction.scope.turn_id.clone()),
            item_id: Set(interaction.scope.item_id.clone()),
            tool_id: Set(interaction.scope.tool_id.clone()),
            agent_path: Set(interaction.scope.agent_path.clone()),
            revision: Set(0),
            state_json: Set(serde_json::to_string(&interaction.content)?),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&tx)
        .await?;
        tx.commit().await?;
        Ok((task_run_record(updated)?, interaction))
    }

    #[cfg(test)]
    pub(crate) async fn apply_task_transition(
        &self,
        root_thread_id: &str,
        expected_revision: u64,
        expected_generation: u64,
        command: TaskCommand,
    ) -> Result<TaskRun> {
        let tx = self.db.begin().await?;
        let model = active_task_model(&tx, root_thread_id).await?;
        let run = task_run_record(model.clone())?;
        ensure_task_version(&run, expected_revision, expected_generation)?;
        let decision = run.decide(command)?;
        let updated = compare_and_swap_task_run(&tx, &model, Some(&decision.next_state))
            .await?
            .context("TaskRun transition lost its revision CAS")?;
        tx.commit().await?;
        task_run_record(updated)
    }

    #[cfg(test)]
    pub(crate) async fn resolve_task_plan_confirmation(
        &self,
        interaction_id: &str,
        resolution: crate::PlanConfirmationResolutionPayload,
    ) -> Result<(TaskRun, crate::InteractionRequest)> {
        let tx = self.db.begin().await?;
        let interaction_model =
            entities::interaction::Entity::find_by_id(interaction_id.to_string())
                .one(&tx)
                .await?
                .context("plan confirmation interaction not found")?;
        let mut interaction =
            crate::studio::mappers::interaction_record(interaction_model.clone())?;
        if interaction.status() != crate::InteractionStatus::Pending {
            let run_model = active_task_model(&tx, &interaction.scope.thread_id).await?;
            let run = task_run_record(run_model)?;
            tx.commit().await?;
            return Ok((run, interaction));
        }
        let run_model = active_task_model(&tx, &interaction.scope.thread_id).await?;
        let run = task_run_record(run_model.clone())?;
        if run.kind() != TaskRunStateKind::PendingConfirmation {
            bail!(
                "plan response requires pendingConfirmation; current state is {}",
                run.kind().as_str()
            );
        }
        let plan_revision = run
            .plan
            .as_ref()
            .context("pending confirmation TaskRun has no plan")?
            .revision;
        let task_command = match resolution.decision {
            crate::PlanConfirmationResolution::Confirm => {
                TaskCommand::ConfirmPlan { plan_revision }
            }
            crate::PlanConfirmationResolution::RevisePlan => TaskCommand::RequestPlanRevision,
        };
        let now = unix_seconds();
        let interaction_decision = interaction.decide(
            crate::InteractionCommand::ResolvePlanConfirmation(crate::ResolvePlanConfirmation {
                interaction_id: interaction.interaction_id.clone(),
                expected_revision: interaction.revision,
                operation_id: format!("resolve:{}", interaction.interaction_id),
                resolved_at: now,
                decision: resolution.decision,
                content: resolution.content,
                reason: resolution.reason,
            }),
        )?;
        interaction.apply(interaction_decision, now);
        let task_decision = run.decide(task_command)?;
        let updated_run =
            compare_and_swap_task_run(&tx, &run_model, Some(&task_decision.next_state))
                .await?
                .context("TaskRun plan response lost its revision CAS")?;
        let mut active: entities::interaction::ActiveModel = interaction_model.into();
        active.revision = Set(i64::try_from(interaction.revision)?);
        active.state_json = Set(serde_json::to_string(&interaction.content)?);
        active.updated_at = Set(interaction.updated_at);
        active.update(&tx).await?;
        tx.commit().await?;
        Ok((task_run_record(updated_run)?, interaction))
    }

    #[cfg(test)]
    pub(crate) async fn finish_task_document_editing(
        &self,
        root_thread_id: &str,
        expected_revision: u64,
        expected_generation: u64,
        summary: &str,
    ) -> Result<TaskRun> {
        self.apply_task_transition(
            root_thread_id,
            expected_revision,
            expected_generation,
            TaskCommand::FinishDocumentEditing {
                summary: summary.to_string(),
            },
        )
        .await
    }
}

#[cfg(test)]
fn validate_create_task_run(input: &CreateTaskRun) -> Result<()> {
    for (label, value) in [
        ("projectId", input.project_id.as_str()),
        ("rootThreadId", input.root_thread_id.as_str()),
        ("request", input.request.as_str()),
        ("workspaceRoot", input.workspace_root.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("{label} must not be empty");
        }
    }
    Ok(())
}

#[cfg(test)]
fn ensure_task_version(run: &TaskRun, revision: u64, generation: u64) -> Result<()> {
    if run.revision != revision {
        bail!(
            "task revision conflict: expected {revision}, actual {}",
            run.revision
        );
    }
    if run.generation() != generation {
        bail!(
            "task generation conflict: expected {generation}, actual {}",
            run.generation()
        );
    }
    Ok(())
}

#[cfg(test)]
async fn active_task_model<C>(
    connection: &C,
    root_thread_id: &str,
) -> Result<entities::task_run::Model>
where
    C: sea_orm::ConnectionTrait,
{
    let models = entities::task_run::Entity::find()
        .filter(entities::task_run::Column::RootThreadId.eq(root_thread_id.to_string()))
        .filter(entities::task_run::Column::StateKind.ne(TaskRunStateKind::Completed.as_str()))
        .all(connection)
        .await?;
    match models.as_slice() {
        [model] => Ok(model.clone()),
        [] => bail!("active TaskRun not found"),
        _ => bail!("multiple unfinished TaskRuns found for root Thread"),
    }
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
    let plan = model
        .plan_json
        .as_deref()
        .map(serde_json::from_str::<TaskPlan>)
        .transpose()
        .context("invalid stored task plan JSON")?;
    Ok(TaskRun {
        context: TaskContext {
            id: model.id,
            project_id: model.project_id,
            root_thread_id: model.root_thread_id,
            request: model.request,
            plan,
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

#[cfg(test)]
async fn compare_and_swap_task_run_with_plan<C>(
    connection: &C,
    model: &entities::task_run::Model,
    next_state: &TaskRunState,
    plan: Option<&TaskPlan>,
) -> Result<Option<entities::task_run::Model>>
where
    C: sea_orm::ConnectionTrait,
{
    let next_revision = model
        .revision
        .checked_add(1)
        .context("task revision overflow")?;
    let mut active = entities::task_run::ActiveModel {
        state_json: Set(serde_json::to_string(next_state)?),
        revision: Set(next_revision),
        updated_at: Set(unix_seconds()),
        ..Default::default()
    };
    if let Some(plan) = plan {
        active.plan_json = Set(Some(serde_json::to_string(plan)?));
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
