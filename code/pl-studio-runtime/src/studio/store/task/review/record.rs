use anyhow::{Context, Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, sea_query::Expr};

use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::task_coordinator::{
    ReviewRoundRecord, ReviewRoundState, ReviewScope, decode_review_round_state,
};

pub(in crate::studio::store::task) fn review_round_record(
    model: entities::review_round::Model,
) -> Result<ReviewRoundRecord> {
    let state = review_round_state(&model)?;
    Ok(ReviewRoundRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        round: u32::try_from(model.round).context("review round must be positive")?,
        scope: ReviewScope::from_str(&model.scope)
            .with_context(|| format!("invalid review scope: {}", model.scope))?,
        work_unit_id: model.work_unit_id,
        completion_id: model.completion_id,
        completion_revision: model
            .completion_revision
            .map(u32::try_from)
            .transpose()
            .context("completion revision must be positive")?,
        reviewed_head: model.reviewed_head,
        requested_by_call_id: model.requested_by_call_id,
        state,
        design_references: serde_json::from_str(&model.design_references_json)?,
        findings: serde_json::from_str(&model.findings_json)?,
        file_reviews: model
            .file_reviews_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?,
        revision: u64::try_from(model.revision).context("review round revision is negative")?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

pub(in crate::studio::store::task) fn review_round_state(
    model: &entities::review_round::Model,
) -> Result<ReviewRoundState> {
    let state = decode_review_round_state(&model.state_json)?;
    if state.kind().as_str() != model.state_kind {
        bail!(
            "stored ReviewRound state discriminator mismatch: JSON is {}, generated column is {}",
            state.kind().as_str(),
            model.state_kind
        );
    }
    if state.reviewer_thread_id() != model.reviewer_thread_id.as_deref() {
        bail!("stored ReviewRound reviewer association disagrees with canonical state");
    }
    Ok(state)
}

pub(in crate::studio::store::task) async fn update_review_round_state<C>(
    connection: &C,
    model: entities::review_round::Model,
    state: ReviewRoundState,
) -> Result<entities::review_round::Model>
where
    C: sea_orm::ConnectionTrait,
{
    let next_revision = model
        .revision
        .checked_add(1)
        .context("ReviewRound revision overflow")?;
    let result = entities::review_round::Entity::update_many()
        .col_expr(
            entities::review_round::Column::StateJson,
            Expr::value(serde_json::to_string(&state)?),
        )
        .col_expr(
            entities::review_round::Column::ReviewerThreadId,
            Expr::value(state.reviewer_thread_id().map(str::to_string)),
        )
        .col_expr(
            entities::review_round::Column::Revision,
            Expr::value(next_revision),
        )
        .col_expr(
            entities::review_round::Column::UpdatedAt,
            Expr::value(unix_seconds()),
        )
        .filter(entities::review_round::Column::Id.eq(model.id.clone()))
        .filter(entities::review_round::Column::Revision.eq(model.revision))
        .exec(connection)
        .await?;
    if result.rows_affected != 1 {
        bail!("ReviewRound state update lost its revision CAS");
    }
    entities::review_round::Entity::find_by_id(model.id)
        .one(connection)
        .await?
        .context("ReviewRound disappeared after state update")
}
