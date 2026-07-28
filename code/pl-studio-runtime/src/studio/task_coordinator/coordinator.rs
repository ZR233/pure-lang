use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tokio::sync::{MutexGuard, broadcast};

use super::git::{
    RepositorySnapshot, inspect_repository, inspect_worktree_changes, prepare_repository_for_task,
};
use super::{
    CreateTaskRun, TaskRunPhase, TaskRunRecord, TaskWorktreeOwnerSnapshot, WorkUnitRecord,
};
use crate::agent::worktree::{
    DurableWorktreeDisposition, DurableWorktreePresence, DurableWorktreeResource,
    DurableWorktreeResourcePresence, cleanup_task_worktree_resources,
    inspect_task_worktree_resources, validate_task_worktree_resource_identities,
};
use crate::studio::ids::new_id;
use crate::studio::runtime_state::{
    StudioRecoveryCleanupPreview, StudioRecoveryCleanupResource, StudioRecoveryIssue,
    StudioRecoveryIssueAction, StudioRecoveryIssueCategory, StudioRecoveryIssueScope,
    StudioRecoveryResourcePresence,
};
use crate::studio::store::StudioStore;

mod recovery;
use recovery::resolve_worktree_recovery_groups;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BranchKey {
    common_dir: String,
    branch: String,
}

struct WorktreeRecoveryGroup {
    repositories: Vec<PathBuf>,
    owners: Vec<TaskWorktreeOwnerSnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TaskRecoveryReport {
    pub(crate) recovered_runs: Vec<TaskRunRecord>,
    pub(crate) issues: Vec<StudioRecoveryIssue>,
}

impl std::ops::Deref for TaskRecoveryReport {
    type Target = [TaskRunRecord];

    fn deref(&self) -> &Self::Target {
        &self.recovered_runs
    }
}

impl IntoIterator for TaskRecoveryReport {
    type Item = TaskRunRecord;
    type IntoIter = std::vec::IntoIter<TaskRunRecord>;

    fn into_iter(self) -> Self::IntoIter {
        self.recovered_runs.into_iter()
    }
}

impl PartialEq<Vec<TaskRunRecord>> for TaskRecoveryReport {
    fn eq(&self, other: &Vec<TaskRunRecord>) -> bool {
        self.recovered_runs == *other
    }
}

impl BranchKey {
    fn new(common_dir: &Path, branch: &str) -> Self {
        let common_dir = common_dir.to_string_lossy().replace('\\', "/");
        Self {
            common_dir: if cfg!(windows) {
                common_dir.to_lowercase()
            } else {
                common_dir
            },
            branch: branch.to_string(),
        }
    }
}

/// 持久化 Task 模式事实并守护用户当前分支。
pub(crate) struct TaskCoordinator {
    pub(super) store: StudioStore,
    owned_process_leases: Mutex<HashMap<BranchKey, String>>,
    pub(super) allocation_lock: tokio::sync::Mutex<()>,
    pub(super) branch_mutation_lock: tokio::sync::Mutex<()>,
    branch_mutation_owner: Arc<()>,
    terminal_fact_tx: broadcast::Sender<String>,
    #[cfg(test)]
    pub(super) design_after_commit_barrier: Mutex<Option<super::design::DesignCommitTestBarrier>>,
    #[cfg(test)]
    pub(super) design_before_head_persist_barrier:
        Mutex<Option<super::design::DesignCommitTestBarrier>>,
    #[cfg(test)]
    pub(super) design_after_head_persist_barrier:
        Mutex<Option<super::design::DesignCommitTestBarrier>>,
    #[cfg(test)]
    pub(super) design_before_rollback_barrier:
        Mutex<Option<super::design::DesignCommitTestBarrier>>,
    #[cfg(test)]
    pub(super) fail_design_compensation: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    pub(super) merge_cleanup_barrier: Mutex<Option<super::merge::MergeCleanupTestBarrier>>,
    #[cfg(test)]
    pub(super) merge_after_commit_barrier: Mutex<Option<super::merge::MergeCommitTestBarrier>>,
    #[cfg(test)]
    pub(super) merge_before_proof_barrier: Mutex<Option<super::merge::MergeCommitTestBarrier>>,
    #[cfg(test)]
    pub(super) merge_after_acceptance_barrier: Mutex<Option<super::merge::MergeCommitTestBarrier>>,
    #[cfg(test)]
    pub(super) merge_after_abort_barrier: Mutex<Option<super::merge::MergeCommitTestBarrier>>,
    #[cfg(test)]
    pub(super) fail_merge_post_accept_read: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    pub(super) merge_failure_point: Mutex<Option<super::merge::MergeFailureTestPoint>>,
}

/// 持有期间串行化任务分支变更，并阻止 executor 基于中间 HEAD 分配。
pub(crate) struct BranchMutationGuard<'a> {
    owner: &'a Arc<()>,
    _guard: MutexGuard<'a, ()>,
}

impl TaskCoordinator {
    pub(crate) fn new(store: StudioStore) -> Self {
        let (terminal_fact_tx, _) = broadcast::channel(256);
        Self {
            store,
            owned_process_leases: Mutex::new(HashMap::new()),
            allocation_lock: tokio::sync::Mutex::new(()),
            branch_mutation_lock: tokio::sync::Mutex::new(()),
            branch_mutation_owner: Arc::new(()),
            terminal_fact_tx,
            #[cfg(test)]
            design_after_commit_barrier: Mutex::new(None),
            #[cfg(test)]
            design_before_head_persist_barrier: Mutex::new(None),
            #[cfg(test)]
            design_after_head_persist_barrier: Mutex::new(None),
            #[cfg(test)]
            design_before_rollback_barrier: Mutex::new(None),
            #[cfg(test)]
            fail_design_compensation: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            merge_cleanup_barrier: Mutex::new(None),
            #[cfg(test)]
            merge_after_commit_barrier: Mutex::new(None),
            #[cfg(test)]
            merge_before_proof_barrier: Mutex::new(None),
            #[cfg(test)]
            merge_after_acceptance_barrier: Mutex::new(None),
            #[cfg(test)]
            merge_after_abort_barrier: Mutex::new(None),
            #[cfg(test)]
            fail_merge_post_accept_read: std::sync::atomic::AtomicBool::new(false),
            #[cfg(test)]
            merge_failure_point: Mutex::new(None),
        }
    }

    pub(super) fn subscribe_terminal_facts(&self) -> broadcast::Receiver<String> {
        self.terminal_fact_tx.subscribe()
    }

    pub(super) fn publish_terminal_fact(&self, task_run_id: &str) {
        let _ = self.terminal_fact_tx.send(task_run_id.to_string());
    }

    pub(crate) async fn lock_branch_mutation(&self) -> BranchMutationGuard<'_> {
        BranchMutationGuard {
            owner: &self.branch_mutation_owner,
            _guard: self.branch_mutation_lock.lock().await,
        }
    }

    pub(super) fn ensure_branch_mutation_guard(
        &self,
        guard: &BranchMutationGuard<'_>,
    ) -> Result<()> {
        if Arc::ptr_eq(&self.branch_mutation_owner, guard.owner) {
            return Ok(());
        }
        bail!("branch mutation guard belongs to another coordinator")
    }

    pub(crate) async fn start_confirmed_task(
        &self,
        session_id: &str,
        plan: &str,
        repository: impl AsRef<Path>,
    ) -> Result<TaskRunRecord> {
        if plan.trim().is_empty() {
            bail!("task plan must not be empty");
        }
        let snapshot = prepare_repository_for_task(repository).await?;
        let key = BranchKey::new(&snapshot.git_common_dir, &snapshot.branch);
        let owner_token = new_id("task-owner");
        acquire_process_lease(&key, &owner_token)?;

        let result = self
            .store
            .create_task_run_with_lease(CreateTaskRun {
                session_id: session_id.to_string(),
                phase: TaskRunPhase::DesignUpdating,
                plan: plan.trim().to_string(),
                workspace_root: snapshot.workspace_root.to_string_lossy().to_string(),
                git_common_dir: snapshot.git_common_dir.to_string_lossy().to_string(),
                branch: snapshot.branch,
                head_commit: snapshot.head,
            })
            .await;
        let (run, _) = match result {
            Ok(result) => result,
            Err(error) => {
                release_process_lease(&key, &owner_token);
                return Err(error);
            }
        };
        replace_process_lease_owner(&key, &owner_token, &run.id)?;
        self.owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, run.id.clone());
        Ok(run)
    }

    pub(crate) async fn recover_active_tasks(&self) -> Result<TaskRecoveryReport> {
        let mut report = TaskRecoveryReport::default();
        let mut prepared = Vec::new();
        let mut failed_agent_runs = HashSet::new();
        for run in self.store.list_active_task_runs().await? {
            let key = BranchKey::new(Path::new(&run.git_common_dir), &run.branch);
            if let Err(error) = acquire_process_lease(&key, &run.id) {
                let message = error.to_string();
                self.block_run(&run, message.clone()).await?;
                self.push_recovery_issue(
                    &mut report,
                    &run,
                    StudioRecoveryIssueScope::Session,
                    StudioRecoveryIssueCategory::ProcessLease,
                    StudioRecoveryIssueAction::CleanupSession,
                    message,
                )
                .await?;
                continue;
            }
            self.owned_process_leases
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(key, run.id.clone());
            if let Err(error) = self
                .store
                .reconcile_task_agents_after_restart(&run.id)
                .await
            {
                let message = format!("agent restart reconciliation failed: {error}");
                self.block_run(&run, message.clone()).await?;
                self.push_recovery_issue(
                    &mut report,
                    &run,
                    StudioRecoveryIssueScope::Session,
                    StudioRecoveryIssueCategory::AgentState,
                    StudioRecoveryIssueAction::CleanupSession,
                    message,
                )
                .await?;
                failed_agent_runs.insert(run.id.clone());
                continue;
            }
            prepared.push(run);
        }

        let owners = self.store.list_all_task_worktree_owners().await?;
        let preflight = resolve_worktree_recovery_groups(owners).await;
        let mut failed_preflight_runs = HashSet::new();
        for failure in preflight.failures {
            for run in &failure.runs {
                failed_preflight_runs.insert(run.id.clone());
                if !run.phase.is_terminal() {
                    self.block_run(run, failure.message.clone()).await?;
                }
                self.push_recovery_issue(
                    &mut report,
                    run,
                    StudioRecoveryIssueScope::Project,
                    StudioRecoveryIssueCategory::Repository,
                    StudioRecoveryIssueAction::RemoveProject,
                    failure.message.clone(),
                )
                .await?;
            }
        }
        let groups = preflight.groups;
        let run_groups = preflight.run_groups;
        let skipped_groups = failed_agent_runs
            .iter()
            .filter_map(|run_id| run_groups.get(run_id).cloned())
            .collect::<HashSet<_>>();
        let mut successful_groups = HashSet::new();
        for (key, group) in groups {
            if skipped_groups.contains(&key) {
                continue;
            }
            if let Err(error) = self
                .reconcile_durable_worktrees(&group.repositories, &group.owners)
                .await
            {
                let message = format!("worktree restart reconciliation failed: {error}");
                let affected = prepared
                    .iter()
                    .filter(|run| run_groups.get(&run.id) == Some(&key))
                    .cloned()
                    .collect::<Vec<_>>();
                for run in &affected {
                    self.block_run(run, message.clone()).await?;
                }
                for owner in &group.owners {
                    self.push_recovery_issue(
                        &mut report,
                        &owner.run,
                        StudioRecoveryIssueScope::Session,
                        StudioRecoveryIssueCategory::Worktree,
                        StudioRecoveryIssueAction::CleanupSession,
                        message.clone(),
                    )
                    .await?;
                }
                continue;
            }
            successful_groups.insert(key);
        }

        for run in prepared {
            if failed_preflight_runs.contains(&run.id) {
                continue;
            }
            if !run_groups
                .get(&run.id)
                .is_some_and(|group| successful_groups.contains(group))
            {
                continue;
            }
            if run.phase == TaskRunPhase::Merging {
                match self.recover_merging_run(&run).await {
                    Ok(super::merge::MergeRestartRecovery::Resume(recovered_run)) => {
                        report.recovered_runs.push(*recovered_run);
                    }
                    Ok(super::merge::MergeRestartRecovery::Blocked) => {
                        self.push_recovery_issue(
                            &mut report,
                            &run,
                            StudioRecoveryIssueScope::Session,
                            StudioRecoveryIssueCategory::Merge,
                            StudioRecoveryIssueAction::CleanupSession,
                            "merge recovery blocked the task".to_string(),
                        )
                        .await?;
                    }
                    Err(error) => {
                        let message = format!("merge recovery failed: {error}");
                        self.block_run(&run, message.clone()).await?;
                        self.push_recovery_issue(
                            &mut report,
                            &run,
                            StudioRecoveryIssueScope::Session,
                            StudioRecoveryIssueCategory::Merge,
                            StudioRecoveryIssueAction::CleanupSession,
                            message,
                        )
                        .await?;
                    }
                }
                continue;
            }
            let resolving_conflict = run.phase == TaskRunPhase::ResolvingConflict;
            let snapshot = match inspect_repository(&run.workspace_root, !resolving_conflict).await
            {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    let message = format!("repository recovery failed: {error}");
                    self.block_run(&run, message.clone()).await?;
                    self.push_recovery_issue(
                        &mut report,
                        &run,
                        StudioRecoveryIssueScope::Project,
                        StudioRecoveryIssueCategory::Repository,
                        StudioRecoveryIssueAction::RemoveProject,
                        message,
                    )
                    .await?;
                    continue;
                }
            };
            if let Err(reason) = validate_snapshot(&run, &snapshot) {
                let message = reason.to_string();
                self.block_run(&run, message.clone()).await?;
                self.push_recovery_issue(
                    &mut report,
                    &run,
                    StudioRecoveryIssueScope::Project,
                    StudioRecoveryIssueCategory::Repository,
                    StudioRecoveryIssueAction::RemoveProject,
                    message,
                )
                .await?;
                continue;
            }
            if resolving_conflict {
                let records = self.store.list_merge_records(&run.id).await?;
                let conflicted = records
                    .iter()
                    .filter(|record| record.status == super::MergeStatus::Conflicted)
                    .collect::<Vec<_>>();
                let result = match conflicted.as_slice() {
                    [record] => super::merge::validate_conflict_recovery(&run, record).await,
                    [] => Err(anyhow::anyhow!(
                        "resolving-conflict run has no conflicted merge record"
                    )),
                    _ => Err(anyhow::anyhow!(
                        "resolving-conflict run has multiple conflicted merge records"
                    )),
                };
                if let Err(error) = result {
                    let message = format!("conflict recovery failed: {error}");
                    self.block_run(&run, message.clone()).await?;
                    self.push_recovery_issue(
                        &mut report,
                        &run,
                        StudioRecoveryIssueScope::Session,
                        StudioRecoveryIssueCategory::Conflict,
                        StudioRecoveryIssueAction::CleanupSession,
                        message,
                    )
                    .await?;
                    continue;
                }
            }
            report.recovered_runs.push(run);
        }
        Ok(report)
    }

    async fn push_recovery_issue(
        &self,
        report: &mut TaskRecoveryReport,
        run: &TaskRunRecord,
        scope: StudioRecoveryIssueScope,
        category: StudioRecoveryIssueCategory,
        action: StudioRecoveryIssueAction,
        message: String,
    ) -> Result<()> {
        let session = self.store.read_session(&run.session_id).await?;
        let project_id = session.as_ref().map(|session| session.project_id.clone());
        let category_key = match category {
            StudioRecoveryIssueCategory::ProcessLease => "process-lease",
            StudioRecoveryIssueCategory::AgentState => "agent-state",
            StudioRecoveryIssueCategory::Worktree => "worktree",
            StudioRecoveryIssueCategory::Repository => "repository",
            StudioRecoveryIssueCategory::Merge => "merge",
            StudioRecoveryIssueCategory::Conflict => "conflict",
        };
        let id = format!("recovery-issue-{category_key}-{}", run.id);
        if report.issues.iter().any(|issue| issue.id == id) {
            return Ok(());
        }
        report.issues.push(StudioRecoveryIssue {
            id,
            scope,
            category,
            action,
            project_id,
            session_id: Some(run.session_id.clone()),
            task_run_id: Some(run.id.clone()),
            message,
        });
        Ok(())
    }

    pub(crate) async fn preview_recovery_cleanup(
        &self,
        issue: &StudioRecoveryIssue,
    ) -> Result<StudioRecoveryCleanupPreview> {
        if !matches!(
            issue.action,
            StudioRecoveryIssueAction::CleanupSession | StudioRecoveryIssueAction::RemoveProject
        ) {
            bail!("recovery issue does not authorize destructive cleanup");
        }
        let task_run_id = issue
            .task_run_id
            .as_deref()
            .context("recovery issue has no task run")?;
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("recovery cleanup task run not found")?;
        let work_units = self.store.list_work_units(task_run_id).await?;
        let durable = work_units
            .iter()
            .map(|unit| DurableWorktreeResource {
                task_run_id: task_run_id.to_string(),
                path: unit.worktree_path.clone().into(),
                branch: unit.branch.clone(),
                expected_head: None,
                presence: DurableWorktreePresence::MayBeUncreated,
                disposition: DurableWorktreeDisposition::Cleanup,
            })
            .collect::<Vec<_>>();
        validate_task_worktree_resource_identities(&run.workspace_root, &durable)?;
        let inspections = match inspect_task_worktree_resources(&run.workspace_root, &durable).await
        {
            Ok(inspections) => Some(inspections),
            Err(_) if issue.scope == StudioRecoveryIssueScope::Project => None,
            Err(error) => return Err(error),
        };
        let mut resources = Vec::with_capacity(work_units.len());
        for (index, unit) in work_units.iter().enumerate() {
            let inspection = inspections
                .as_ref()
                .and_then(|inspections| inspections.get(index));
            let changes = if inspection.is_some_and(|inspection| inspection.path_exists) {
                inspect_worktree_changes(&unit.worktree_path, &unit.base_commit)
                    .await
                    .ok()
            } else {
                None
            };
            resources.push(StudioRecoveryCleanupResource {
                work_unit_id: unit.id.clone(),
                path: unit.worktree_path.clone(),
                branch: unit.branch.clone(),
                presence: match inspection.map(|inspection| inspection.presence) {
                    Some(DurableWorktreeResourcePresence::Absent) => {
                        StudioRecoveryResourcePresence::Absent
                    }
                    Some(DurableWorktreeResourcePresence::Complete) => {
                        StudioRecoveryResourcePresence::Complete
                    }
                    Some(DurableWorktreeResourcePresence::Partial) | None => {
                        StudioRecoveryResourcePresence::Partial
                    }
                },
                registration_exists: inspection
                    .is_some_and(|inspection| inspection.registration_exists),
                path_exists: inspection.is_some_and(|inspection| inspection.path_exists),
                branch_exists: inspection.is_some_and(|inspection| inspection.branch_exists),
                branch_head: inspection.and_then(|inspection| inspection.branch_head.clone()),
                dirty: changes.as_ref().is_some_and(|changes| changes.dirty),
                ahead_by: changes.as_ref().map_or(0, |changes| changes.ahead_by),
                changed_file_count: changes
                    .as_ref()
                    .map_or(0, |changes| changes.changed_file_count),
            });
        }
        let expected_revision = recovery_cleanup_revision(issue, &run, &work_units, &resources);
        Ok(StudioRecoveryCleanupPreview {
            issue_id: issue.id.clone(),
            expected_revision,
            scope: issue.scope,
            project_id: issue.project_id.clone(),
            session_id: issue.session_id.clone(),
            message: issue.message.clone(),
            resources,
        })
    }

    pub(crate) async fn cleanup_recovery_issue(
        &self,
        issue: &StudioRecoveryIssue,
        expected_revision: &str,
    ) -> Result<()> {
        if !matches!(
            issue.action,
            StudioRecoveryIssueAction::CleanupSession | StudioRecoveryIssueAction::RemoveProject
        ) {
            bail!("recovery issue does not authorize destructive cleanup");
        }
        let preview = self.preview_recovery_cleanup(issue).await?;
        if preview.expected_revision != expected_revision {
            bail!("recovery cleanup state changed; refresh the preview before confirming");
        }
        let task_run_id = issue
            .task_run_id
            .as_deref()
            .context("recovery issue has no task run")?;
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("recovery cleanup task run not found")?;
        let work_units = self.store.list_work_units(task_run_id).await?;
        if issue.action == StudioRecoveryIssueAction::RemoveProject {
            self.store.authorize_recovery_cleanup(task_run_id).await?;
            self.release_owned_process_lease(task_run_id);
            return Ok(());
        }
        let durable = work_units
            .iter()
            .map(|unit| DurableWorktreeResource {
                task_run_id: task_run_id.to_string(),
                path: unit.worktree_path.clone().into(),
                branch: unit.branch.clone(),
                expected_head: preview
                    .resources
                    .iter()
                    .find(|resource| resource.work_unit_id == unit.id)
                    .and_then(|resource| resource.branch_head.clone()),
                presence: DurableWorktreePresence::MayBeUncreated,
                disposition: DurableWorktreeDisposition::Cleanup,
            })
            .collect::<Vec<_>>();
        validate_task_worktree_resource_identities(&run.workspace_root, &durable)?;
        self.store.authorize_recovery_cleanup(task_run_id).await?;
        cleanup_task_worktree_resources(&run.workspace_root, &durable).await?;
        self.release_owned_process_lease(task_run_id);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn verify_expected_head(&self, task_run_id: &str) -> Result<bool> {
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("task run not found")?;
        let snapshot = match inspect_repository(&run.workspace_root, true).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.block_run(&run, format!("repository verification failed: {error}"))
                    .await?;
                return Ok(false);
            }
        };
        if let Err(reason) = validate_snapshot(&run, &snapshot) {
            self.block_run(&run, reason.to_string()).await?;
            return Ok(false);
        }
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) async fn finish_task(
        &self,
        task_run_id: &str,
        phase: TaskRunPhase,
        status_message: Option<String>,
    ) -> Result<TaskRunRecord> {
        if !matches!(
            phase,
            TaskRunPhase::Completed | TaskRunPhase::Failed | TaskRunPhase::Cancelled
        ) {
            bail!("finish_task requires a terminal phase");
        }
        let run = self
            .store
            .transition_task_run(task_run_id, phase, status_message)
            .await?;
        self.store.release_branch_lease(task_run_id).await?;
        self.release_owned_process_lease(task_run_id);
        Ok(run)
    }

    pub(crate) fn suspend(&self) {
        let mut owned = self
            .owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (key, owner) in owned.drain() {
            release_process_lease(&key, &owner);
        }
    }

    pub(crate) async fn block_continuation_failure(
        &self,
        task_run_id: &str,
        reason: String,
    ) -> Result<()> {
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("task run not found while blocking continuation failure")?;
        if !run.phase.is_terminal() {
            self.block_run(&run, reason).await?;
        }
        Ok(())
    }

    pub(super) async fn block_run(&self, run: &TaskRunRecord, reason: String) -> Result<()> {
        self.store
            .block_task_and_release_lease(&run.id, &reason)
            .await?;
        self.release_owned_process_lease(&run.id);
        release_process_lease(
            &BranchKey::new(Path::new(&run.git_common_dir), &run.branch),
            &run.id,
        );
        Ok(())
    }

    pub(super) fn ensure_process_lease_owned(&self, run: &TaskRunRecord) -> Result<()> {
        let key = BranchKey::new(Path::new(&run.git_common_dir), &run.branch);
        let locally_owned = self
            .owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .is_some_and(|owner| owner == &run.id);
        let globally_owned = process_leases()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&key)
            .is_some_and(|owner| owner == &run.id);
        if !locally_owned || !globally_owned {
            bail!("task process branch lease is not owned by this coordinator");
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn process_lease_is_held(&self, run: &TaskRunRecord) -> bool {
        process_leases()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&BranchKey::new(Path::new(&run.git_common_dir), &run.branch))
            .is_some_and(|owner| owner == &run.id)
    }

    pub(super) fn release_owned_process_lease(&self, task_run_id: &str) {
        let mut owned = self
            .owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = owned
            .iter()
            .find_map(|(key, owner)| (owner == task_run_id).then(|| key.clone()));
        if let Some(key) = key {
            owned.remove(&key);
            release_process_lease(&key, task_run_id);
        }
    }
}

impl Drop for TaskCoordinator {
    fn drop(&mut self) {
        self.suspend();
    }
}

fn recovery_cleanup_revision(
    issue: &StudioRecoveryIssue,
    run: &TaskRunRecord,
    work_units: &[WorkUnitRecord],
    resources: &[StudioRecoveryCleanupResource],
) -> String {
    let mut digest = Sha256::new();
    digest.update(issue.id.as_bytes());
    digest.update(run.id.as_bytes());
    digest.update(run.updated_at.to_le_bytes());
    for (unit, resource) in work_units.iter().zip(resources) {
        digest.update(unit.id.as_bytes());
        digest.update(unit.status.as_str().as_bytes());
        digest.update(unit.worktree_disposition.as_str().as_bytes());
        digest.update(resource.path.as_bytes());
        digest.update(resource.branch.as_bytes());
        digest.update([resource.registration_exists as u8]);
        digest.update([resource.path_exists as u8]);
        digest.update([resource.branch_exists as u8]);
        digest.update(
            resource
                .branch_head
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        );
        digest.update([resource.dirty as u8]);
        digest.update(resource.ahead_by.to_le_bytes());
        digest.update(resource.changed_file_count.to_le_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn validate_snapshot(run: &TaskRunRecord, snapshot: &RepositorySnapshot) -> Result<()> {
    let expected_key = BranchKey::new(Path::new(&run.git_common_dir), &run.branch);
    let actual_key = BranchKey::new(&snapshot.git_common_dir, &snapshot.branch);
    if expected_key != actual_key {
        bail!("task branch changed outside the coordinator");
    }
    if snapshot.head != run.expected_head {
        bail!(
            "task HEAD drifted: expected {}, actual {}",
            run.expected_head,
            snapshot.head
        );
    }
    Ok(())
}

fn process_leases() -> &'static Mutex<HashMap<BranchKey, String>> {
    static LEASES: OnceLock<Mutex<HashMap<BranchKey, String>>> = OnceLock::new();
    LEASES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn acquire_process_lease(key: &BranchKey, owner: &str) -> Result<()> {
    let mut leases = process_leases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = leases.get(key) {
        bail!("branch is already owned by task {existing}");
    }
    leases.insert(key.clone(), owner.to_string());
    Ok(())
}

fn replace_process_lease_owner(key: &BranchKey, current: &str, next: &str) -> Result<()> {
    let mut leases = process_leases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let owner = leases
        .get_mut(key)
        .context("process branch lease disappeared")?;
    if owner != current {
        bail!("process branch lease owner changed unexpectedly");
    }
    *owner = next.to_string();
    Ok(())
}

fn release_process_lease(key: &BranchKey, owner: &str) {
    let mut leases = process_leases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if leases.get(key).is_some_and(|current| current == owner) {
        leases.remove(key);
    }
}
