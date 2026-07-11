use anyhow::{Context, Result};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    CreateMergeRecord, MergeRecord, MergeStatus, UpdateMergeRecord,
};

impl StudioStore {
    pub(crate) async fn create_merge_record(
        &self,
        input: CreateMergeRecord,
    ) -> Result<MergeRecord> {
        let now = unix_seconds();
        merge_record(
            entities::merge_record::ActiveModel {
                id: Set(new_id("merge")),
                task_run_id: Set(input.task_run_id),
                agent_id: Set(input.agent_id),
                status: Set(MergeStatus::Pending.as_str().to_string()),
                expected_head: Set(input.expected_head),
                source_commit: Set(input.source_commit),
                conflict_files_json: Set(serde_json::to_string(&input.conflict_files)?),
                resolution_summary: Set(None),
                verification_json: Set(None),
                attempt: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&self.db)
            .await?,
        )
    }

    pub(crate) async fn update_merge_record(
        &self,
        merge_id: &str,
        update: UpdateMergeRecord,
    ) -> Result<MergeRecord> {
        let model = entities::merge_record::Entity::find_by_id(merge_id.to_string())
            .one(&self.db)
            .await?
            .context("merge record not found")?;
        let mut active: entities::merge_record::ActiveModel = model.into();
        active.status = Set(update.status.as_str().to_string());
        active.resolution_summary = Set(update.resolution_summary);
        active.verification_json = Set(update
            .verification
            .map(|value| serde_json::to_string(&value))
            .transpose()?);
        active.attempt = Set(update.attempt as i32);
        active.updated_at = Set(unix_seconds());
        merge_record(active.update(&self.db).await?)
    }

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

pub(super) fn merge_record(model: entities::merge_record::Model) -> Result<MergeRecord> {
    Ok(MergeRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        agent_id: model.agent_id,
        status: MergeStatus::from_str(&model.status)
            .with_context(|| format!("invalid merge status: {}", model.status))?,
        expected_head: model.expected_head,
        source_commit: model.source_commit,
        conflict_files: serde_json::from_str(&model.conflict_files_json)?,
        resolution_summary: model.resolution_summary,
        verification: model
            .verification_json
            .map(|json| serde_json::from_str(&json))
            .transpose()?,
        attempt: model.attempt as u32,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
