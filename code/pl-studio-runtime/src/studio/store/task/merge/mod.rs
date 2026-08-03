mod cleanup;
mod conflict_resolution;
mod record;

pub(super) use record::{merge_record, parse_required_evidence};

use anyhow::{Context, Result, bail};
use pl_core::{AgentLifecycleState, AgentSnapshot};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait,
    QueryFilter, QueryOrder, Statement, TransactionTrait,
};

use super::outcome::agent_outcome_record;
use super::work_completion::{delivery_from_completion, work_completion_record};
use super::work_unit::work_unit_record;
use super::{branch_lease_record, task_run_record};
use crate::agent::worktree::same_worktree_path;
use crate::studio::entities;
use crate::studio::ids::{new_id, unix_seconds};
use crate::studio::store::StudioStore;
use crate::studio::task_coordinator::{
    AgentOutcomeStatus, BeginTaskMerge, CompleteTaskMerge, ConflictTaskMerge, FailTaskMerge,
    MergeEvidence, MergeRecord, MergeStatus, TaskMergeScope, TaskRunPhase, TaskRunRecord,
    WorkCompletionStatus, WorkUnitStatus,
};

impl StudioStore {
    pub(crate) async fn begin_task_merge(&self, input: BeginTaskMerge) -> Result<TaskMergeScope> {
        let tx = self.db.begin().await?;
        let result = async {
            let runs = entities::task_run::Entity::find()
                .filter(entities::task_run::Column::SessionId.eq(input.session_id.clone()))
                .filter(entities::task_run::Column::Phase.is_not_in([
                    TaskRunPhase::Completed.as_str(),
                    TaskRunPhase::Blocked.as_str(),
                    TaskRunPhase::Failed.as_str(),
                    TaskRunPhase::Cancelled.as_str(),
                ]))
                .all(&tx)
                .await?;
            let run_model = match runs.as_slice() {
                [run] => run.clone(),
                [] => bail!("active task run not found for this session"),
                _ => bail!("multiple active task runs found for this session"),
            };
            let origin_phase = TaskRunPhase::from_str(&run_model.phase)
                .with_context(|| format!("invalid stored task phase: {}", run_model.phase))?;
            if !matches!(
                origin_phase,
                TaskRunPhase::Implementing | TaskRunPhase::Reworking
            ) {
                bail!("task merge requires phase implementing or reworking");
            }
            if run_model.expected_head != input.expected_head {
                bail!("caller expectedHeadCommit does not match TaskRun expectedHead");
            }

            let lease_model = entities::branch_lease::Entity::find()
                .filter(entities::branch_lease::Column::TaskRunId.eq(run_model.id.clone()))
                .one(&tx)
                .await?
                .context("task branch lease not found")?;
            if lease_model.expected_head != input.expected_head
                || lease_model.git_common_dir != run_model.git_common_dir
                || lease_model.branch != run_model.branch
            {
                bail!("TaskRun and BranchLease do not describe the same branch head");
            }

            let existing = entities::merge_record::Entity::find()
                .filter(entities::merge_record::Column::TaskRunId.eq(run_model.id.clone()))
                .all(&tx)
                .await?;
            if existing.iter().any(|record| {
                matches!(
                    MergeStatus::from_str(&record.status),
                    Some(MergeStatus::Pending | MergeStatus::Verifying | MergeStatus::Conflicted)
                )
            }) {
                bail!("task run already has an active merge");
            }

            let outcomes = entities::agent_outcome::Entity::find()
                .filter(entities::agent_outcome::Column::TaskRunId.eq(run_model.id.clone()))
                .filter(entities::agent_outcome::Column::AgentId.eq(input.agent_id.clone()))
                .all(&tx)
                .await?;
            let outcome_model = match outcomes.as_slice() {
                [outcome] => outcome.clone(),
                [] => bail!("approved executor outcome not found for agent"),
                _ => bail!("ambiguous executor outcome for agent"),
            };
            if outcome_model.role != "executor"
                || outcome_model.initiated_by != "planner"
                || outcome_model.owner_path != "/root"
                || outcome_model.status != AgentOutcomeStatus::Completed.as_str()
            {
                bail!("agent outcome is not a planner-owned completed executor delivery");
            }
            let runtime_row = tx
                .query_one(Statement::from_sql_and_values(
                    DatabaseBackend::Sqlite,
                    "SELECT snapshot_json FROM agent_runtime_states WHERE agent_id = ?",
                    [input.agent_id.clone().into()],
                ))
                .await?
                .context("executor canonical runtime snapshot not found")?;
            let snapshot: AgentSnapshot =
                serde_json::from_str(&runtime_row.try_get::<String>("", "snapshot_json")?)
                    .context("executor canonical runtime snapshot is invalid")?;
            if snapshot.identity.id.as_str() != input.agent_id
                || snapshot.identity.role.as_str() != "executor"
                || snapshot.lifecycle != AgentLifecycleState::Closed
            {
                bail!("executor must be canonically closed before merge");
            }
            let work_unit_id = outcome_model
                .work_unit_id
                .clone()
                .context("executor outcome has no work unit")?;
            let work_unit_model = entities::work_unit::Entity::find_by_id(work_unit_id.clone())
                .one(&tx)
                .await?
                .context("executor work unit not found")?;
            if work_unit_model.task_run_id != run_model.id
                || work_unit_model.agent_id.as_deref() != Some(input.agent_id.as_str())
                || work_unit_model.status != WorkUnitStatus::Approved.as_str()
                || work_unit_model.attempt != outcome_model.attempt
                || work_unit_model.attempt <= 0
            {
                bail!("work unit does not match an approved executor completion");
            }
            let completion_model = entities::work_completion::Entity::find()
                .filter(
                    entities::work_completion::Column::WorkUnitId.eq(work_unit_model.id.clone()),
                )
                .order_by_desc(entities::work_completion::Column::Revision)
                .one(&tx)
                .await?
                .context("approved work unit has no completion")?;
            if completion_model.executor_agent_id != input.agent_id
                || completion_model.status != WorkCompletionStatus::Approved.as_str()
            {
                bail!("latest executor completion is not approved");
            }
            let completion = work_completion_record(completion_model)?;
            let delivery = delivery_from_completion(&completion)?;
            if !same_worktree_path(&delivery.worktree.path, &work_unit_model.worktree_path)
                || delivery.worktree.branch != work_unit_model.branch
                || delivery.base_commit != work_unit_model.base_commit
                || delivery.changed_files != input.changed_files
            {
                bail!("delivery identity no longer matches its validated work unit scope");
            }
            if existing.iter().any(|record| {
                (record.agent_id == input.agent_id || record.source_commit == delivery.head_commit)
                    && !matches!(
                        MergeStatus::from_str(&record.status),
                        Some(MergeStatus::Failed | MergeStatus::Aborted)
                    )
            }) {
                bail!("executor delivery already has a merge record");
            }

            let evidence = MergeEvidence {
                version: 1,
                origin_phase,
                work_unit_id: work_unit_id.clone(),
                outcome_id: outcome_model.id.clone(),
                completion_id: completion.id.clone(),
                completion_revision: completion.revision,
                delivery_head: delivery.head_commit.clone(),
                pre_index_tree: input.pre_index_tree,
                changed_files: input.changed_files,
                verification_steps: Vec::new(),
                merge_commit: None,
                conflict_manifest: None,
                conflict_verification: None,
                compensation: None,
                cleanup: None,
            };
            let now = unix_seconds();
            let merge_model = entities::merge_record::ActiveModel {
                id: Set(new_id("merge")),
                task_run_id: Set(run_model.id.clone()),
                agent_id: Set(input.agent_id),
                status: Set(MergeStatus::Pending.as_str().to_string()),
                expected_head: Set(input.expected_head),
                source_commit: Set(delivery.head_commit.clone()),
                conflict_files_json: Set("[]".to_string()),
                resolution_summary: Set(None),
                verification_json: Set(Some(serde_json::to_string(&evidence)?)),
                attempt: Set(0),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&tx)
            .await?;
            let mut run_active: entities::task_run::ActiveModel = run_model.into();
            run_active.phase = Set(TaskRunPhase::Merging.as_str().to_string());
            run_active.status_message = Set(None);
            run_active.updated_at = Set(now);
            let run_model = run_active.update(&tx).await?;
            let mut work_unit_active: entities::work_unit::ActiveModel =
                work_unit_model.clone().into();
            work_unit_active.status = Set(WorkUnitStatus::Merging.as_str().to_string());
            work_unit_active.updated_at = Set(now);
            let work_unit_model = work_unit_active.update(&tx).await?;

            Ok(TaskMergeScope {
                run: task_run_record(run_model)?,
                lease: branch_lease_record(lease_model),
                work_unit: work_unit_record(work_unit_model)?,
                outcome: agent_outcome_record(outcome_model)?,
                completion,
                delivery,
                merge: merge_record(merge_model)?,
            })
        }
        .await;
        match result {
            Ok(scope) => {
                tx.commit().await?;
                Ok(scope)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn mark_task_merge_verifying(&self, merge_id: &str) -> Result<MergeRecord> {
        let model = entities::merge_record::Entity::find_by_id(merge_id.to_string())
            .one(&self.db)
            .await?
            .context("merge record not found")?;
        if model.status != MergeStatus::Pending.as_str() {
            bail!("only a pending merge can enter verification");
        }
        let mut active: entities::merge_record::ActiveModel = model.into();
        active.status = Set(MergeStatus::Verifying.as_str().to_string());
        active.updated_at = Set(unix_seconds());
        merge_record(active.update(&self.db).await?)
    }

    pub(crate) async fn recover_unstarted_task_merge(
        &self,
        merge_id: &str,
    ) -> Result<TaskRunRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let merge = entities::merge_record::Entity::find_by_id(merge_id.to_string())
                .one(&tx)
                .await?
                .context("merge record not found")?;
            if merge.status != MergeStatus::Pending.as_str() {
                bail!("only a pending merge can recover before Git starts");
            }
            let evidence = parse_required_evidence(merge.verification_json.as_deref())?;
            let run = entities::task_run::Entity::find_by_id(merge.task_run_id.clone())
                .one(&tx)
                .await?
                .context("task run not found")?;
            if run.phase != TaskRunPhase::Merging.as_str()
                || run.expected_head != merge.expected_head
            {
                bail!("task run no longer matches the pending merge prestate");
            }
            let now = unix_seconds();
            let mut merge_active: entities::merge_record::ActiveModel = merge.into();
            merge_active.status = Set(MergeStatus::Failed.as_str().to_string());
            merge_active.resolution_summary = Set(Some(
                "restart recovered pending merge before Git started".to_string(),
            ));
            merge_active.updated_at = Set(now);
            merge_active.update(&tx).await?;
            let mut run_active: entities::task_run::ActiveModel = run.into();
            run_active.phase = Set(evidence.origin_phase.as_str().to_string());
            run_active.status_message = Set(Some(
                "pending merge recovered before Git started; planner may retry".to_string(),
            ));
            run_active.updated_at = Set(now);
            let run = task_run_record(run_active.update(&tx).await?)?;
            let work_unit = entities::work_unit::Entity::find_by_id(evidence.work_unit_id.clone())
                .one(&tx)
                .await?
                .context("pending merge work unit not found")?;
            if work_unit.status != WorkUnitStatus::Merging.as_str() {
                bail!("pending merge work unit left merging");
            }
            let mut work_unit_active: entities::work_unit::ActiveModel = work_unit.into();
            work_unit_active.status = Set(WorkUnitStatus::Approved.as_str().to_string());
            work_unit_active.updated_at = Set(now);
            work_unit_active.update(&tx).await?;
            Ok(run)
        }
        .await;
        match result {
            Ok(run) => {
                tx.commit().await?;
                Ok(run)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn complete_task_merge(
        &self,
        input: CompleteTaskMerge,
    ) -> Result<MergeRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let merge = entities::merge_record::Entity::find_by_id(input.merge_id.clone())
                .one(&tx)
                .await?
                .context("merge record not found")?;
            if merge.status != MergeStatus::Verifying.as_str()
                || merge.expected_head != input.expected_head
            {
                bail!("merge record no longer matches the verifying CAS scope");
            }
            let mut evidence = parse_required_evidence(merge.verification_json.as_deref())?;
            let run = entities::task_run::Entity::find_by_id(merge.task_run_id.clone())
                .one(&tx)
                .await?
                .context("task run not found")?;
            if run.phase != TaskRunPhase::Merging.as_str()
                || run.expected_head != input.expected_head
            {
                bail!("TaskRun no longer matches the active merge CAS scope");
            }
            let lease = entities::branch_lease::Entity::find()
                .filter(entities::branch_lease::Column::TaskRunId.eq(run.id.clone()))
                .one(&tx)
                .await?
                .context("task branch lease not found")?;
            if lease.expected_head != input.expected_head
                || lease.branch != run.branch
                || lease.git_common_dir != run.git_common_dir
            {
                bail!("BranchLease no longer matches the active merge CAS scope");
            }
            let work_unit = entities::work_unit::Entity::find_by_id(evidence.work_unit_id.clone())
                .one(&tx)
                .await?
                .context("merge work unit not found")?;
            let outcome = entities::agent_outcome::Entity::find_by_id(evidence.outcome_id.clone())
                .one(&tx)
                .await?
                .context("merge agent outcome not found")?;
            let completion =
                entities::work_completion::Entity::find_by_id(evidence.completion_id.clone())
                    .one(&tx)
                    .await?
                    .context("merge work completion not found")?;
            if work_unit.task_run_id != run.id
                || work_unit.agent_id.as_deref() != Some(merge.agent_id.as_str())
                || work_unit.status != WorkUnitStatus::Merging.as_str()
                || outcome.task_run_id != run.id
                || outcome.work_unit_id.as_deref() != Some(work_unit.id.as_str())
                || outcome.agent_id != merge.agent_id
                || outcome.status != AgentOutcomeStatus::Completed.as_str()
                || completion.task_run_id != run.id
                || completion.work_unit_id != work_unit.id
                || completion.executor_agent_id != merge.agent_id
                || completion.revision != i32::try_from(evidence.completion_revision)?
                || completion.status != WorkCompletionStatus::Approved.as_str()
                || evidence.delivery_head != merge.source_commit
            {
                bail!("approved executor completion identity changed before merge acceptance");
            }
            let completion = work_completion_record(completion)?;
            let delivery = delivery_from_completion(&completion)?;
            if delivery.head_commit != merge.source_commit {
                bail!("delivery source commit changed before merge acceptance");
            }

            let now = unix_seconds();
            evidence.verification_steps = input.verification_steps;
            evidence.merge_commit = Some(input.merge_commit.clone());
            let mut merge_active: entities::merge_record::ActiveModel = merge.into();
            merge_active.status = Set(MergeStatus::Merged.as_str().to_string());
            merge_active.verification_json = Set(Some(serde_json::to_string(&evidence)?));
            merge_active.updated_at = Set(now);
            let merge_model = merge_active.update(&tx).await?;

            let mut work_unit_active: entities::work_unit::ActiveModel = work_unit.into();
            work_unit_active.status = Set(WorkUnitStatus::Merged.as_str().to_string());
            work_unit_active.updated_at = Set(now);
            work_unit_active.update(&tx).await?;

            let mut run_active: entities::task_run::ActiveModel = run.into();
            run_active.phase = Set(evidence.origin_phase.as_str().to_string());
            run_active.expected_head = Set(input.merge_commit.clone());
            run_active.status_message = Set(None);
            run_active.updated_at = Set(now);
            run_active.update(&tx).await?;

            let mut lease_active: entities::branch_lease::ActiveModel = lease.into();
            lease_active.expected_head = Set(input.merge_commit);
            lease_active.updated_at = Set(now);
            lease_active.update(&tx).await?;
            merge_record(merge_model)
        }
        .await;
        match result {
            Ok(record) => {
                tx.commit().await?;
                Ok(record)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn fail_task_merge(&self, input: FailTaskMerge) -> Result<MergeRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let merge = entities::merge_record::Entity::find_by_id(input.merge_id)
                .one(&tx)
                .await?
                .context("merge record not found")?;
            if !matches!(
                MergeStatus::from_str(&merge.status),
                Some(MergeStatus::Pending | MergeStatus::Verifying)
            ) {
                bail!("only an active clean merge can fail");
            }
            let mut evidence = parse_required_evidence(merge.verification_json.as_deref())?;
            evidence.verification_steps = input.verification_steps;
            evidence.compensation = input.compensation;
            let now = unix_seconds();
            let task_run_id = merge.task_run_id.clone();
            let mut merge_active: entities::merge_record::ActiveModel = merge.into();
            merge_active.status = Set(MergeStatus::Failed.as_str().to_string());
            merge_active.resolution_summary = Set(Some(input.reason.clone()));
            merge_active.verification_json = Set(Some(serde_json::to_string(&evidence)?));
            merge_active.updated_at = Set(now);
            let merge_model = merge_active.update(&tx).await?;

            let run = entities::task_run::Entity::find_by_id(task_run_id.clone())
                .one(&tx)
                .await?
                .context("task run not found")?;
            if run.phase != TaskRunPhase::Merging.as_str() {
                bail!("task run left merging before failure persistence");
            }
            super::write_task_terminal_fact(
                &tx,
                run,
                TaskRunPhase::Blocked,
                Some(input.reason),
                None,
            )
            .await?;
            super::delete_blocked_branch_lease(&tx, &task_run_id).await?;
            merge_record(merge_model)
        }
        .await;
        match result {
            Ok(record) => {
                tx.commit().await?;
                Ok(record)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }

    pub(crate) async fn conflict_task_merge(
        &self,
        input: ConflictTaskMerge,
    ) -> Result<MergeRecord> {
        let tx = self.db.begin().await?;
        let result = async {
            let merge = entities::merge_record::Entity::find_by_id(input.merge_id)
                .one(&tx)
                .await?
                .context("merge record not found")?;
            if merge.status != MergeStatus::Pending.as_str() {
                bail!("only a pending merge can hand off conflicts");
            }
            let mut evidence = parse_required_evidence(merge.verification_json.as_deref())?;
            if input.manifest.merge_head != merge.source_commit
                || input.manifest.pre_index_tree != evidence.pre_index_tree
                || input.manifest.conflicts.is_empty()
            {
                bail!("conflict manifest does not match the active merge prestate");
            }
            let mut conflict_files = input
                .manifest
                .conflicts
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>();
            conflict_files.sort();
            conflict_files.dedup();
            evidence.conflict_manifest = Some(input.manifest);
            let now = unix_seconds();
            let task_run_id = merge.task_run_id.clone();
            let mut merge_active: entities::merge_record::ActiveModel = merge.into();
            merge_active.status = Set(MergeStatus::Conflicted.as_str().to_string());
            merge_active.conflict_files_json = Set(serde_json::to_string(&conflict_files)?);
            merge_active.verification_json = Set(Some(serde_json::to_string(&evidence)?));
            merge_active.updated_at = Set(now);
            let merge_model = merge_active.update(&tx).await?;
            let run = entities::task_run::Entity::find_by_id(task_run_id)
                .one(&tx)
                .await?
                .context("task run not found")?;
            if run.phase != TaskRunPhase::Merging.as_str() {
                bail!("task run left merging before conflict persistence");
            }
            let mut run_active: entities::task_run::ActiveModel = run.into();
            run_active.phase = Set(TaskRunPhase::ResolvingConflict.as_str().to_string());
            run_active.status_message = Set(Some(format!(
                "merge conflict requires planner resolution: {}",
                conflict_files.join(", ")
            )));
            run_active.updated_at = Set(now);
            run_active.update(&tx).await?;
            merge_record(merge_model)
        }
        .await;
        match result {
            Ok(record) => {
                tx.commit().await?;
                Ok(record)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }
}
