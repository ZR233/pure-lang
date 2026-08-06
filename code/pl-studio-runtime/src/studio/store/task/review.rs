use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, TransactionTrait, sea_query::Expr,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentReview, ReviewRoundRecord, ReviewScope, ReviewVerdict, TaskRunPhase,
    ThreadExecutionStatus, WorkCompletionKind, WorkCompletionStatus, WorkUnitStatus,
};
use pl_core::TurnOutcomeKind;

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
                [unit] if unit.status == WorkUnitStatus::ReadyForReview.as_str() => unit.clone(),
                [unit] => bail!("executor work unit is {}, not readyForReview", unit.status),
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
                },
            )
            .await?;
            let mut unit_active: entities::work_unit::ActiveModel = work_unit.into();
            unit_active.status = Set(WorkUnitStatus::Reviewing.as_str().to_string());
            unit_active.updated_at = Set(unix_seconds());
            unit_active.update(&tx).await?;
            Ok(round)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn begin_integrated_review(
        &self,
        thread_id: &str,
        requested_by_call_id: &str,
    ) -> Result<ReviewRoundRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_implementation_run(&tx, thread_id).await?;
            ensure_review_call_unused(&tx, &run.id, requested_by_call_id).await?;
            let work_units = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
                .all(&tx)
                .await?;
            if work_units.iter().any(|unit| {
                !matches!(
                    WorkUnitStatus::from_str(&unit.status),
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
                        reviewed_head: &run.expected_head,
                    },
                    requested_by_call_id,
                },
            )
            .await?;
            let mut run_active: entities::task_run::ActiveModel = run.into();
            run_active.phase = Set(TaskRunPhase::Reviewing.as_str().to_string());
            run_active.status_message = Set(Some(
                "integrated reviewer is inspecting the task HEAD".to_string(),
            ));
            run_active.updated_at = Set(unix_seconds());
            run_active.update(&tx).await?;
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
            let now = unix_seconds();
            let mut round_active: entities::review_round::ActiveModel = round.into();
            round_active.reviewer_thread_id = Set(Some(agent_id.to_string()));
            round_active.reviewer_status = Set(ThreadExecutionStatus::Queued.as_str().to_string());
            round_active.reviewer_error = Set(None);
            round_active.updated_at = Set(now);
            let round = round_active.update(&tx).await?;
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
        if round.reviewer_thread_id.as_deref() != Some(reviewer_thread_id)
            || round.status != ReviewVerdict::Pending.as_str()
            || round.reviewer_status != ThreadExecutionStatus::Queued.as_str()
        {
            bail!("reviewer activation does not match the pending review round");
        }
        let mut active: entities::review_round::ActiveModel = round.into();
        active.reviewer_status = Set(ThreadExecutionStatus::Running.as_str().to_string());
        active.reviewer_error = Set(None);
        active.updated_at = Set(unix_seconds());
        active.update(&self.db).await?;
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
            let now = unix_seconds();
            let mut round_active: entities::review_round::ActiveModel = round.clone().into();
            round_active.status = Set(ReviewVerdict::Failed.as_str().to_string());
            round_active.reviewer_status = Set(ThreadExecutionStatus::Failed.as_str().to_string());
            round_active.reviewer_error = Set(Some(error.to_string()));
            round_active.summary = Set(Some(error.to_string()));
            round_active.updated_at = Set(now);
            round_active.update(&tx).await?;
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
                    let mut active: entities::work_unit::ActiveModel = unit.into();
                    active.status = Set(WorkUnitStatus::ReadyForReview.as_str().to_string());
                    active.updated_at = Set(now);
                    active.update(&tx).await?;
                }
                Some(ReviewScope::Integrated) => {
                    let mut active: entities::task_run::ActiveModel = run.into();
                    active.phase = Set(TaskRunPhase::Reworking.as_str().to_string());
                    active.status_message = Set(Some(format!("reviewer spawn failed: {error}")));
                    active.updated_at = Set(now);
                    active.update(&tx).await?;
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
    ) -> Result<ReviewRoundRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = active_nonterminal_run(&tx, thread_id).await?;
            let rounds = entities::review_round::Entity::find()
                .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
                .filter(
                    entities::review_round::Column::ReviewerThreadId
                        .eq(reviewer_agent_id.to_string()),
                )
                .filter(entities::review_round::Column::Status.eq(ReviewVerdict::Pending.as_str()))
                .all(&tx)
                .await?;
            let round = match rounds.as_slice() {
                [round] => round.clone(),
                [] => bail!("pending review not found for reviewer"),
                _ => bail!("reviewer owns multiple pending reviews"),
            };
            if round.reviewer_status != ThreadExecutionStatus::Running.as_str()
                || round.reviewer_thread_id.as_deref() != Some(reviewer_agent_id)
            {
                bail!("reviewer Thread does not match the pending review");
            }
            let now = unix_seconds();
            match ReviewScope::from_str(&round.scope).context("invalid stored review scope")? {
                ReviewScope::Delivery => {
                    complete_delivery_review(&tx, &run, &round, &review, now).await?;
                }
                ReviewScope::Integrated => {
                    if round.reviewed_head != run.expected_head
                        || run.phase != TaskRunPhase::Reviewing.as_str()
                    {
                        bail!("integrated review no longer matches current Task HEAD");
                    }
                    let next_phase = match review.verdict {
                        ReviewVerdict::Pass => TaskRunPhase::Reviewing.as_str().to_string(),
                        ReviewVerdict::ChangesRequired | ReviewVerdict::Blocked => {
                            TaskRunPhase::Reworking.as_str().to_string()
                        }
                        ReviewVerdict::Pending | ReviewVerdict::Failed => {
                            bail!("reviewer cannot select pending or failed")
                        }
                    };
                    let updated = entities::task_run::Entity::update_many()
                        .col_expr(entities::task_run::Column::Phase, Expr::value(next_phase))
                        .col_expr(
                            entities::task_run::Column::StatusMessage,
                            Expr::value(Some(review.summary.clone())),
                        )
                        .col_expr(entities::task_run::Column::UpdatedAt, Expr::value(now))
                        .filter(entities::task_run::Column::Id.eq(run.id.clone()))
                        .filter(
                            entities::task_run::Column::Phase.eq(TaskRunPhase::Reviewing.as_str()),
                        )
                        .filter(
                            entities::task_run::Column::ExpectedHead
                                .eq(round.reviewed_head.clone()),
                        )
                        .exec(&tx)
                        .await?;
                    if updated.rows_affected != 1 {
                        bail!("integrated review no longer matches current Task HEAD");
                    }
                }
            }
            let updated_round = entities::review_round::Entity::update_many()
                .col_expr(
                    entities::review_round::Column::Status,
                    Expr::value(review.verdict.as_str()),
                )
                .col_expr(
                    entities::review_round::Column::Summary,
                    Expr::value(Some(review.summary.clone())),
                )
                .col_expr(
                    entities::review_round::Column::ReviewerStatus,
                    Expr::value(ThreadExecutionStatus::Completed.as_str()),
                )
                .col_expr(
                    entities::review_round::Column::ReviewerError,
                    Expr::value(Option::<String>::None),
                )
                .col_expr(
                    entities::review_round::Column::DesignReferencesJson,
                    Expr::value(serde_json::to_string(&review.design_references)?),
                )
                .col_expr(
                    entities::review_round::Column::FindingsJson,
                    Expr::value(serde_json::to_string(&review.findings)?),
                )
                .col_expr(entities::review_round::Column::UpdatedAt, Expr::value(now))
                .filter(entities::review_round::Column::Id.eq(round.id.clone()))
                .filter(entities::review_round::Column::Status.eq(ReviewVerdict::Pending.as_str()))
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
                .filter(entities::review_round::Column::Status.eq(ReviewVerdict::Pending.as_str()))
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
            let now = unix_seconds();
            let outcome_status = match outcome_kind {
                TurnOutcomeKind::Cancelled => ThreadExecutionStatus::Cancelled,
                TurnOutcomeKind::Completed
                | TurnOutcomeKind::Failed
                | TurnOutcomeKind::BudgetLimited => ThreadExecutionStatus::Failed,
            };

            let updated_round = entities::review_round::Entity::update_many()
                .col_expr(
                    entities::review_round::Column::Status,
                    Expr::value(ReviewVerdict::Failed.as_str()),
                )
                .col_expr(
                    entities::review_round::Column::Summary,
                    Expr::value(Some(detail.clone())),
                )
                .col_expr(
                    entities::review_round::Column::ReviewerStatus,
                    Expr::value(outcome_status.as_str()),
                )
                .col_expr(
                    entities::review_round::Column::ReviewerError,
                    Expr::value(Some(detail.clone())),
                )
                .col_expr(entities::review_round::Column::UpdatedAt, Expr::value(now))
                .filter(entities::review_round::Column::Id.eq(round.id.clone()))
                .filter(entities::review_round::Column::Status.eq(ReviewVerdict::Pending.as_str()))
                .exec(&tx)
                .await?;
            if updated_round.rows_affected == 0 {
                return Ok(());
            }

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
                    if work_unit.status == WorkUnitStatus::Reviewing.as_str() {
                        let mut active: entities::work_unit::ActiveModel = work_unit.into();
                        active.status = Set(WorkUnitStatus::ReadyForReview.as_str().to_string());
                        active.updated_at = Set(now);
                        active.update(&tx).await?;
                    }
                }
                ReviewScope::Integrated => {
                    let run = entities::task_run::Entity::find_by_id(round.task_run_id)
                        .one(&tx)
                        .await?
                        .context("integrated review task run not found")?;
                    if run.phase == TaskRunPhase::Reviewing.as_str() {
                        let mut active: entities::task_run::ActiveModel = run.into();
                        active.phase = Set(TaskRunPhase::Reworking.as_str().to_string());
                        active.status_message = Set(Some(detail));
                        active.updated_at = Set(now);
                        active.update(&tx).await?;
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
    if work_unit.status != WorkUnitStatus::Reviewing.as_str()
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
    let updated_unit = entities::work_unit::Entity::update_many()
        .col_expr(
            entities::work_unit::Column::Status,
            Expr::value(unit_status.as_str()),
        )
        .col_expr(entities::work_unit::Column::UpdatedAt, Expr::value(now))
        .filter(entities::work_unit::Column::Id.eq(work_unit.id))
        .filter(entities::work_unit::Column::Status.eq(WorkUnitStatus::Reviewing.as_str()))
        .exec(tx)
        .await?;
    if updated_unit.rows_affected != 1 {
        bail!("delivery review work unit is stale or was already settled");
    }
    if completion_status == WorkCompletionStatus::Approved
        && WorkCompletionKind::from_str(&completion.kind) == Some(WorkCompletionKind::Delivery)
    {
        let updated_run = entities::task_run::Entity::update_many()
            .col_expr(
                entities::task_run::Column::Phase,
                Expr::value(TaskRunPhase::Merging.as_str()),
            )
            .col_expr(
                entities::task_run::Column::StatusMessage,
                Expr::value(Some(format!(
                    "approved Completion {} is ready for planner Git integration",
                    completion.id
                ))),
            )
            .col_expr(entities::task_run::Column::UpdatedAt, Expr::value(now))
            .filter(entities::task_run::Column::Id.eq(run.id.clone()))
            .filter(entities::task_run::Column::Phase.eq(run.phase.clone()))
            .exec(tx)
            .await?;
        if updated_run.rows_affected != 1 {
            bail!("TaskRun changed before entering planner merge phase");
        }
    }
    Ok(())
}

struct NewReviewRound<'a> {
    task_run_id: &'a str,
    target: NewReviewTarget<'a>,
    requested_by_call_id: &'a str,
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
            status: Set(ReviewVerdict::Pending.as_str().to_string()),
            requested_by_call_id: Set(request.requested_by_call_id.to_string()),
            reviewer_thread_id: Set(None),
            reviewer_status: Set(ThreadExecutionStatus::Queued.as_str().to_string()),
            reviewer_error: Set(None),
            summary: Set(None),
            design_references_json: Set("[]".to_string()),
            findings_json: Set("[]".to_string()),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(tx)
        .await?,
    )
}

pub(super) fn review_round_record(
    model: entities::review_round::Model,
) -> Result<ReviewRoundRecord> {
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
        verdict: ReviewVerdict::from_str(&model.status)
            .with_context(|| format!("invalid review verdict: {}", model.status))?,
        requested_by_call_id: model.requested_by_call_id,
        reviewer_thread_id: model.reviewer_thread_id,
        reviewer_status: ThreadExecutionStatus::from_str(&model.reviewer_status).with_context(
            || format!("invalid reviewer Thread status: {}", model.reviewer_status),
        )?,
        reviewer_error: model.reviewer_error,
        summary: model.summary,
        design_references: serde_json::from_str(&model.design_references_json)?,
        findings: serde_json::from_str(&model.findings_json)?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}

async fn active_implementation_run(
    tx: &sea_orm::DatabaseTransaction,
    thread_id: &str,
) -> Result<entities::task_run::Model> {
    let run = active_nonterminal_run(tx, thread_id).await?;
    if !matches!(
        TaskRunPhase::from_str(&run.phase),
        Some(TaskRunPhase::Implementing | TaskRunPhase::Reworking)
    ) || run.stop_requested != 0
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
        .filter(entities::task_run::Column::Phase.is_not_in([
            TaskRunPhase::Completed.as_str(),
            TaskRunPhase::Blocked.as_str(),
            TaskRunPhase::Failed.as_str(),
            TaskRunPhase::Cancelled.as_str(),
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
        .filter(entities::review_round::Column::Status.eq(ReviewVerdict::Pending.as_str()))
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
        .filter(entities::review_round::Column::Status.eq(ReviewVerdict::Pending.as_str()))
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
        .filter(entities::review_round::Column::Status.eq(ReviewVerdict::Pending.as_str()))
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
