use anyhow::Result;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};

use crate::studio::entity as entities;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::ReviewRoundRecord;

use super::record::review_round_record;

impl StudioStore {
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
