use anyhow::{Context, Result};
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter,
    QueryOrder, SqliteTransactionMode, TransactionOptions, TransactionTrait, sea_query::Expr,
};

use crate::studio::entity as entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    RecordTaskAgentFailure, ReviewRoundCommand, ReviewRoundStateKind, TaskCommand, TaskFailureKind,
    TaskIssueCommand, TaskIssueDisposition, TaskIssueRecord, TaskIssueSettlement, TaskIssueState,
    TaskIssueStateKind, TaskOutcome, TaskRun, TaskWorktreeDisposition, WorkUnitCommand,
    WorkUnitStateKind,
};

use super::review::{review_round_state, update_review_round_state};
use super::work_unit::{apply_work_unit_command, work_unit_record};

pub(crate) struct ResolveTaskIssue<'a> {
    pub(crate) root_thread_id: &'a str,
    pub(crate) issue_id: &'a str,
    pub(crate) requested_by_call_id: &'a str,
    pub(crate) summary: &'a str,
    pub(crate) evidence: &'a str,
    pub(crate) expected_revision: u64,
    pub(crate) expected_generation: u64,
}

impl StudioStore {
    pub(crate) async fn resolve_task_issue(&self, input: ResolveTaskIssue<'_>) -> Result<TaskRun> {
        let ResolveTaskIssue {
            root_thread_id,
            issue_id,
            requested_by_call_id,
            summary,
            evidence,
            expected_revision,
            expected_generation,
        } = input;
        let (summary, evidence) = (summary.trim(), evidence.trim());
        if issue_id.trim().is_empty() || summary.is_empty() || evidence.is_empty() {
            anyhow::bail!("resolveIssue requires issueId, summary, and resolutionEvidence");
        }
        let tx = self.db.begin().await?;
        let run_model = entities::task_run::Entity::find()
            .filter(entities::task_run::Column::RootThreadId.eq(root_thread_id.to_string()))
            .filter(
                entities::task_run::Column::StateKind
                    .ne(crate::studio::task_coordinator::TaskRunStateKind::Completed.as_str()),
            )
            .one(&tx)
            .await?
            .context("active TaskRun not found")?;
        let run = super::task_run_record(run_model.clone())?;
        if run.revision != expected_revision || run.generation() != expected_generation {
            anyhow::bail!(
                "task version changed: expected revision {expected_revision}/generation {expected_generation}, actual {}/{}",
                run.revision,
                run.generation()
            );
        }
        let issue_model = entities::task_issue::Entity::find_by_id(issue_id.trim().to_string())
            .one(&tx)
            .await?
            .context("Task issue not found")?;
        if issue_model.task_run_id != run.id {
            anyhow::bail!("issueId does not belong to the active Task");
        }
        let issue = task_issue_record(issue_model.clone())?;
        let decision = issue.decide(
            issue.revision,
            TaskIssueCommand::Resolve {
                operation_id: requested_by_call_id.to_string(),
                summary: summary.to_string(),
                evidence: evidence.to_string(),
                resolved_at: unix_seconds(),
            },
        )?;
        let now = unix_seconds();
        let mut active: entities::task_issue::ActiveModel = issue_model.into();
        active.state_json = Set(serde_json::to_string(&decision.next_state())?);
        active.revision = Set(i64::try_from(issue.revision.saturating_add(1))?);
        active.updated_at = Set(now);
        active.update(&tx).await?;
        let updated = super::compare_and_swap_task_run(&tx, &run_model, None)
            .await?
            .context("Task issue resolution lost its TaskRun revision CAS")?;
        tx.commit().await?;
        super::task_run_record(updated)
    }

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
            let state = TaskIssueState::open(input.failure.clone());
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

    pub(crate) async fn resolve_recoverable_task_issues(
        &self,
        source_thread_id: &str,
    ) -> Result<()> {
        let now = unix_seconds();
        for model in entities::task_issue::Entity::find()
            .filter(entities::task_issue::Column::SourceThreadId.eq(source_thread_id.to_string()))
            .filter(
                entities::task_issue::Column::StateKind
                    .eq(TaskIssueStateKind::OpenRecoverable.as_str()),
            )
            .all(&self.db)
            .await?
        {
            let record = task_issue_record(model.clone())?;
            let decision = record.decide(
                record.revision,
                TaskIssueCommand::Resolve {
                    operation_id: format!(
                        "resolve-task-failure:{}:{}",
                        source_thread_id, record.source_turn_id
                    ),
                    summary: "后续执行已成功启动".to_string(),
                    evidence: format!("sourceThreadId={source_thread_id}"),
                    resolved_at: now,
                },
            )?;
            if !decision.changed() {
                continue;
            }
            let result = entities::task_issue::Entity::update_many()
                .col_expr(
                    entities::task_issue::Column::StateJson,
                    Expr::value(serde_json::to_string(&decision.next_state())?),
                )
                .col_expr(
                    entities::task_issue::Column::Revision,
                    Expr::value(model.revision.saturating_add(1)),
                )
                .col_expr(entities::task_issue::Column::UpdatedAt, Expr::value(now))
                .filter(entities::task_issue::Column::Id.eq(model.id))
                .filter(entities::task_issue::Column::Revision.eq(model.revision))
                .exec(&self.db)
                .await?;
            if result.rows_affected != 1 {
                anyhow::bail!("Task issue resolution lost its revision CAS");
            }
        }
        Ok(())
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
