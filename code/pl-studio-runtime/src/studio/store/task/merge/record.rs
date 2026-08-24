use anyhow::{Context, Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::entity as entities;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{MergeCleanupState, MergeMethod, MergeRecord};

impl StudioStore {
    pub(crate) async fn list_merge_records(&self, task_run_id: &str) -> Result<Vec<MergeRecord>> {
        entities::merge_record::Entity::find()
            .filter(entities::merge_record::Column::TaskRunId.eq(task_run_id.to_string()))
            .order_by_asc(entities::merge_record::Column::CreatedAt)
            .order_by_asc(entities::merge_record::Column::Id)
            .all(&self.db)
            .await?
            .into_iter()
            .map(merge_record)
            .collect()
    }
}

pub(crate) fn merge_record(model: entities::merge_record::Model) -> Result<MergeRecord> {
    let cleanup: MergeCleanupState = serde_json::from_str(&model.cleanup_state_json)
        .context("invalid stored merge cleanup state JSON")?;
    if cleanup.kind().as_str() != model.cleanup_state_kind {
        bail!(
            "stored merge cleanup discriminator mismatch: JSON is {}, generated column is {}",
            cleanup.kind().as_str(),
            model.cleanup_state_kind
        );
    }
    Ok(MergeRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        work_unit_id: model.work_unit_id,
        completion_id: model.completion_id,
        completion_revision: u32::try_from(model.completion_revision)?,
        executor_agent_id: model.executor_agent_id,
        expected_previous_head: model.expected_previous_head,
        resulting_head: model.resulting_head,
        delivery_head: model.delivery_head,
        method: MergeMethod::from_str(&model.method)
            .with_context(|| format!("invalid merge method: {}", model.method))?,
        summary: model.summary,
        cleanup,
        revision: u64::try_from(model.revision).context("merge revision is negative")?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
