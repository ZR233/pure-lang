use anyhow::{Context, Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::entity as entities;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    WorkCompletionContent, WorkCompletionRecord, WorkCompletionState,
};

impl StudioStore {
    pub(crate) async fn list_work_completions(
        &self,
        task_run_id: &str,
    ) -> Result<Vec<WorkCompletionRecord>> {
        entities::work_completion::Entity::find()
            .filter(entities::work_completion::Column::TaskRunId.eq(task_run_id.to_string()))
            .order_by_asc(entities::work_completion::Column::CreatedAt)
            .order_by_asc(entities::work_completion::Column::Id)
            .all(&self.db)
            .await?
            .into_iter()
            .map(work_completion_record)
            .collect()
    }
}

pub(super) fn work_completion_record(
    model: entities::work_completion::Model,
) -> Result<WorkCompletionRecord> {
    let content: WorkCompletionContent = serde_json::from_str(&model.content_json)
        .context("invalid stored WorkCompletion content JSON")?;
    if content.kind().as_str() != model.content_kind {
        bail!("stored WorkCompletion content discriminator mismatch");
    }
    let state: WorkCompletionState = serde_json::from_str(&model.state_json)
        .context("invalid stored WorkCompletion state JSON")?;
    if state.status().as_str() != model.state_kind {
        bail!("stored WorkCompletion state discriminator mismatch");
    }
    Ok(WorkCompletionRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        work_unit_id: model.work_unit_id,
        executor_agent_id: model.executor_agent_id,
        revision: u32::try_from(model.revision).context("completion revision must be positive")?,
        content,
        state,
        state_revision: u64::try_from(model.state_revision)
            .context("WorkCompletion state revision is negative")?,
        base_commit: model.base_commit,
        verification_summary: model.verification_summary,
        worktree_path: model.worktree_path,
        branch: model.branch,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
