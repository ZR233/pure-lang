//! 已由 `TaskRuntime` 决定的完整 Task 事实快照持久化适配器。
//!
//! 本模块不执行任何业务状态转换。它只在 writer 已开启的 SQLite 事务中校验
//! TaskRun 修订基线，并幂等写入内存 owner 已经提交的事实。

use anyhow::{Context, Result, bail};
use pl_core::canonical_content_hash;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseTransaction, EntityTrait,
    IntoActiveModel,
};
use serde::{Deserialize, Serialize};

use super::entity::{
    merge_record, review_round, task_issue, task_run, task_stop_event, work_completion, work_unit,
};
use super::store::object::{PersistedStudioObject, load_object, put_object};
use super::task_projection::LoadedTaskAggregate;

#[derive(Debug, Clone)]
pub(in crate::studio) struct TaskPersistenceCommit {
    pub(in crate::studio) owner_id: String,
    pub(in crate::studio) expected_owner_revision: u64,
    pub(in crate::studio) revision: u64,
    pub(in crate::studio) expected_run_revision: Option<u64>,
    pub(in crate::studio) aggregate: LoadedTaskAggregate,
    pub(in crate::studio) stop_events: Vec<TaskStopEventFact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct TaskStopEventFact {
    pub(in crate::studio) id: String,
    pub(in crate::studio) task_run_id: String,
    pub(in crate::studio) generation: u64,
    pub(in crate::studio) origin: String,
    pub(in crate::studio) reason: String,
    pub(in crate::studio) source_turn_id: Option<String>,
    pub(in crate::studio) created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskCommitReceipt {
    revision: u64,
    payload_hash: String,
}

pub(in crate::studio) async fn load_task_commit_revision(
    db: &impl ConnectionTrait,
    owner_id: &str,
) -> Result<Option<u64>> {
    Ok(load_object::<TaskCommitReceipt>(db, owner_id)
        .await?
        .map(|receipt| receipt.revision))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::studio) struct TaskCommitReceiptDto {
    revision: u64,
    payload_hash: String,
}

impl PersistedStudioObject for TaskCommitReceipt {
    type PersistenceDto = TaskCommitReceiptDto;

    const OWNER_KIND: &'static str = "task";
    const OBJECT_KIND: &'static str = "commitReceipt";
    const SCHEMA_VERSION: i64 = 1;

    fn revision(&self) -> u64 {
        self.revision
    }

    fn to_persistence_dto(&self) -> Self::PersistenceDto {
        TaskCommitReceiptDto {
            revision: self.revision,
            payload_hash: self.payload_hash.clone(),
        }
    }

    fn from_persistence_dto(dto: Self::PersistenceDto) -> Result<Self> {
        Ok(Self {
            revision: dto.revision,
            payload_hash: dto.payload_hash,
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskPersistencePayload<'a> {
    run: &'a super::task_coordinator::TaskRun,
    work_units: &'a [super::task_coordinator::WorkUnit],
    completions: &'a [super::task_coordinator::WorkCompletionRecord],
    merges: &'a [super::task_coordinator::MergeRecord],
    reviews: &'a [super::task_coordinator::ReviewRoundRecord],
    issues: &'a [super::task_coordinator::TaskIssueRecord],
    stop_events: &'a [TaskStopEventFact],
}

impl TaskPersistenceCommit {
    pub(in crate::studio) fn starts_lifecycle(&self) -> bool {
        self.expected_run_revision.is_none() && !self.aggregate.run.kind().is_terminal()
    }

    pub(in crate::studio) fn ends_lifecycle(&self) -> bool {
        self.aggregate.run.kind().is_terminal()
    }

    pub(in crate::studio) fn lifecycle_key(&self) -> String {
        format!("task:{}:{}", self.owner_id, self.aggregate.run.id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::studio) enum ApplyTaskCommitOutcome {
    Applied,
    AlreadyApplied,
    RevisionConflict { actual_revision: Option<u64> },
}

pub(in crate::studio) async fn apply_task_commit(
    tx: &DatabaseTransaction,
    commit: &TaskPersistenceCommit,
) -> Result<ApplyTaskCommitOutcome> {
    for event in &commit.stop_events {
        anyhow::ensure!(
            event.task_run_id == commit.aggregate.run.id,
            "Task stop event does not belong to the committed TaskRun"
        );
    }
    let payload_hash = task_commit_payload_hash(commit)?;
    if let Some(receipt) = load_object::<TaskCommitReceipt>(tx, &commit.owner_id).await? {
        if receipt.revision == commit.revision {
            return Ok(if receipt.payload_hash == payload_hash {
                ApplyTaskCommitOutcome::AlreadyApplied
            } else {
                ApplyTaskCommitOutcome::RevisionConflict {
                    actual_revision: Some(receipt.revision),
                }
            });
        }
        if receipt.revision != commit.expected_owner_revision {
            return Ok(ApplyTaskCommitOutcome::RevisionConflict {
                actual_revision: Some(receipt.revision),
            });
        }
    }
    let existing = task_run::Entity::find_by_id(commit.aggregate.run.id.clone())
        .one(tx)
        .await?;
    let actual_revision = existing
        .as_ref()
        .map(|model| u64::try_from(model.revision))
        .transpose()
        .context("stored TaskRun revision must not be negative")?;
    if actual_revision != commit.expected_run_revision {
        return Ok(ApplyTaskCommitOutcome::RevisionConflict { actual_revision });
    }

    upsert_task_run(tx, existing, &commit.aggregate).await?;
    for record in &commit.aggregate.work_units {
        upsert_work_unit(tx, record).await?;
    }
    for record in &commit.aggregate.completions {
        upsert_completion(tx, record).await?;
    }
    for record in &commit.aggregate.reviews {
        upsert_review(tx, record).await?;
    }
    for record in &commit.aggregate.merges {
        upsert_merge(tx, record).await?;
    }
    for record in &commit.aggregate.issues {
        upsert_issue(tx, record).await?;
    }
    for event in &commit.stop_events {
        upsert_stop_event(tx, event).await?;
    }
    put_object(
        tx,
        &commit.owner_id,
        &TaskCommitReceipt {
            revision: commit.revision,
            payload_hash,
        },
        commit.aggregate.run.updated_at,
    )
    .await?;
    Ok(ApplyTaskCommitOutcome::Applied)
}

fn task_commit_payload_hash(commit: &TaskPersistenceCommit) -> Result<String> {
    let payload = TaskPersistencePayload {
        run: &commit.aggregate.run,
        work_units: &commit.aggregate.work_units,
        completions: &commit.aggregate.completions,
        merges: &commit.aggregate.merges,
        reviews: &commit.aggregate.reviews,
        issues: &commit.aggregate.issues,
        stop_events: &commit.stop_events,
    };
    Ok(canonical_content_hash(&serde_json::to_vec(&payload)?))
}

async fn upsert_stop_event(tx: &DatabaseTransaction, event: &TaskStopEventFact) -> Result<()> {
    let existing = task_stop_event::Entity::find_by_id(event.id.clone())
        .one(tx)
        .await?;
    if let Some(existing) = existing {
        anyhow::ensure!(
            existing.task_run_id == event.task_run_id
                && u64::try_from(existing.generation).ok() == Some(event.generation)
                && existing.origin == event.origin
                && existing.reason == event.reason
                && existing.source_turn_id == event.source_turn_id
                && existing.created_at == event.created_at,
            "Task stop event id is already bound to different facts"
        );
        return Ok(());
    }
    task_stop_event::ActiveModel {
        id: Set(event.id.clone()),
        task_run_id: Set(event.task_run_id.clone()),
        generation: Set(i64::try_from(event.generation).context("Task stop generation overflow")?),
        origin: Set(event.origin.clone()),
        reason: Set(event.reason.clone()),
        source_turn_id: Set(event.source_turn_id.clone()),
        created_at: Set(event.created_at),
    }
    .insert(tx)
    .await?;
    Ok(())
}

async fn upsert_task_run(
    tx: &DatabaseTransaction,
    existing: Option<task_run::Model>,
    aggregate: &LoadedTaskAggregate,
) -> Result<()> {
    let run = &aggregate.run;
    let plan_json = run.plan.as_ref().map(serde_json::to_string).transpose()?;
    let is_new = existing.is_none();
    let mut active = existing.map_or_else(Default::default, IntoActiveModel::into_active_model);
    active.id = Set(run.id.clone());
    active.project_id = Set(run.project_id.clone());
    active.root_thread_id = Set(run.root_thread_id.clone());
    active.request = Set(run.request.clone());
    active.plan_json = Set(plan_json);
    active.workspace_root = Set(run.workspace_root.clone());
    active.state_json = Set(serde_json::to_string(&run.state)?);
    active.revision = Set(i64::try_from(run.revision).context("TaskRun revision overflow")?);
    active.created_at = Set(run.created_at);
    active.updated_at = Set(run.updated_at);
    if is_new {
        active.insert(tx).await?;
    } else {
        active.update(tx).await?;
    }
    Ok(())
}

async fn upsert_work_unit(
    tx: &DatabaseTransaction,
    record: &super::task_coordinator::WorkUnit,
) -> Result<()> {
    let existing = work_unit::Entity::find_by_id(record.id.clone())
        .one(tx)
        .await?;
    let is_new = existing.is_none();
    let mut active = existing.map_or_else(Default::default, IntoActiveModel::into_active_model);
    active.id = Set(record.id.clone());
    active.task_run_id = Set(record.task_run_id.clone());
    active.title = Set(record.title.clone());
    active.scope_hints_json = Set(serde_json::to_string(&record.scope_hints)?);
    active.base_commit = Set(record.base_commit.clone());
    active.worktree_path = Set(record.worktree_path.clone());
    active.branch = Set(record.branch.clone());
    active.attempt = Set(i32::try_from(record.attempt).context("WorkUnit attempt overflow")?);
    active.supersedes_work_unit_id = Set(record.supersedes_work_unit_id.clone());
    active.executor_thread_id = Set(record.executor_thread_id.clone());
    active.requested_by_call_id = Set(record.requested_by_call_id.clone());
    active.state_json = Set(super::task_coordinator::encode_work_unit_state(
        &record.state,
        record.blueprint.as_ref(),
    )?);
    active.revision = Set(i64::try_from(record.revision).context("WorkUnit revision overflow")?);
    active.created_at = Set(record.created_at);
    active.updated_at = Set(record.updated_at);
    if is_new {
        active.insert(tx).await?;
    } else {
        active.update(tx).await?;
    }
    Ok(())
}

async fn upsert_completion(
    tx: &DatabaseTransaction,
    record: &super::task_coordinator::WorkCompletionRecord,
) -> Result<()> {
    let existing = work_completion::Entity::find_by_id(record.id.clone())
        .one(tx)
        .await?;
    let is_new = existing.is_none();
    let mut active = existing.map_or_else(Default::default, IntoActiveModel::into_active_model);
    active.id = Set(record.id.clone());
    active.task_run_id = Set(record.task_run_id.clone());
    active.work_unit_id = Set(record.work_unit_id.clone());
    active.executor_agent_id = Set(record.executor_agent_id.clone());
    active.revision = Set(i32::try_from(record.revision).context("completion revision overflow")?);
    active.content_json = Set(serde_json::to_string(&record.content)?);
    active.state_json = Set(serde_json::to_string(&record.state)?);
    active.state_revision =
        Set(i64::try_from(record.state_revision).context("completion state revision overflow")?);
    active.base_commit = Set(record.base_commit.clone());
    active.verification_summary = Set(record.verification_summary.clone());
    active.worktree_path = Set(record.worktree_path.clone());
    active.branch = Set(record.branch.clone());
    active.created_at = Set(record.created_at);
    active.updated_at = Set(record.updated_at);
    if is_new {
        active.insert(tx).await?;
    } else {
        active.update(tx).await?;
    }
    Ok(())
}

async fn upsert_review(
    tx: &DatabaseTransaction,
    record: &super::task_coordinator::ReviewRoundRecord,
) -> Result<()> {
    anyhow::ensure!(
        record.reviewer_thread_id() == record.state.reviewer_thread_id(),
        "ReviewRound reviewer identity does not match its canonical state"
    );
    let existing = review_round::Entity::find_by_id(record.id.clone())
        .one(tx)
        .await?;
    let is_new = existing.is_none();
    let mut active = existing.map_or_else(Default::default, IntoActiveModel::into_active_model);
    active.id = Set(record.id.clone());
    active.task_run_id = Set(record.task_run_id.clone());
    active.round = Set(i32::try_from(record.round).context("review round overflow")?);
    active.scope = Set(record.scope.as_str().to_string());
    active.work_unit_id = Set(record.work_unit_id.clone());
    active.completion_id = Set(record.completion_id.clone());
    active.completion_revision = Set(record
        .completion_revision
        .map(i32::try_from)
        .transpose()
        .context("review completion revision overflow")?);
    active.reviewed_head = Set(record.reviewed_head.clone());
    active.requested_by_call_id = Set(record.requested_by_call_id.clone());
    active.reviewer_thread_id = Set(record.reviewer_thread_id().map(ToOwned::to_owned));
    active.state_json = Set(serde_json::to_string(&record.state)?);
    active.revision = Set(i64::try_from(record.revision).context("review revision overflow")?);
    active.design_references_json = Set(serde_json::to_string(&record.design_references)?);
    active.findings_json = Set(serde_json::to_string(&record.findings)?);
    active.created_at = Set(record.created_at);
    active.updated_at = Set(record.updated_at);
    active.file_reviews_json = Set(record
        .file_reviews
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?);
    if is_new {
        active.insert(tx).await?;
    } else {
        active.update(tx).await?;
    }
    Ok(())
}

async fn upsert_merge(
    tx: &DatabaseTransaction,
    record: &super::task_coordinator::MergeRecord,
) -> Result<()> {
    let existing = merge_record::Entity::find_by_id(record.id.clone())
        .one(tx)
        .await?;
    let is_new = existing.is_none();
    let mut active = existing.map_or_else(Default::default, IntoActiveModel::into_active_model);
    active.id = Set(record.id.clone());
    active.task_run_id = Set(record.task_run_id.clone());
    active.work_unit_id = Set(record.work_unit_id.clone());
    active.completion_id = Set(record.completion_id.clone());
    active.completion_revision =
        Set(i32::try_from(record.completion_revision)
            .context("merge completion revision overflow")?);
    active.executor_agent_id = Set(record.executor_agent_id.clone());
    active.expected_previous_head = Set(record.expected_previous_head.clone());
    active.resulting_head = Set(record.resulting_head.clone());
    active.delivery_head = Set(record.delivery_head.clone());
    active.method = Set(record.method.as_str().to_string());
    active.summary = Set(record.summary.clone());
    active.cleanup_state_json = Set(serde_json::to_string(&record.cleanup)?);
    active.revision = Set(i64::try_from(record.revision).context("merge revision overflow")?);
    active.created_at = Set(record.created_at);
    active.updated_at = Set(record.updated_at);
    if is_new {
        active.insert(tx).await?;
    } else {
        active.update(tx).await?;
    }
    Ok(())
}

async fn upsert_issue(
    tx: &DatabaseTransaction,
    record: &super::task_coordinator::TaskIssueRecord,
) -> Result<()> {
    let existing = task_issue::Entity::find_by_id(record.id.clone())
        .one(tx)
        .await?;
    let is_new = existing.is_none();
    let mut active = existing.map_or_else(Default::default, IntoActiveModel::into_active_model);
    active.id = Set(record.id.clone());
    active.task_run_id = Set(record.task_run_id.clone());
    active.source_thread_id = Set(record.source_thread_id.clone());
    active.source_turn_id = Set(record.source_turn_id.clone());
    active.source_agent_id = Set(record.source_agent_id.clone());
    active.source_role = Set(record.source_role.clone());
    active.work_unit_id = Set(record.work_unit_id.clone());
    active.review_round_id = Set(record.review_round_id.clone());
    active.state_json = Set(serde_json::to_string(&record.state)?);
    active.revision = Set(i64::try_from(record.revision).context("issue revision overflow")?);
    active.created_at = Set(record.created_at);
    active.updated_at = Set(record.updated_at);
    if is_new {
        active.insert(tx).await?;
    } else {
        active.update(tx).await?;
    }
    Ok(())
}

pub(in crate::studio) fn validate_task_commit(commit: &TaskPersistenceCommit) -> Result<()> {
    if commit.owner_id != commit.aggregate.run.root_thread_id {
        bail!("Task persistence owner does not match TaskRun root Thread");
    }
    if commit.revision != commit.expected_owner_revision.saturating_add(1) {
        bail!("Task persistence owner revision is not contiguous");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sea_orm::TransactionTrait;

    use super::*;
    use crate::StudioMode;
    use crate::studio::task_coordinator::{
        CreateTaskRun, WorkUnit, WorkUnitContext, WorkUnitState,
    };
    use crate::studio::{StudioStore, task_projection};

    #[tokio::test]
    async fn same_revision_replay_requires_identical_snapshot_and_stop_events() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let workspace = std::env::temp_dir().join("pure-task-persistence-idempotence");
        let project = store.upsert_project(&workspace).await.expect("project");
        let thread = store
            .create_thread(&project.id, "Task", StudioMode::Task)
            .await
            .expect("thread");
        let run = store
            .create_task_run(CreateTaskRun {
                project_id: project.id,
                root_thread_id: thread.id.clone(),
                request: "implement".to_string(),
                workspace_root: workspace.to_string_lossy().to_string(),
            })
            .await
            .expect("task run");
        let mut facts = task_projection::load_task_aggregate(&store, &thread.id)
            .await
            .expect("load aggregate")
            .expect("aggregate exists");
        facts.run.revision = 1;
        facts.run.updated_at = facts.run.updated_at.saturating_add(1);
        facts.refresh_projection().expect("refresh projection");
        let commit = TaskPersistenceCommit {
            owner_id: thread.id.clone(),
            expected_owner_revision: 1,
            revision: 2,
            expected_run_revision: Some(0),
            aggregate: facts.clone(),
            stop_events: Vec::new(),
        };

        let tx = store.database().begin().await.expect("begin first apply");
        assert_eq!(
            apply_task_commit(&tx, &commit).await.expect("first apply"),
            ApplyTaskCommitOutcome::Applied
        );
        tx.commit().await.expect("commit first apply");

        let tx = store.database().begin().await.expect("begin replay");
        assert_eq!(
            apply_task_commit(&tx, &commit)
                .await
                .expect("identical replay"),
            ApplyTaskCommitOutcome::AlreadyApplied
        );
        tx.rollback().await.expect("rollback replay");

        let now = crate::studio::ids::unix_seconds();
        let mut different = commit.clone();
        different.aggregate.work_units.push(WorkUnit {
            context: WorkUnitContext {
                id: "stale-work-unit".to_string(),
                task_run_id: run.id.clone(),
                title: "stale".to_string(),
                scope_hints: Vec::new(),
                blueprint: None,
                base_commit: "HEAD".to_string(),
                worktree_path: workspace
                    .join(".pure/worktrees/stale")
                    .to_string_lossy()
                    .to_string(),
                branch: "pure-task-stale".to_string(),
                attempt: 1,
                supersedes_work_unit_id: None,
                executor_thread_id: Some("executor-stale".to_string()),
                requested_by_call_id: "spawn-stale".to_string(),
            },
            state: WorkUnitState::pending(),
            revision: 0,
            created_at: now,
            updated_at: now,
        });
        different
            .aggregate
            .refresh_projection()
            .expect("refresh different projection");
        let tx = store.database().begin().await.expect("begin conflict");
        assert_eq!(
            apply_task_commit(&tx, &different)
                .await
                .expect("different replay"),
            ApplyTaskCommitOutcome::RevisionConflict {
                actual_revision: Some(2)
            }
        );
        tx.rollback().await.expect("rollback conflict");

        let mut stopped_facts = facts;
        stopped_facts.run.revision = 2;
        stopped_facts.run.updated_at = stopped_facts.run.updated_at.saturating_add(1);
        stopped_facts
            .refresh_projection()
            .expect("refresh stop projection");
        let stop_event = TaskStopEventFact {
            id: "task-stop-idempotent".to_string(),
            task_run_id: run.id.clone(),
            generation: 1,
            origin: "runtimeFailure".to_string(),
            reason: "stop".to_string(),
            source_turn_id: None,
            created_at: now,
        };
        let stop_commit = TaskPersistenceCommit {
            owner_id: thread.id,
            expected_owner_revision: 2,
            revision: 3,
            expected_run_revision: Some(1),
            aggregate: stopped_facts,
            stop_events: vec![stop_event],
        };
        let tx = store.database().begin().await.expect("begin stop");
        assert_eq!(
            apply_task_commit(&tx, &stop_commit)
                .await
                .expect("apply stop"),
            ApplyTaskCommitOutcome::Applied
        );
        tx.commit().await.expect("commit stop");
        let tx = store.database().begin().await.expect("begin stop replay");
        assert_eq!(
            apply_task_commit(&tx, &stop_commit)
                .await
                .expect("replay stop"),
            ApplyTaskCommitOutcome::AlreadyApplied
        );
        tx.rollback().await.expect("rollback stop replay");

        let mut conflicting_stop = stop_commit;
        conflicting_stop.stop_events[0].reason = "different stop".to_string();
        let tx = store.database().begin().await.expect("begin stop conflict");
        assert_eq!(
            apply_task_commit(&tx, &conflicting_stop)
                .await
                .expect("conflicting stop"),
            ApplyTaskCommitOutcome::RevisionConflict {
                actual_revision: Some(3)
            }
        );
        tx.rollback().await.expect("rollback stop conflict");
    }

    #[tokio::test]
    async fn stop_event_must_belong_to_committed_task_run() {
        let store = StudioStore::open_memory().await.expect("memory store");
        let workspace = std::env::temp_dir().join("pure-task-persistence-stop-owner");
        let project = store.upsert_project(&workspace).await.expect("project");
        let thread = store
            .create_thread(&project.id, "Task", StudioMode::Task)
            .await
            .expect("thread");
        store
            .create_task_run(CreateTaskRun {
                project_id: project.id,
                root_thread_id: thread.id.clone(),
                request: "implement".to_string(),
                workspace_root: workspace.to_string_lossy().to_string(),
            })
            .await
            .expect("task run");
        let facts = task_projection::load_task_aggregate(&store, &thread.id)
            .await
            .expect("load aggregate")
            .expect("aggregate exists");
        let commit = TaskPersistenceCommit {
            owner_id: thread.id,
            expected_owner_revision: 0,
            revision: 1,
            expected_run_revision: Some(0),
            aggregate: facts,
            stop_events: vec![TaskStopEventFact {
                id: "foreign-stop".to_string(),
                task_run_id: "another-task".to_string(),
                generation: 1,
                origin: "runtimeFailure".to_string(),
                reason: "stop".to_string(),
                source_turn_id: None,
                created_at: crate::studio::ids::unix_seconds(),
            }],
        };
        let tx = store.database().begin().await.expect("begin");
        let error = apply_task_commit(&tx, &commit)
            .await
            .expect_err("foreign stop must fail");
        assert!(error.to_string().contains("does not belong"));
        tx.rollback().await.expect("rollback");
    }
}
