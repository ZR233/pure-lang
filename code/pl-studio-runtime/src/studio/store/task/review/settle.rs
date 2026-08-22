use anyhow::{Context, Result, bail};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait, sea_query::Expr};

use pl_protocol::TurnOutcome;

use crate::studio::entity as entities;
use crate::studio::ids::unix_seconds;
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentReview, ReviewExitDiagnostics, ReviewFileCoverage, ReviewPassedOutcome,
    ReviewRoundCommand, ReviewRoundRecord, ReviewRoundStateKind, ReviewScope, ReviewTarget,
    ReviewVerdict, TaskCommand, TaskRunState, TaskRunStateKind, WaitingReviewPhase,
    WorkCompletionCommand, WorkCompletionKind, WorkCompletionStatus, WorkUnitCommand,
    WorkUnitStateKind,
};

use super::super::apply_task_command;
use super::super::task_run_record;
use super::super::work_completion::work_completion_record;
use super::super::work_unit::{apply_work_unit_command, work_unit_record};
use super::helpers::{active_nonterminal_run, finish_transaction, pending_review_for_reviewer};
use super::record::{review_round_record, review_round_state, update_review_round_state};

impl StudioStore {
    pub(crate) async fn complete_task_review(
        &self,
        thread_id: &str,
        reviewer_agent_id: &str,
        review: AgentReview,
        file_reviews: ReviewFileCoverage,
    ) -> Result<ReviewRoundRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_nonterminal_run(&tx, thread_id).await?;
            let round = pending_review_for_reviewer(&tx, &run.id, reviewer_agent_id).await?;
            if review_round_state(&round)?.kind() != ReviewRoundStateKind::Running
                || review_round_state(&round)?.reviewer_thread_id() != Some(reviewer_agent_id)
            {
                bail!("reviewer Thread does not match the pending review");
            }
            let file_reviews_json =
                prepare_file_reviews(&round, file_reviews, ReviewCoverageSubmission::Accepted)?;
            let now = unix_seconds();
            match ReviewScope::from_str(&round.scope).context("invalid stored review scope")? {
                ReviewScope::Delivery => {
                    complete_delivery_review(&tx, &run, &round, &review, now).await?;
                }
                ReviewScope::Integrated => {
                    let task = task_run_record(run.clone())?;
                    let target_matches = matches!(
                        &task.state,
                        TaskRunState::Reviewing(state)
                            if matches!(
                                state.target(),
                                ReviewTarget::Integration { reviewed_head }
                                    if reviewed_head == &round.reviewed_head
                            )
                    );
                    if !target_matches {
                        bail!("integrated review no longer matches the durable Task target");
                    }
                    match review.verdict {
                        ReviewVerdict::Pass => {}
                        ReviewVerdict::ChangesRequired | ReviewVerdict::Blocked => {
                            apply_task_command(
                                &tx,
                                run.clone(),
                                TaskCommand::BeginReworking {
                                    status_message: review.summary.clone(),
                                },
                            )
                            .await?;
                        }
                        ReviewVerdict::Pending | ReviewVerdict::Failed => {
                            bail!("reviewer cannot select pending or failed")
                        }
                    }
                }
            }
            let command = match review.verdict {
                ReviewVerdict::Pass => ReviewRoundCommand::Pass {
                    reviewer_thread_id: reviewer_agent_id.to_string(),
                    summary: review.summary.clone(),
                },
                ReviewVerdict::ChangesRequired => ReviewRoundCommand::RequireChanges {
                    reviewer_thread_id: reviewer_agent_id.to_string(),
                    summary: review.summary.clone(),
                },
                ReviewVerdict::Blocked => ReviewRoundCommand::Block {
                    reviewer_thread_id: reviewer_agent_id.to_string(),
                    summary: review.summary.clone(),
                },
                ReviewVerdict::Pending | ReviewVerdict::Failed => {
                    bail!("reviewer cannot select pending or failed")
                }
            };
            let next_state = review_round_state(&round)?
                .decide(&round.id, command)?
                .next_state();
            let next_revision = round.revision.saturating_add(1);
            let updated_round = entities::review_round::Entity::update_many()
                .col_expr(
                    entities::review_round::Column::StateJson,
                    Expr::value(serde_json::to_string(&next_state)?),
                )
                .col_expr(
                    entities::review_round::Column::DesignReferencesJson,
                    Expr::value(serde_json::to_string(&review.design_references)?),
                )
                .col_expr(
                    entities::review_round::Column::FindingsJson,
                    Expr::value(serde_json::to_string(&review.findings)?),
                )
                .col_expr(
                    entities::review_round::Column::FileReviewsJson,
                    Expr::value(Some(file_reviews_json)),
                )
                .col_expr(entities::review_round::Column::UpdatedAt, Expr::value(now))
                .col_expr(
                    entities::review_round::Column::Revision,
                    Expr::value(next_revision),
                )
                .filter(entities::review_round::Column::Id.eq(round.id.clone()))
                .filter(entities::review_round::Column::Revision.eq(round.revision))
                .exec(&tx)
                .await?;
            if updated_round.rows_affected != 1 {
                bail!("review result is stale or was already settled");
            }
            let round = entities::review_round::Entity::find_by_id(round.id)
                .one(&tx)
                .await?
                .context("completed review round disappeared")?;
            review_round_record(round)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn record_review_rejection(
        &self,
        thread_id: &str,
        reviewer_agent_id: &str,
        file_reviews: ReviewFileCoverage,
    ) -> Result<ReviewRoundRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_nonterminal_run(&tx, thread_id).await?;
            let round = pending_review_for_reviewer(&tx, &run.id, reviewer_agent_id).await?;
            if review_round_state(&round)?.kind() != ReviewRoundStateKind::Running {
                bail!("reviewer Thread does not match the pending review");
            }
            let file_reviews_json =
                prepare_file_reviews(&round, file_reviews, ReviewCoverageSubmission::Rejected)?;
            let updated = entities::review_round::Entity::update_many()
                .col_expr(
                    entities::review_round::Column::FileReviewsJson,
                    Expr::value(Some(file_reviews_json)),
                )
                .col_expr(
                    entities::review_round::Column::UpdatedAt,
                    Expr::value(unix_seconds()),
                )
                .col_expr(
                    entities::review_round::Column::Revision,
                    Expr::value(round.revision.saturating_add(1)),
                )
                .filter(entities::review_round::Column::Id.eq(round.id.clone()))
                .filter(entities::review_round::Column::Revision.eq(round.revision))
                .exec(&tx)
                .await?;
            if updated.rows_affected != 1 {
                bail!("review rejection is stale or was already settled");
            }
            let round = entities::review_round::Entity::find_by_id(round.id)
                .one(&tx)
                .await?
                .context("rejected review round disappeared")?;
            review_round_record(round)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn settle_reviewer_turn_finished(
        &self,
        reviewer_agent_id: &str,
        outcome: &TurnOutcome,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let rounds = entities::review_round::Entity::find()
                .filter(
                    entities::review_round::Column::ReviewerThreadId
                        .eq(reviewer_agent_id.to_string()),
                )
                .filter(entities::review_round::Column::StateKind.is_in([
                    ReviewRoundStateKind::PendingDispatch.as_str(),
                    ReviewRoundStateKind::Dispatched.as_str(),
                    ReviewRoundStateKind::Running.as_str(),
                ]))
                .all(&tx)
                .await?;
            let round = match rounds.as_slice() {
                [] => return Ok(()),
                [round] => round.clone(),
                _ => bail!("reviewer owns multiple pending review rounds"),
            };
            let detail = reviewer_outcome_detail(outcome);
            let state = review_round_state(&round)?;
            let command = match outcome {
                TurnOutcome::Cancelled(_) => ReviewRoundCommand::Cancel {
                    reviewer_thread_id: Some(reviewer_agent_id.to_string()),
                    reason: detail.clone(),
                    summary: detail.clone(),
                },
                TurnOutcome::Completed(_)
                | TurnOutcome::Failed(_)
                | TurnOutcome::BudgetLimited(_) => ReviewRoundCommand::Fail {
                    reviewer_thread_id: Some(reviewer_agent_id.to_string()),
                    error: detail.clone(),
                    summary: detail.clone(),
                },
            };
            let failed_state = state.decide(&round.id, command)?.next_state();
            update_review_round_state(&tx, round.clone(), failed_state).await?;

            match ReviewScope::from_str(&round.scope).context("invalid stored review scope")? {
                ReviewScope::Delivery => {
                    let work_unit_id = round
                        .work_unit_id
                        .as_deref()
                        .context("delivery review has no work unit")?;
                    let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id)
                        .one(&tx)
                        .await?
                        .context("delivery review work unit not found")?;
                    if work_unit.state_kind == WorkUnitStateKind::WaitingReview.as_str() {
                        apply_work_unit_command(
                            &tx,
                            work_unit,
                            WorkUnitCommand::ReviewFailed {
                                review_round_id: round.id.clone(),
                            },
                        )
                        .await?;
                    }
                }
                ReviewScope::Integrated => {
                    let run = entities::task_run::Entity::find_by_id(round.task_run_id)
                        .one(&tx)
                        .await?
                        .context("integrated review task run not found")?;
                    if task_run_record(run.clone())?.kind() == TaskRunStateKind::Reviewing {
                        apply_task_command(
                            &tx,
                            run,
                            TaskCommand::BeginReworking {
                                status_message: detail,
                            },
                        )
                        .await?;
                    }
                }
            }
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }
}

fn reviewer_outcome_detail(outcome: &TurnOutcome) -> String {
    match outcome {
        TurnOutcome::Completed(_) => "reviewer ended without a successful review_exit".to_string(),
        TurnOutcome::Cancelled(value) => format!("reviewer cancelled: {:?}", value.cause()),
        TurnOutcome::Failed(value) => value.failure().message.clone(),
        TurnOutcome::BudgetLimited(_) => "reviewer stopped at its budget limit".to_string(),
    }
}

async fn complete_delivery_review(
    tx: &sea_orm::DatabaseTransaction,
    run: &entities::task_run::Model,
    round: &entities::review_round::Model,
    review: &AgentReview,
    now: i64,
) -> Result<()> {
    let work_unit_id = round
        .work_unit_id
        .as_deref()
        .context("delivery review has no work unit")?;
    let completion_id = round
        .completion_id
        .as_deref()
        .context("delivery review has no completion")?;
    let completion_revision = round
        .completion_revision
        .context("delivery review has no completion revision")?;
    let work_unit = entities::work_unit::Entity::find_by_id(work_unit_id)
        .one(tx)
        .await?
        .context("delivery review work unit not found")?;
    let completion = entities::work_completion::Entity::find_by_id(completion_id)
        .one(tx)
        .await?
        .context("delivery review completion not found")?;
    let completion_record = work_completion_record(completion.clone())?;
    let reviewed_head = completion_record
        .head_commit()
        .unwrap_or(work_unit.base_commit.as_str());
    let work_unit_record = work_unit_record(work_unit.clone())?;
    if !matches!(
        work_unit_record.waiting_review_phase(),
        Some(WaitingReviewPhase::Reviewing(_))
    ) || completion.work_unit_id != work_unit.id
        || completion.revision != completion_revision
        || completion_record.status() != WorkCompletionStatus::ReadyForReview
        || round.reviewed_head != reviewed_head
    {
        bail!("delivery review target changed after reviewer creation");
    }
    let (completion_command, completion_status, work_unit_command) = match review.verdict {
        ReviewVerdict::Pass => {
            let outcome = if completion_record.kind() == WorkCompletionKind::NoDelivery {
                ReviewPassedOutcome::NoDelivery
            } else {
                ReviewPassedOutcome::Delivery
            };
            (
                WorkCompletionCommand::Approve {
                    review_round_id: round.id.clone(),
                    decided_at: now,
                },
                WorkCompletionStatus::Approved,
                WorkUnitCommand::PassReview {
                    review_round_id: round.id.clone(),
                    outcome,
                },
            )
        }
        ReviewVerdict::ChangesRequired | ReviewVerdict::Blocked => (
            WorkCompletionCommand::RequireChanges {
                review_round_id: round.id.clone(),
                decided_at: now,
            },
            WorkCompletionStatus::ChangesRequired,
            WorkUnitCommand::RequireChanges {
                review_round_id: round.id.clone(),
            },
        ),
        ReviewVerdict::Pending | ReviewVerdict::Failed => {
            bail!("reviewer cannot select pending or failed")
        }
    };
    let decision =
        completion_record.decide(completion_record.state_revision, completion_command)?;
    if !decision.changed() {
        bail!("delivery review completion was already settled");
    }
    let next_completion_state = decision.next_state();
    let updated_completion = entities::work_completion::Entity::update_many()
        .col_expr(
            entities::work_completion::Column::StateJson,
            Expr::value(serde_json::to_string(&next_completion_state)?),
        )
        .col_expr(
            entities::work_completion::Column::StateRevision,
            Expr::value(i64::try_from(completion_record.state_revision + 1)?),
        )
        .col_expr(
            entities::work_completion::Column::UpdatedAt,
            Expr::value(now),
        )
        .filter(entities::work_completion::Column::Id.eq(completion.id.clone()))
        .filter(entities::work_completion::Column::Revision.eq(completion_revision))
        .filter(
            entities::work_completion::Column::StateRevision
                .eq(i64::try_from(completion_record.state_revision)?),
        )
        .filter(
            entities::work_completion::Column::StateKind
                .eq(WorkCompletionStatus::ReadyForReview.as_str()),
        )
        .exec(tx)
        .await?;
    if updated_completion.rows_affected != 1 {
        bail!("delivery review completion is stale or was already settled");
    }
    apply_work_unit_command(tx, work_unit, work_unit_command).await?;
    if completion_status == WorkCompletionStatus::Approved
        && completion_record.kind() == WorkCompletionKind::Delivery
    {
        apply_task_command(
            tx,
            run.clone(),
            TaskCommand::BeginMerging {
                status_message: Some(format!(
                    "approved Completion {} is ready for planner Git integration",
                    completion.id
                )),
            },
        )
        .await?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ReviewCoverageSubmission {
    Rejected,
    Accepted,
}

fn prepare_file_reviews(
    round: &entities::review_round::Model,
    mut submitted: ReviewFileCoverage,
    outcome: ReviewCoverageSubmission,
) -> Result<String> {
    let stored: ReviewFileCoverage = serde_json::from_str(
        round
            .file_reviews_json
            .as_deref()
            .context("review round has no file coverage snapshot")?,
    )?;
    if submitted.version != stored.version || submitted.expected_paths() != stored.expected_paths()
    {
        bail!("review file coverage no longer matches the frozen round target");
    }
    match outcome {
        ReviewCoverageSubmission::Rejected
            if submitted
                .last_diagnostics
                .as_ref()
                .is_none_or(ReviewExitDiagnostics::is_empty) =>
        {
            bail!("rejected review coverage must contain diagnostics");
        }
        ReviewCoverageSubmission::Accepted
            if !submitted.is_complete()
                || submitted
                    .last_diagnostics
                    .as_ref()
                    .is_some_and(|diagnostics| !diagnostics.is_empty()) =>
        {
            bail!("accepted review coverage must be complete and have no diagnostics");
        }
        ReviewCoverageSubmission::Rejected | ReviewCoverageSubmission::Accepted => {}
    }
    submitted.diagnostics_revision = stored
        .diagnostics_revision
        .checked_add(1)
        .context("review diagnostics revision overflow")?;
    serde_json::to_string(&submitted).map_err(Into::into)
}
