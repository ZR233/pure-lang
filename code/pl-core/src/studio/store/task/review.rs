use anyhow::{Context, Result};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    CompleteReviewRound, CreateReviewRound, ReviewRoundRecord, ReviewVerdict,
};

impl StudioStore {
    pub(crate) async fn create_review_round(
        &self,
        input: CreateReviewRound,
    ) -> Result<ReviewRoundRecord> {
        let now = unix_seconds();
        review_round_record(
            entities::review_round::ActiveModel {
                id: Set(new_id("review")),
                task_run_id: Set(input.task_run_id),
                round: Set(input.round as i32),
                head_commit: Set(input.head_commit),
                status: Set(ReviewVerdict::Pending.as_str().to_string()),
                reviewer_agent_id: Set(input.reviewer_agent_id),
                summary: Set(None),
                design_references_json: Set("[]".to_string()),
                findings_json: Set("[]".to_string()),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&self.db)
            .await?,
        )
    }

    pub(crate) async fn update_review_round(
        &self,
        review_id: &str,
        update: CompleteReviewRound,
    ) -> Result<ReviewRoundRecord> {
        let model = entities::review_round::Entity::find_by_id(review_id.to_string())
            .one(&self.db)
            .await?
            .context("review round not found")?;
        let mut active: entities::review_round::ActiveModel = model.into();
        active.status = Set(update.verdict.as_str().to_string());
        active.summary = Set(Some(update.summary));
        active.design_references_json = Set(serde_json::to_string(&update.design_references)?);
        active.findings_json = Set(serde_json::to_string(&update.findings)?);
        active.updated_at = Set(unix_seconds());
        review_round_record(active.update(&self.db).await?)
    }

    pub(crate) async fn list_review_rounds(
        &self,
        task_run_id: &str,
    ) -> Result<Vec<ReviewRoundRecord>> {
        entities::review_round::Entity::find()
            .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
            .order_by_asc(entities::review_round::Column::Round)
            .all(&self.db)
            .await?
            .into_iter()
            .map(review_round_record)
            .collect()
    }
}

pub(super) fn review_round_record(
    model: entities::review_round::Model,
) -> Result<ReviewRoundRecord> {
    Ok(ReviewRoundRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        round: model.round as u32,
        head_commit: model.head_commit,
        verdict: ReviewVerdict::from_str(&model.status)
            .with_context(|| format!("invalid review verdict: {}", model.status))?,
        reviewer_agent_id: model.reviewer_agent_id,
        summary: model.summary,
        design_references: serde_json::from_str(&model.design_references_json)?,
        findings: serde_json::from_str(&model.findings_json)?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
