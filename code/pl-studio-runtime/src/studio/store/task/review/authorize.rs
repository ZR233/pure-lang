use anyhow::{Context, Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait, sea_query::Expr};

use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    ReviewRoundRecord, ReviewRoundState, ReviewScope, ReviewVerdict, TaskCommand,
    ThreadExecutionStatus, WorkUnitState,
};

use super::super::apply_task_command;
use super::super::work_unit::{update_work_unit_state, work_unit_state};
use super::helpers::{active_nonterminal_run, finish_transaction, pending_review_by_call};
use super::record::{review_round_record, review_round_state, update_review_round_state};

impl StudioStore {
    pub(crate) async fn authorize_reviewer_spawn(
        &self,
        thread_id: &str,
        requested_by_call_id: &str,
        agent_id: &str,
    ) -> Result<ReviewRoundRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_nonterminal_run(&tx, thread_id).await?;
            let round = pending_review_by_call(&tx, &run.id, requested_by_call_id).await?;
            if round.reviewer_thread_id.is_some() {
                bail!("reviewer spawn authorization is already consumed");
            }
            let next_revision = round
                .revision
                .checked_add(1)
                .context("ReviewRound revision overflow")?;
            let update = entities::review_round::Entity::update_many()
                .col_expr(
                    entities::review_round::Column::ReviewerThreadId,
                    Expr::value(Some(agent_id.to_string())),
                )
                .col_expr(
                    entities::review_round::Column::Revision,
                    Expr::value(next_revision),
                )
                .col_expr(
                    entities::review_round::Column::UpdatedAt,
                    Expr::value(unix_seconds()),
                )
                .filter(entities::review_round::Column::Id.eq(round.id.clone()))
                .filter(entities::review_round::Column::Revision.eq(round.revision))
                .exec(&tx)
                .await?;
            if update.rows_affected != 1 {
                bail!("ReviewRound authorization lost its revision CAS");
            }
            let round = entities::review_round::Entity::find_by_id(round.id)
                .one(&tx)
                .await?
                .context("ReviewRound disappeared after reviewer authorization")?;
            review_round_record(round)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn activate_reviewer(
        &self,
        review_round_id: &str,
        reviewer_thread_id: &str,
    ) -> Result<()> {
        let round = entities::review_round::Entity::find_by_id(review_round_id.to_string())
            .one(&self.db)
            .await?
            .context("review round not found")?;
        let state = review_round_state(&round)?;
        if round.reviewer_thread_id.as_deref() != Some(reviewer_thread_id)
            || state.verdict() != ReviewVerdict::Pending
            || state.reviewer_status() != ThreadExecutionStatus::Queued
        {
            bail!("reviewer activation does not match the pending review round");
        }
        let state = ReviewRoundState::running();
        update_review_round_state(&self.db, round, state).await?;
        Ok(())
    }

    pub(crate) async fn fail_reviewer_spawn(
        &self,
        thread_id: &str,
        agent_id: Option<&str>,
        requested_by_call_id: &str,
        error: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_nonterminal_run(&tx, thread_id).await?;
            let round = pending_review_by_call(&tx, &run.id, requested_by_call_id).await?;
            if let Some(agent_id) = agent_id
                && round.reviewer_thread_id.as_deref() != Some(agent_id)
            {
                bail!("review spawn failure does not match reviewer authorization");
            }
            let failed_state = ReviewRoundState::failed(error.to_string(), error.to_string());
            update_review_round_state(&tx, round.clone(), failed_state).await?;
            match ReviewScope::from_str(&round.scope) {
                Some(ReviewScope::Delivery) => {
                    let work_unit_id = round
                        .work_unit_id
                        .as_deref()
                        .context("delivery review has no work unit")?;
                    let unit = entities::work_unit::Entity::find_by_id(work_unit_id)
                        .one(&tx)
                        .await?
                        .context("delivery review work unit not found")?;
                    let state = work_unit_state(&unit)?;
                    let progress = state.into_progress();
                    update_work_unit_state(&tx, unit, WorkUnitState::ready_for_review(progress))
                        .await?;
                }
                Some(ReviewScope::Integrated) => {
                    apply_task_command(
                        &tx,
                        run,
                        TaskCommand::BeginReworking {
                            status_message: format!("reviewer spawn failed: {error}"),
                        },
                    )
                    .await?;
                }
                None => bail!("invalid stored review scope"),
            }
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }
}
