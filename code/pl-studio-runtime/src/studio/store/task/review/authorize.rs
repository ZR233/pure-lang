use anyhow::{Context, Result, bail};
use sea_orm::{EntityTrait, TransactionTrait};

use crate::studio::entity as entities;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    ReviewRoundCommand, ReviewRoundRecord, ReviewScope, TaskCommand, WorkUnitCommand,
};

use super::super::apply_task_command;
use super::super::work_unit::apply_work_unit_command;
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
            let record = review_round_record(round.clone())?;
            if record.kind()
                != crate::studio::task_coordinator::ReviewRoundStateKind::PendingDispatch
            {
                bail!("reviewer spawn authorization is already consumed");
            }
            let decision = record.decide(
                record.revision,
                ReviewRoundCommand::Dispatch {
                    reviewer_thread_id: agent_id.to_string(),
                },
            )?;
            if !decision.changed() {
                return Ok(record);
            }
            let next = decision.next_state();
            let round = update_review_round_state(&tx, round, next).await?;
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
        let next = state
            .decide(
                &round.id,
                ReviewRoundCommand::Start {
                    reviewer_thread_id: reviewer_thread_id.to_string(),
                },
            )?
            .next_state();
        update_review_round_state(&self.db, round, next).await?;
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
            let state = review_round_state(&round)?;
            let failed_state = state
                .decide(
                    &round.id,
                    ReviewRoundCommand::Fail {
                        reviewer_thread_id: agent_id.map(str::to_string),
                        error: error.to_string(),
                        summary: error.to_string(),
                    },
                )?
                .next_state();
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
                    apply_work_unit_command(
                        &tx,
                        unit,
                        WorkUnitCommand::ReviewFailed {
                            review_round_id: round.id.clone(),
                        },
                    )
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
