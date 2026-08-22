use anyhow::{Result, bail};
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

    pub(crate) async fn find_review_round_for_reviewer(
        &self,
        reviewer_agent_id: &str,
    ) -> Result<Option<ReviewRoundRecord>> {
        let rounds = entities::review_round::Entity::find()
            .filter(
                entities::review_round::Column::ReviewerThreadId.eq(reviewer_agent_id.to_string()),
            )
            .all(&self.db)
            .await?;
        match rounds.as_slice() {
            [] => Ok(None),
            [round] => review_round_record(round.clone()).map(Some),
            _ => bail!("reviewer Thread owns multiple review rounds"),
        }
    }
}
