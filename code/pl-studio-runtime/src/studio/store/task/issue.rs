use anyhow::{Context, Result};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::entity as entities;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{TaskIssueRecord, TaskIssueState};

impl StudioStore {
    pub(crate) async fn list_task_issues(&self, task_run_id: &str) -> Result<Vec<TaskIssueRecord>> {
        entities::task_issue::Entity::find()
            .filter(entities::task_issue::Column::TaskRunId.eq(task_run_id))
            .order_by_asc(entities::task_issue::Column::CreatedAt)
            .all(&self.db)
            .await?
            .into_iter()
            .map(task_issue_record)
            .collect()
    }
}

fn task_issue_record(model: entities::task_issue::Model) -> Result<TaskIssueRecord> {
    let state: TaskIssueState =
        serde_json::from_str(&model.state_json).context("invalid stored TaskIssue state JSON")?;
    if state.kind().as_str() != model.state_kind {
        anyhow::bail!(
            "stored TaskIssue state discriminator mismatch: JSON is {}, generated column is {}",
            state.kind().as_str(),
            model.state_kind
        );
    }
    Ok(TaskIssueRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        source_thread_id: model.source_thread_id,
        source_turn_id: model.source_turn_id,
        source_agent_id: model.source_agent_id,
        source_role: model.source_role,
        work_unit_id: model.work_unit_id,
        review_round_id: model.review_round_id,
        state,
        revision: u64::try_from(model.revision).context("TaskIssue revision is negative")?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
