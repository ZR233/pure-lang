use anyhow::{Context, Result};
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter,
    QueryOrder, SqliteTransactionMode, TransactionOptions, TransactionTrait,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    RecordTaskAgentFailure, ReviewRoundCommand, ReviewRoundStateKind, TaskCommand, TaskFailureKind,
    TaskIssueDisposition, TaskIssueRecord, TaskIssueSettlement, TaskIssueState, TaskOutcome,
    TaskWorktreeDisposition, WorkUnitCommand, WorkUnitStateKind,
};

use super::review::{review_round_state, update_review_round_state};
use super::work_unit::{apply_work_unit_command, work_unit_record};

impl StudioStore {
    pub(crate) async fn record_task_agent_failure(
        &self,
        input: RecordTaskAgentFailure,
    ) -> Result<Option<TaskIssueSettlement>> {
        // Acquire SQLite's write reservation before reading the TaskRun. This makes
        // terminalization first-writer-wins: a second fatal event cannot observe a
        // stale lifecycle state and overwrite the task outcome.
        let tx = self
            .db
            .begin_with_options(TransactionOptions {
                sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
                ..Default::default()
            })
            .await?;
        let result = async {
            let Some(run) = entities::task_run::Entity::find()
                .filter(entities::task_run::Column::RootThreadId.eq(input.root_thread_id.clone()))
                .order_by_desc(entities::task_run::Column::UpdatedAt)
                .one(&tx)
                .await?
            else {
                return Ok(None);
            };
            let record = super::task_run_record(run.clone())?;
            if record.kind().is_terminal() {
                return Ok(None);
            }
            if entities::task_issue::Entity::find()
                .filter(entities::task_issue::Column::TaskRunId.eq(run.id.clone()))
                .filter(entities::task_issue::Column::SourceTurnId.eq(input.source_turn_id.clone()))
                .one(&tx)
                .await?
                .is_some()
            {
                return Ok(Some(TaskIssueSettlement {
                    terminalized: false,
                }));
            }

            let work_unit = entities::work_unit::Entity::find()
                .filter(entities::work_unit::Column::TaskRunId.eq(run.id.clone()))
                .filter(
                    entities::work_unit::Column::ExecutorThreadId
                        .eq(input.source_thread_id.clone()),
                )
                .one(&tx)
                .await?;
            let review_round = entities::review_round::Entity::find()
                .filter(entities::review_round::Column::TaskRunId.eq(run.id.clone()))
                .filter(
                    entities::review_round::Column::ReviewerThreadId
                        .eq(input.source_thread_id.clone()),
                )
                .order_by_desc(entities::review_round::Column::Round)
                .one(&tx)
                .await?;
            let state =
                TaskIssueState::open_with_disposition(input.failure.clone(), input.disposition);
            let disposition = state.disposition();
            let now = unix_seconds();
            let issue_model = entities::task_issue::ActiveModel {
                id: Set(new_id("task-issue")),
                task_run_id: Set(run.id.clone()),
                source_thread_id: Set(input.source_thread_id),
                source_turn_id: Set(input.source_turn_id),
                source_agent_id: Set(input.source_agent_id),
                source_role: Set(input.source_role),
                work_unit_id: Set(work_unit.as_ref().map(|unit| unit.id.clone())),
                review_round_id: Set(review_round.as_ref().map(|round| round.id.clone())),
                state_json: Set(serde_json::to_string(&state)?),
                state_kind: NotSet,
                revision: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await?;

            let terminalized = disposition == TaskIssueDisposition::Fatal;
            if terminalized {
                settle_task_children(&tx, &issue_model.task_run_id, &input.failure.message).await?;
                super::apply_task_command(
                    &tx,
                    run,
                    TaskCommand::Complete {
                        outcome: TaskOutcome::Failed {
                            kind: TaskFailureKind::Fatal,
                            summary: input.failure.message.clone(),
                            evidence: format!(
                                "Task issue {} from turn {}",
                                issue_model.id, issue_model.source_turn_id
                            ),
                            cause: input.failure.message.clone(),
                            completed_at: now,
                        },
                    },
                )
                .await?;
            }
            Ok(Some(TaskIssueSettlement { terminalized }))
        }
        .await;
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

    pub(crate) async fn list_task_issues(&self, task_run_id: &str) -> Result<Vec<TaskIssueRecord>> {
        entities::task_issue::Entity::find()
            .filter(entities::task_issue::Column::TaskRunId.eq(task_run_id))
            .order_by_asc(entities::task_issue::Column::CreatedAt)
            .all(&self.db)
            .await?
            .into_iter()
            .map(task_issue_record)
            .collect()
    }
}

async fn settle_task_children(
    tx: &sea_orm::DatabaseTransaction,
    task_run_id: &str,
    message: &str,
) -> Result<()> {
    for unit in entities::work_unit::Entity::find()
        .filter(entities::work_unit::Column::TaskRunId.eq(task_run_id))
        .all(tx)
        .await?
    {
        let record = work_unit_record(unit.clone())?;
        if record.kind() == WorkUnitStateKind::Completed {
            continue;
        }
        apply_work_unit_command(
            tx,
            unit,
            WorkUnitCommand::FailExecution {
                operation_id: format!("task-failure:{task_run_id}"),
                detail: message.to_string(),
                disposition: TaskWorktreeDisposition::Protect,
            },
        )
        .await?;
    }
    for round in entities::review_round::Entity::find()
        .filter(entities::review_round::Column::TaskRunId.eq(task_run_id))
        .filter(entities::review_round::Column::StateKind.is_in([
            ReviewRoundStateKind::PendingDispatch.as_str(),
            ReviewRoundStateKind::Dispatched.as_str(),
            ReviewRoundStateKind::Running.as_str(),
        ]))
        .all(tx)
        .await?
    {
        let current = review_round_state(&round)?;
        let state = current
            .decide(
                &round.id,
                ReviewRoundCommand::Fail {
                    reviewer_thread_id: current.reviewer_thread_id().map(str::to_string),
                    error: message.to_string(),
                    summary: message.to_string(),
                },
            )?
            .next_state();
        update_review_round_state(tx, round, state).await?;
    }
    Ok(())
}

fn task_issue_record(model: entities::task_issue::Model) -> Result<TaskIssueRecord> {
    let state: TaskIssueState =
        serde_json::from_str(&model.state_json).context("invalid stored TaskIssue state JSON")?;
    if state.kind().as_str() != model.state_kind {
        anyhow::bail!(
            "stored TaskIssue state discriminator mismatch: JSON is {}, generated column is {}",
            state.kind().as_str(),
            model.state_kind
        );
    }
    Ok(TaskIssueRecord {
        id: model.id,
        task_run_id: model.task_run_id,
        source_thread_id: model.source_thread_id,
        source_turn_id: model.source_turn_id,
        source_agent_id: model.source_agent_id,
        source_role: model.source_role,
        work_unit_id: model.work_unit_id,
        review_round_id: model.review_round_id,
        state,
        revision: u64::try_from(model.revision).context("TaskIssue revision is negative")?,
        created_at: model.created_at,
        updated_at: model.updated_at,
    })
}
