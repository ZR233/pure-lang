use anyhow::{Context, Result, bail};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder,
    TransactionTrait,
};

use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentOutcomeRecord, AgentOutcomeStatus, AgentReview, MergeStatus, ReviewRoundRecord,
    ReviewVerdict, TaskRunPhase, WorkUnitStatus,
};
#[cfg(test)]
use crate::studio::task_coordinator::{CompleteReviewRound, CreateReviewRound};

use super::outcome::agent_outcome_record;

impl StudioStore {
    pub(crate) async fn begin_task_review(
        &self,
        session_id: &str,
        requested_by_call_id: &str,
    ) -> Result<ReviewRoundRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let runs = entities::task_run::Entity::find()
                .filter(entities::task_run::Column::SessionId.eq(session_id.to_string()))
                .filter(entities::task_run::Column::Phase.is_in([
                    TaskRunPhase::Implementing.as_str(),
                    TaskRunPhase::Reworking.as_str(),
                ]))
                .filter(entities::task_run::Column::StopRequested.eq(0))
                .all(&tx)
                .await?;
            let run = match runs.as_slice() {
                [run] => run.clone(),
                [] => bail!("task_request_review requires implementing or reworking"),
                _ => bail!("multiple reviewable task runs found for session"),
            };
            validate_review_gate(&tx, &run).await?;
            let rounds = entities::review_round::Entity::find()
                .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
                .order_by_asc(entities::review_round::Column::Round)
                .all(&tx)
                .await?;
            if rounds.len() >= 3 {
                bail!("task review exceeded the three-round limit");
            }
            if rounds.iter().any(|round| {
                (round.head_commit == run.expected_head
                    && round.status != ReviewVerdict::Failed.as_str())
                    || round.status == ReviewVerdict::Pending.as_str()
            }) {
                bail!("current HEAD is already reviewed or has a pending review");
            }
            let now = unix_seconds();
            let round = entities::review_round::ActiveModel {
                id: Set(new_id("review")),
                task_run_id: Set(run.id.clone()),
                round: Set(i32::try_from(rounds.len() + 1)?),
                head_commit: Set(run.expected_head.clone()),
                status: Set(ReviewVerdict::Pending.as_str().to_string()),
                reviewer_agent_id: Set(None),
                summary: Set(Some(format!("authorization:{requested_by_call_id}"))),
                design_references_json: Set("[]".to_string()),
                findings_json: Set("[]".to_string()),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await?;
            let mut run_active: entities::task_run::ActiveModel = run.into();
            run_active.phase = Set(TaskRunPhase::Reviewing.as_str().to_string());
            run_active.status_message =
                Set(Some("reviewer is inspecting the current HEAD".to_string()));
            run_active.updated_at = Set(now);
            run_active.update(&tx).await?;
            review_round_record(round)
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn authorize_reviewer_spawn(
        &self,
        session_id: &str,
        requested_by_call_id: &str,
        agent_id: &str,
    ) -> Result<(ReviewRoundRecord, AgentOutcomeRecord)> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = review_run(&tx, session_id).await?;
            let round = pending_review(&tx, &run.id).await?;
            if round.reviewer_agent_id.is_some()
                || round.summary.as_deref()
                    != Some(format!("authorization:{requested_by_call_id}").as_str())
            {
                bail!("reviewer spawn authorization is missing or already consumed");
            }
            let now = unix_seconds();
            let outcome = entities::agent_outcome::ActiveModel {
                id: Set(new_id("outcome")),
                task_run_id: Set(run.id.clone()),
                work_unit_id: Set(None),
                agent_id: Set(agent_id.to_string()),
                owner_path: Set("/root".to_string()),
                initiated_by: Set("planner".to_string()),
                requested_by_call_id: Set(requested_by_call_id.to_string()),
                role: Set("reviewer".to_string()),
                status: Set(AgentOutcomeStatus::Queued.as_str().to_string()),
                attempt: Set(round.round),
                summary: Set(None),
                error: Set(None),
                delivery_json: Set(None),
                review_json: Set(None),
                completion_contract_json: Set(None),
                delivery_recovery_count: Set(0),
                terminal_observed: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await?;
            let mut round_active: entities::review_round::ActiveModel = round.into();
            round_active.reviewer_agent_id = Set(Some(agent_id.to_string()));
            round_active.summary = Set(Some(format!("requestedByCallId:{requested_by_call_id}")));
            round_active.updated_at = Set(now);
            let round = round_active.update(&tx).await?;
            Ok((review_round_record(round)?, agent_outcome_record(outcome)?))
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn fail_reviewer_spawn(
        &self,
        session_id: &str,
        agent_id: Option<&str>,
        requested_by_call_id: &str,
        error: &str,
    ) -> Result<()> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = review_run(&tx, session_id).await?;
            let round = pending_review(&tx, &run.id).await?;
            let authorized = round.summary.as_deref()
                == Some(format!("authorization:{requested_by_call_id}").as_str())
                || agent_id
                    .is_some_and(|agent_id| round.reviewer_agent_id.as_deref() == Some(agent_id));
            if !authorized {
                bail!("review spawn failure does not match the pending authorization");
            }
            let now = unix_seconds();
            if let Some(agent_id) = agent_id {
                let outcomes = entities::agent_outcome::Entity::find()
                    .filter(entities::agent_outcome::Column::TaskRunId.eq(run.id.clone()))
                    .filter(entities::agent_outcome::Column::AgentId.eq(agent_id.to_string()))
                    .all(&tx)
                    .await?;
                if let [outcome] = outcomes.as_slice() {
                    let mut active: entities::agent_outcome::ActiveModel = outcome.clone().into();
                    active.status = Set(AgentOutcomeStatus::Failed.as_str().to_string());
                    active.error = Set(Some(error.to_string()));
                    active.updated_at = Set(now);
                    active.update(&tx).await?;
                }
            }
            let mut round_active: entities::review_round::ActiveModel = round.clone().into();
            round_active.status = Set(ReviewVerdict::Failed.as_str().to_string());
            round_active.summary = Set(Some(error.to_string()));
            round_active.updated_at = Set(now);
            round_active.update(&tx).await?;
            let mut run_active: entities::task_run::ActiveModel = run.into();
            run_active.phase = Set(if round.round == 1 {
                TaskRunPhase::Implementing.as_str().to_string()
            } else {
                TaskRunPhase::Reworking.as_str().to_string()
            });
            run_active.status_message = Set(Some(format!("reviewer spawn failed: {error}")));
            run_active.updated_at = Set(now);
            run_active.update(&tx).await?;
            Ok(())
        }
        .await;
        finish_transaction(tx, result).await
    }

    pub(crate) async fn complete_task_review(
        &self,
        session_id: &str,
        reviewer_agent_id: &str,
        review: AgentReview,
    ) -> Result<ReviewRoundRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let run = review_run(&tx, session_id).await?;
            let round = pending_review(&tx, &run.id).await?;
            if round.reviewer_agent_id.as_deref() != Some(reviewer_agent_id)
                || round.head_commit != run.expected_head
            {
                bail!("reviewer, round, and current task HEAD do not match");
            }
            let outcomes = entities::agent_outcome::Entity::find()
                .filter(entities::agent_outcome::Column::TaskRunId.eq(run.id.clone()))
                .filter(entities::agent_outcome::Column::AgentId.eq(reviewer_agent_id.to_string()))
                .all(&tx)
                .await?;
            let outcome = match outcomes.as_slice() {
                [outcome] => outcome.clone(),
                [] => bail!("reviewer outcome not found"),
                _ => bail!("multiple reviewer outcomes found"),
            };
            if outcome.role != "reviewer"
                || outcome.owner_path != "/root"
                || outcome.initiated_by != "planner"
                || outcome.status != AgentOutcomeStatus::Running.as_str()
                || outcome.requested_by_call_id.is_empty()
                || round.summary.as_deref()
                    != Some(format!("requestedByCallId:{}", outcome.requested_by_call_id).as_str())
            {
                bail!("reviewer outcome does not match the harness authorization");
            }
            let now = unix_seconds();
            let mut round_active: entities::review_round::ActiveModel = round.into();
            round_active.status = Set(review.verdict.as_str().to_string());
            round_active.summary = Set(Some(review.summary.clone()));
            round_active.design_references_json =
                Set(serde_json::to_string(&review.design_references)?);
            round_active.findings_json = Set(serde_json::to_string(&review.findings)?);
            round_active.updated_at = Set(now);
            let round = round_active.update(&tx).await?;
            let mut outcome_active: entities::agent_outcome::ActiveModel = outcome.into();
            outcome_active.status = Set(AgentOutcomeStatus::Completed.as_str().to_string());
            outcome_active.summary = Set(Some(review.summary.clone()));
            outcome_active.review_json = Set(Some(serde_json::to_string(&review)?));
            outcome_active.updated_at = Set(now);
            outcome_active.update(&tx).await?;
            let mut run_active: entities::task_run::ActiveModel = run.into();
            run_active.phase = Set(match review.verdict {
                ReviewVerdict::ChangesRequired => TaskRunPhase::Reworking.as_str().to_string(),
                ReviewVerdict::Pass | ReviewVerdict::Blocked => {
                    TaskRunPhase::Reviewing.as_str().to_string()
                }
                ReviewVerdict::Pending | ReviewVerdict::Failed => {
                    bail!("reviewer cannot select pending or failed verdict")
                }
            });
            run_active.status_message = Set(Some(review.summary));
            run_active.updated_at = Set(now);
            run_active.update(&tx).await?;
            review_round_record(round)
        }
        .await;
        finish_transaction(tx, result).await
    }
    #[cfg(test)]
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

    #[cfg(test)]
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

async fn validate_review_gate(
    tx: &sea_orm::DatabaseTransaction,
    run: &entities::task_run::Model,
) -> Result<()> {
    let lease = entities::branch_lease::Entity::find()
        .filter(entities::branch_lease::Column::TaskRunId.eq(run.id.clone()))
        .one(tx)
        .await?
        .context("task branch lease not found")?;
    if lease.expected_head != run.expected_head
        || lease.branch != run.branch
        || lease.git_common_dir != run.git_common_dir
    {
        bail!("task run and branch lease drifted before review");
    }
    let work_units = entities::work_unit::Entity::find()
        .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
        .all(tx)
        .await?;
    if work_units.iter().any(|unit| {
        matches!(
            WorkUnitStatus::from_str(&unit.status),
            Some(
                WorkUnitStatus::Pending
                    | WorkUnitStatus::Running
                    | WorkUnitStatus::WaitingForDelivery
                    | WorkUnitStatus::Delivered
            )
        )
    }) {
        bail!("all executor work must be merged or terminal before review");
    }
    let outcomes = entities::agent_outcome::Entity::find()
        .filter(entities::agent_outcome::Column::TaskRunId.eq(run.id.clone()))
        .all(tx)
        .await?;
    if outcomes.iter().any(|outcome| {
        matches!(
            AgentOutcomeStatus::from_str(&outcome.status),
            Some(
                AgentOutcomeStatus::Queued
                    | AgentOutcomeStatus::Running
                    | AgentOutcomeStatus::WaitingForDelivery
            )
        )
    }) {
        bail!("all task agents must be terminal before review");
    }
    let merges = entities::merge_record::Entity::find()
        .filter(entities::merge_record::Column::TaskRunId.eq(run.id.clone()))
        .all(tx)
        .await?;
    if merges.iter().any(|merge| {
        matches!(
            MergeStatus::from_str(&merge.status),
            Some(MergeStatus::Pending | MergeStatus::Verifying | MergeStatus::Conflicted)
        )
    }) {
        bail!("active merge must finish before review");
    }
    Ok(())
}

async fn review_run(
    tx: &sea_orm::DatabaseTransaction,
    session_id: &str,
) -> Result<entities::task_run::Model> {
    let runs = entities::task_run::Entity::find()
        .filter(entities::task_run::Column::SessionId.eq(session_id.to_string()))
        .filter(entities::task_run::Column::Phase.eq(TaskRunPhase::Reviewing.as_str()))
        .filter(entities::task_run::Column::StopRequested.eq(0))
        .all(tx)
        .await?;
    match runs.as_slice() {
        [run] => Ok(run.clone()),
        [] => bail!("reviewing task run not found for session"),
        _ => bail!("multiple reviewing task runs found for session"),
    }
}

async fn pending_review(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
) -> Result<entities::review_round::Model> {
    let rounds = entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id.to_string()))
        .filter(entities::review_round::Column::Status.eq(ReviewVerdict::Pending.as_str()))
        .all(tx)
        .await?;
    match rounds.as_slice() {
        [round] => Ok(round.clone()),
        [] => bail!("pending review round not found"),
        _ => bail!("multiple pending review rounds found"),
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
