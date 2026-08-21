use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, TransactionTrait, sea_query::Expr,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentReview, BeginIntegratedReview, ReviewExitDiagnostics, ReviewFileCoverage,
    ReviewRoundRecord, ReviewRoundState, ReviewScope, ReviewTarget, ReviewVerdict, TaskCommand,
    TaskRunStateKind, ThreadExecutionStatus, WorkCompletionKind, WorkCompletionStatus,
    WorkUnitStatus, decode_review_round_state,
};
use pl_core::TurnOutcomeKind;

use super::work_unit::{update_work_unit_state, work_unit_state};

impl StudioStore {
    pub(crate) async fn begin_delivery_review(
        &self,
        thread_id: &str,
        executor_agent_id: &str,
        requested_by_call_id: &str,
    ) -> Result<ReviewRoundRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_implementation_run(&tx, thread_id).await?;
            ensure_review_call_unused(&tx, &run.id, requested_by_call_id).await?;
            let units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
                .filter(
                    entities::work_unit::Column::ExecutorThreadId.eq(executor_agent_id.to_string()),
                )
                .all(&tx)
                .await?;
            let work_unit = match units.as_slice() {
                [unit] if unit.state_kind == WorkUnitStatus::ReadyForReview.as_str() => {
                    unit.clone()
                }
                [unit] => bail!(
                    "executor work unit is {}, not readyForReview",
                    unit.state_kind
                ),
                [] => bail!("executor work unit not found"),
                _ => bail!("executor owns multiple work units"),
            };
            let completion = entities::work_completion::Entity::find()
                .filter(entities::work_completion::Column::WorkUnitId.eq(work_unit.id.clone()))
                .order_by_desc(entities::work_completion::Column::Revision)
                .one(&tx)
                .await?
                .context("work unit has no completion")?;
            if completion.executor_agent_id != executor_agent_id
                || completion.status != WorkCompletionStatus::ReadyForReview.as_str()
            {
                bail!("latest completion is not ready for review");
            }
            let changed_files = serde_json::from_str(&completion.changed_files_json)?;
            ensure_no_pending_delivery_review(&tx, &run.id, &work_unit.id).await?;
            let round = insert_review_round(
                &tx,
                NewReviewRound {
                    task_run_id: &run.id,
                    target: NewReviewTarget::Delivery {
                        work_unit_id: &work_unit.id,
                        completion_id: &completion.id,
                        completion_revision: completion.revision,
                        reviewed_head: completion
                            .head_commit
                            .as_deref()
                            .unwrap_or(work_unit.base_commit.as_str()),
                    },
                    requested_by_call_id,
                    changed_files,
                },
            )
            .await?;
            let state = work_unit_state(&work_unit)?;
            let execution = state.execution_status();
            let progress = state.into_progress();
            update_work_unit_state(
                &tx,
                work_unit,
                WorkUnitStatus::Reviewing,
                execution,
                progress,
            )
            .await?;
            Ok(round)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn begin_integrated_review(
        &self,
        thread_id: &str,
        request: BeginIntegratedReview,
    ) -> Result<ReviewRoundRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_implementation_run(&tx, thread_id).await?;
            if run.expected_head != request.reviewed_head {
                bail!("integrated review target changed before round creation");
            }
            ensure_review_call_unused(&tx, &run.id, &request.requested_by_call_id).await?;
            let work_units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
                .all(&tx)
                .await?;
            if work_units.iter().any(|unit| {
                !matches!(
                    WorkUnitStatus::from_str(&unit.state_kind),
                    Some(WorkUnitStatus::Merged | WorkUnitStatus::NoDelivery)
                )
            }) {
                bail!("integrated review requires every work unit to be merged or noDelivery");
            }
            ensure_no_pending_review(&tx, &run.id).await?;
            let round = insert_review_round(
                &tx,
                NewReviewRound {
                    task_run_id: &run.id,
                    target: NewReviewTarget::Integrated {
                        reviewed_head: &request.reviewed_head,
                    },
                    requested_by_call_id: &request.requested_by_call_id,
                    changed_files: request.changed_files,
                },
            )
            .await?;
            super::apply_task_command(
                &tx,
                run,
                TaskCommand::BeginReviewing(ReviewTarget::Integration {
                    reviewed_head: request.reviewed_head,
                }),
            )
            .await?;
            Ok(round)
        }
        .await;
        finish_transaction(tx, result).await
    }

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
        let state = ReviewRoundState::from_parts(
            ReviewVerdict::Pending,
            ThreadExecutionStatus::Running,
            None,
            None,
        )?;
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
            let failed_state = ReviewRoundState::from_parts(
                ReviewVerdict::Failed,
                ThreadExecutionStatus::Failed,
                Some(error.to_string()),
                Some(error.to_string()),
            )?;
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
                    let execution = state.execution_status();
                    let progress = state.into_progress();
                    update_work_unit_state(
                        &tx,
                        unit,
                        WorkUnitStatus::ReadyForReview,
                        execution,
                        progress,
                    )
                    .await?;
                }
                Some(ReviewScope::Integrated) => {
                    super::apply_task_command(
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
            if review_round_state(&round)?.reviewer_status() != ThreadExecutionStatus::Running
                || round.reviewer_thread_id.as_deref() != Some(reviewer_agent_id)
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
                    let task = super::task_run_record(run.clone())?;
                    if round.reviewed_head != run.expected_head
                        || task.kind() != TaskRunStateKind::Reviewing
                    {
                        bail!("integrated review no longer matches current Task HEAD");
                    }
                    match review.verdict {
                        ReviewVerdict::Pass => {}
                        ReviewVerdict::ChangesRequired | ReviewVerdict::Blocked => {
                            super::apply_task_command(
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
            let next_state = ReviewRoundState::from_parts(
                review.verdict,
                ThreadExecutionStatus::Completed,
                Some(review.summary.clone()),
                None,
            )?;
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
            if review_round_state(&round)?.reviewer_status() != ThreadExecutionStatus::Running {
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

    pub(crate) async fn settle_reviewer_turn_finished(
        &self,
        reviewer_agent_id: &str,
        outcome_kind: TurnOutcomeKind,
        reason: Option<&str>,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let rounds = entities::review_round::Entity::find()
                .filter(
                    entities::review_round::Column::ReviewerThreadId
                        .eq(reviewer_agent_id.to_string()),
                )
                .filter(
                    entities::review_round::Column::StateKind.eq(ReviewVerdict::Pending.as_str()),
                )
                .all(&tx)
                .await?;
            let round = match rounds.as_slice() {
                [] => return Ok(()),
                [round] => round.clone(),
                _ => bail!("reviewer owns multiple pending review rounds"),
            };
            let detail = reason
                .map(str::to_string)
                .unwrap_or_else(|| "reviewer ended without a successful review_exit".to_string());
            let outcome_status = match outcome_kind {
                TurnOutcomeKind::Cancelled => ThreadExecutionStatus::Cancelled,
                TurnOutcomeKind::Completed
                | TurnOutcomeKind::Failed
                | TurnOutcomeKind::BudgetLimited => ThreadExecutionStatus::Failed,
            };

            let failed_state = ReviewRoundState::from_parts(
                ReviewVerdict::Failed,
                outcome_status,
                Some(detail.clone()),
                Some(detail.clone()),
            )?;
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
                    if work_unit.state_kind == WorkUnitStatus::Reviewing.as_str() {
                        let state = work_unit_state(&work_unit)?;
                        let execution = state.execution_status();
                        let progress = state.into_progress();
                        update_work_unit_state(
                            &tx,
                            work_unit,
                            WorkUnitStatus::ReadyForReview,
                            execution,
                            progress,
                        )
                        .await?;
                    }
                }
                ReviewScope::Integrated => {
                    let run = entities::task_run::Entity::find_by_id(round.task_run_id)
                        .one(&tx)
                        .await?
                        .context("integrated review task run not found")?;
                    if super::task_run_record(run.clone())?.kind() == TaskRunStateKind::Reviewing {
                        super::apply_task_command(
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
    let reviewed_head = completion
        .head_commit
        .as_deref()
        .unwrap_or(work_unit.base_commit.as_str());
    if work_unit.state_kind != WorkUnitStatus::Reviewing.as_str()
        || completion.work_unit_id != work_unit.id
        || completion.revision != completion_revision
        || completion.status != WorkCompletionStatus::ReadyForReview.as_str()
        || round.reviewed_head != reviewed_head
    {
        bail!("delivery review target changed after reviewer creation");
    }
    let (completion_status, unit_status) = match review.verdict {
        ReviewVerdict::Pass => (
            WorkCompletionStatus::Approved,
            if WorkCompletionKind::from_str(&completion.kind)
                == Some(WorkCompletionKind::NoDelivery)
            {
                WorkUnitStatus::NoDelivery
            } else {
                WorkUnitStatus::Approved
            },
        ),
        ReviewVerdict::ChangesRequired | ReviewVerdict::Blocked => (
            WorkCompletionStatus::ChangesRequired,
            WorkUnitStatus::ChangesRequested,
        ),
        ReviewVerdict::Pending | ReviewVerdict::Failed => {
            bail!("reviewer cannot select pending or failed")
        }
    };
    let updated_completion = entities::work_completion::Entity::update_many()
        .col_expr(
            entities::work_completion::Column::Status,
            Expr::value(completion_status.as_str()),
        )
        .col_expr(
            entities::work_completion::Column::UpdatedAt,
            Expr::value(now),
        )
        .filter(entities::work_completion::Column::Id.eq(completion.id.clone()))
        .filter(entities::work_completion::Column::Revision.eq(completion_revision))
        .filter(
            entities::work_completion::Column::Status
                .eq(WorkCompletionStatus::ReadyForReview.as_str()),
        )
        .exec(tx)
        .await?;
    if updated_completion.rows_affected != 1 {
        bail!("delivery review completion is stale or was already settled");
    }
    let state = work_unit_state(&work_unit)?;
    let execution = state.execution_status();
    let progress = state.into_progress();
    update_work_unit_state(tx, work_unit, unit_status, execution, progress).await?;
    if completion_status == WorkCompletionStatus::Approved
        && WorkCompletionKind::from_str(&completion.kind) == Some(WorkCompletionKind::Delivery)
    {
        super::apply_task_command(
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

struct NewReviewRound<'a> {
    task_run_id: &'a str,
    target: NewReviewTarget<'a>,
    requested_by_call_id: &'a str,
    changed_files: Vec<String>,
}

enum NewReviewTarget<'a> {
    Delivery {
        work_unit_id: &'a str,
        completion_id: &'a str,
        completion_revision: i32,
        reviewed_head: &'a str,
    },
    Integrated {
        reviewed_head: &'a str,
    },
}

async fn insert_review_round(
    tx: &sea_orm::DatabaseTransaction,
    request: NewReviewRound<'_>,
) -> Result<ReviewRoundRecord> {
    let count = entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(request.task_run_id.to_string()))
        .count(tx)
        .await?;
    let (scope, work_unit_id, completion_id, completion_revision, reviewed_head) =
        match request.target {
            NewReviewTarget::Delivery {
                work_unit_id,
                completion_id,
                completion_revision,
                reviewed_head,
            } => (
                ReviewScope::Delivery,
                Some(work_unit_id.to_string()),
                Some(completion_id.to_string()),
                Some(completion_revision),
                reviewed_head,
            ),
            NewReviewTarget::Integrated { reviewed_head } => {
                (ReviewScope::Integrated, None, None, None, reviewed_head)
            }
        };
    let now = unix_seconds();
    let file_reviews = ReviewFileCoverage::pending(request.changed_files);
    review_round_record(
        entities::review_round::ActiveModel {
            id: Set(new_id("review")),
            task_run_id: Set(request.task_run_id.to_string()),
            round: Set(i32::try_from(count + 1)?),
            scope: Set(scope.as_str().to_string()),
            work_unit_id: Set(work_unit_id),
            completion_id: Set(completion_id),
            completion_revision: Set(completion_revision),
            reviewed_head: Set(reviewed_head.to_string()),
            requested_by_call_id: Set(request.requested_by_call_id.to_string()),
            reviewer_thread_id: Set(None),
            state_json: Set(serde_json::to_string(&ReviewRoundState::pending())?),
            revision: Set(0),
            design_references_json: Set("[]".to_string()),
            findings_json: Set("[]".to_string()),
            file_reviews_json: Set(Some(serde_json::to_string(&file_reviews)?)),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(tx)
        .await?,
    )
}

pub(super) fn review_round_record(
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
        reviewer_thread_id: model.reviewer_thread_id,
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

pub(super) fn review_round_state(
    model: &entities::review_round::Model,
) -> Result<ReviewRoundState> {
    let state = decode_review_round_state(&model.state_json)?;
    if state.verdict().as_str() != model.state_kind {
        bail!(
            "stored ReviewRound state discriminator mismatch: JSON is {}, generated column is {}",
            state.verdict().as_str(),
            model.state_kind
        );
    }
    Ok(state)
}

pub(super) async fn update_review_round_state<C>(
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

#[derive(Clone, Copy)]
enum ReviewCoverageSubmission {
    Rejected,
    Accepted,
}

async fn pending_review_for_reviewer(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
    reviewer_agent_id: &str,
) -> Result<entities::review_round::Model> {
    let rounds = entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
        .filter(entities::review_round::Column::ReviewerThreadId.eq(reviewer_agent_id.to_string()))
        .filter(entities::review_round::Column::StateKind.eq(ReviewVerdict::Pending.as_str()))
        .all(tx)
        .await?;
    match rounds.as_slice() {
        [round] => Ok(round.clone()),
        [] => bail!("pending review not found for reviewer"),
        _ => bail!("reviewer owns multiple pending reviews"),
    }
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

async fn active_implementation_run(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<entities::task_run::Model> {
    let run = active_nonterminal_run(tx, thread_id).await?;
    let record = super::task_run_record(run.clone())?;
    if !matches!(
        record.kind(),
        TaskRunStateKind::Implementing | TaskRunStateKind::Reworking
    ) || record.is_stop_requested()
    {
        bail!("review request requires implementing or reworking");
    }
    Ok(run)
}

async fn active_nonterminal_run(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<entities::task_run::Model> {
    let runs = entities::task_run::Entity::find()
        .filter(entities::task_run::Column::RootThreadId.eq(thread_id.to_string()))
        .filter(entities::task_run::Column::StateKind.is_not_in([
            TaskRunStateKind::Completed.as_str(),
            TaskRunStateKind::Failed.as_str(),
            TaskRunStateKind::Cancelled.as_str(),
        ]))
        .all(tx)
        .await?;
    match runs.as_slice() {
        [run] => Ok(run.clone()),
        [] => bail!("active task run not found"),
        _ => bail!("multiple active task runs found"),
    }
}

async fn ensure_no_pending_review(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
) -> Result<()> {
    if entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
        .filter(entities::review_round::Column::StateKind.eq(ReviewVerdict::Pending.as_str()))
        .one(tx)
        .await?
        .is_some()
    {
        bail!("task already has an active reviewer");
    }
    Ok(())
}

async fn ensure_review_call_unused(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
    requested_by_call_id: &str,
) -> Result<()> {
    if entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
        .filter(
            entities::review_round::Column::RequestedByCallId.eq(requested_by_call_id.to_string()),
        )
        .one(tx)
        .await?
        .is_some()
    {
        bail!("provider call already authorized a review");
    }
    Ok(())
}

async fn ensure_no_pending_delivery_review(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
    work_unit_id: &str,
) -> Result<()> {
    if entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
        .filter(entities::review_round::Column::WorkUnitId.eq(Some(work_unit_id.to_string())))
        .filter(entities::review_round::Column::StateKind.eq(ReviewVerdict::Pending.as_str()))
        .one(tx)
        .await?
        .is_some()
    {
        bail!("work unit already has an active reviewer");
    }
    Ok(())
}

async fn pending_review_by_call(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
    requested_by_call_id: &str,
) -> Result<entities::review_round::Model> {
    let rounds = entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
        .filter(
            entities::review_round::Column::RequestedByCallId.eq(requested_by_call_id.to_string()),
        )
        .filter(entities::review_round::Column::StateKind.eq(ReviewVerdict::Pending.as_str()))
        .all(tx)
        .await?;
    match rounds.as_slice() {
        [round] => Ok(round.clone()),
        [] => bail!("pending reviewer authorization not found"),
        _ => bail!("provider call authorized multiple reviews"),
    }
}

async fn finish_transaction<T>(tx: sea_orm::DatabaseTransaction, result: Result<T>) -> Result<T> {
    match result {
        Ok(value) => {
            tx.commit().await?;
            Ok(value)
        }
        Err(error) => {
            tx.rollback().await?;
            Err(error)
        }
    }
}
