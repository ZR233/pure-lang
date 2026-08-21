use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tokio::sync::{MutexGuard, broadcast};

use super::git::{
    RepositorySnapshot, fingerprint_repository, inspect_repository, inspect_worktree_changes,
    prepare_repository_for_task,
};
use super::recovery::{
    MergingRecovery, inspect_merging_recovery, is_retryable_merge_recovery_message,
    validate_snapshot_owner,
};
use super::{
    CreateTaskRun, RecordTaskAgentFailure, TaskRunRecord, TaskRunStateKind,
    TaskWorktreeOwnerSnapshot, WorkUnitRecord,
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
use crate::{AgentLifecycleState, AgentRuntimeHandle};

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

struct RecoveryCleanupRun {
    run: TaskRunRecord,
    work_units: Vec<WorkUnitRecord>,
}

struct RecoveryCleanupScope {
    project_updated_at: Option<i64>,
    thread_updated_at: Option<i64>,
    runs: Vec<RecoveryCleanupRun>,
}

pub(crate) struct RecoveryCleanupAuthorization {
    preview: StudioRecoveryCleanupPreview,
    scope: RecoveryCleanupScope,
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
}

/// 持有期间串行化任务分支变更，并阻止 executor 基于中间 HEAD 分配。
pub(crate) struct BranchMutationGuard<'a> {
    owner: &'a Arc<()>,
    _guard: MutexGuard<'a, ()>,
}

impl TaskCoordinator {
    pub(in crate::studio) async fn handle_agent_turn_failure(
        &self,
        input: RecordTaskAgentFailure,
        runtime: &AgentRuntimeHandle,
    ) -> Result<bool> {
        let root_thread_id = input.root_thread_id.clone();
        let source_agent_id = input.source_agent_id.clone();
        let Some(settlement) = self.store.record_task_agent_failure(input).await? else {
            return Ok(false);
        };
        if !settlement.terminalized {
            return Ok(false);
        }

        let root = crate::studio::agent_host::root_agent_id(&root_thread_id);
        let snapshots = runtime
            .list()
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        for snapshot in snapshots {
            if snapshot.identity.id == root {
                if snapshot.identity.id.as_str() != source_agent_id
                    && let Some(turn_id) = snapshot.active_turn_id
                {
                    runtime
                        .cancel_turn(snapshot.identity.id, turn_id)
                        .await
                        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                }
            } else if snapshot.identity.parent_id.as_ref() == Some(&root)
                && !matches!(
                    snapshot.lifecycle,
                    AgentLifecycleState::Closing | AgentLifecycleState::Closed
                )
            {
                runtime
                    .close(snapshot.identity.id)
                    .await
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            }
        }
        self.release_owned_process_lease(&settlement.run.id);
        Ok(true)
    }

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
        }
    }

    pub(in crate::studio) fn subscribe_terminal_facts(&self) -> broadcast::Receiver<String> {
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

    pub(crate) fn ensure_branch_mutation_guard(
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
        root_thread_id: &str,
        plan: &str,
        repository: impl AsRef<Path>,
    ) -> Result<TaskRunRecord> {
        if plan.trim().is_empty() {
            bail!("task plan must not be empty");
        }
        let snapshot = prepare_repository_for_task(repository).await?;
        let design_baseline =
            fingerprint_repository(&snapshot.workspace_root, &snapshot.head, &snapshot.head)
                .await?;
        let key = BranchKey::new(&snapshot.git_common_dir, &snapshot.branch);
        let owner_token = new_id("task-owner");
        acquire_process_lease(&key, &owner_token)?;

        let result = self
            .store
            .create_task_run_with_lease(CreateTaskRun {
                root_thread_id: root_thread_id.to_string(),
                plan: plan.trim().to_string(),
                workspace_root: snapshot.workspace_root.to_string_lossy().to_string(),
                git_common_dir: snapshot.git_common_dir.to_string_lossy().to_string(),
                branch: snapshot.branch,
                head_commit: snapshot.head,
                design_baseline,
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
                    StudioRecoveryIssueScope::Thread,
                    StudioRecoveryIssueCategory::ProcessLease,
                    StudioRecoveryIssueAction::CleanupThread,
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
                    StudioRecoveryIssueScope::Thread,
                    StudioRecoveryIssueCategory::AgentState,
                    StudioRecoveryIssueAction::CleanupThread,
                    message,
                )
                .await?;
                failed_agent_runs.insert(run.id.clone());
                continue;
            }
            let run = self
                .store
                .read_task_run(&run.id)
                .await?
                .context("task run disappeared after agent restart reconciliation")?;
            if run.kind() == TaskRunStateKind::Stopping {
                let reason = run
                    .stop_reason()
                    .map_or("task stop resumed after restart", |reason| reason.as_str());
                if let Err(error) = self
                    .store
                    .settle_agents_for_task_stop(&run.id, run.generation(), reason)
                    .await
                {
                    let message = format!("task stop recovery settlement failed: {error}");
                    self.block_run(&run, message.clone()).await?;
                    self.push_recovery_issue(
                        &mut report,
                        &run,
                        StudioRecoveryIssueScope::Thread,
                        StudioRecoveryIssueCategory::AgentState,
                        StudioRecoveryIssueAction::CleanupThread,
                        message,
                    )
                    .await?;
                    failed_agent_runs.insert(run.id.clone());
                    continue;
                }
            }
            prepared.push(run);
        }

        let owners = self.store.list_all_task_worktree_owners().await?;
        let preflight = resolve_worktree_recovery_groups(owners).await;
        let mut failed_preflight_runs = HashSet::new();
        for failure in preflight.failures {
            for run in &failure.runs {
                failed_preflight_runs.insert(run.id.clone());
                if !run.kind().is_terminal() {
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
                        StudioRecoveryIssueScope::Thread,
                        StudioRecoveryIssueCategory::Worktree,
                        StudioRecoveryIssueAction::CleanupThread,
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
            if run.kind() == TaskRunStateKind::Merging {
                match inspect_merging_recovery(&run).await {
                    MergingRecovery::Resume => report.recovered_runs.push(run),
                    MergingRecovery::Retry(message) => {
                        self.block_run(&run, message.clone()).await?;
                        self.push_recovery_issue(
                            &mut report,
                            &run,
                            StudioRecoveryIssueScope::Thread,
                            StudioRecoveryIssueCategory::Merge,
                            StudioRecoveryIssueAction::Retry,
                            message,
                        )
                        .await?;
                    }
                }
                continue;
            }
            let snapshot = match inspect_repository(&run.workspace_root, true).await {
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
            if run.kind() == TaskRunStateKind::Stopping {
                let reason = run
                    .stop_reason()
                    .map_or("task stop resumed after restart", |reason| reason.as_str());
                let guard = self.lock_branch_mutation().await;
                if let Err(error) = self
                    .stop_task_locked(&run.id, run.generation(), reason, &guard)
                    .await
                {
                    drop(guard);
                    let message = format!("task stop recovery failed: {error}");
                    self.block_run(&run, message.clone()).await?;
                    self.push_recovery_issue(
                        &mut report,
                        &run,
                        StudioRecoveryIssueScope::Thread,
                        StudioRecoveryIssueCategory::AgentState,
                        StudioRecoveryIssueAction::CleanupThread,
                        message,
                    )
                    .await?;
                }
                continue;
            }
            report.recovered_runs.push(run);
        }
        for run in self.store.list_retryable_blocked_merge_task_runs().await? {
            let message = run
                .status_message()
                .map(str::to_string)
                .context("retryable merge recovery task is missing its diagnostic")?;
            self.push_recovery_issue(
                &mut report,
                &run,
                StudioRecoveryIssueScope::Thread,
                StudioRecoveryIssueCategory::Merge,
                StudioRecoveryIssueAction::Retry,
                message,
            )
            .await?;
        }
        Ok(report)
    }

    pub(crate) async fn retry_recovery_issue(
        &self,
        issue: &StudioRecoveryIssue,
    ) -> Result<TaskRunRecord> {
        if issue.action != StudioRecoveryIssueAction::Retry
            || issue.category != StudioRecoveryIssueCategory::Merge
        {
            bail!("recovery issue does not authorize merge reconciliation");
        }
        let task_run_id = issue
            .task_run_id
            .as_deref()
            .context("merge recovery issue has no task run")?;
        let run = self
            .store
            .read_task_run(task_run_id)
            .await?
            .context("merge recovery task run not found")?;
        if run.kind() != TaskRunStateKind::Blocked
            || run.root_thread_id != issue.thread_id.as_deref().unwrap_or_default()
            || run.status_message() != Some(issue.message.as_str())
            || !is_retryable_merge_recovery_message(&issue.message)
        {
            bail!("merge recovery state changed before retry");
        }
        let snapshot = inspect_repository(&run.workspace_root, false)
            .await
            .context("merge recovery could not inspect the canonical repository")?;
        validate_snapshot_owner(&run, &snapshot)?;

        let key = BranchKey::new(Path::new(&run.git_common_dir), &run.branch);
        acquire_process_lease(&key, &run.id)?;
        let retried = match self.store.retry_blocked_merge_task(&run).await {
            Ok(retried) => retried,
            Err(error) => {
                release_process_lease(&key, &run.id);
                return Err(error);
            }
        };
        self.owned_process_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key, retried.id.clone());
        Ok(retried)
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
        let session = self.store.read_thread(&run.root_thread_id).await?;
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
            thread_id: Some(run.root_thread_id.clone()),
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
            StudioRecoveryIssueAction::CleanupThread | StudioRecoveryIssueAction::RemoveProject
        ) {
            bail!("recovery issue does not authorize destructive cleanup");
        }
        let scope = self.recovery_cleanup_scope(issue).await?;
        self.recovery_cleanup_preview_for_scope(issue, &scope).await
    }

    pub(crate) async fn project_cleanup_issue(
        &self,
        project_id: &str,
    ) -> Result<StudioRecoveryIssue> {
        let project = self
            .store
            .read_project(project_id)
            .await?
            .context("project cleanup target not found")?;
        Ok(StudioRecoveryIssue {
            id: format!("project-cleanup-{}", project.id),
            scope: StudioRecoveryIssueScope::Project,
            category: StudioRecoveryIssueCategory::Worktree,
            action: StudioRecoveryIssueAction::RemoveProject,
            project_id: Some(project.id),
            thread_id: None,
            task_run_id: None,
            message: format!(
                "Remove {} from Studio and discard its Pure-owned task worktrees.",
                project.name
            ),
        })
    }

    pub(crate) async fn preview_project_cleanup(
        &self,
        project_id: &str,
    ) -> Result<StudioRecoveryCleanupPreview> {
        let issue = self.project_cleanup_issue(project_id).await?;
        self.preview_recovery_cleanup(&issue).await
    }

    pub(crate) async fn validate_recovery_cleanup(
        &self,
        issue: &StudioRecoveryIssue,
        expected_revision: &str,
    ) -> Result<RecoveryCleanupAuthorization> {
        let scope = self.recovery_cleanup_scope(issue).await?;
        let preview = self
            .recovery_cleanup_preview_for_scope(issue, &scope)
            .await?;
        if preview.expected_revision != expected_revision {
            bail!("recovery cleanup state changed; refresh the preview before confirming");
        }
        Ok(RecoveryCleanupAuthorization { preview, scope })
    }

    pub(crate) async fn execute_recovery_cleanup(
        &self,
        issue: &StudioRecoveryIssue,
        authorization: &RecoveryCleanupAuthorization,
    ) -> Result<()> {
        let scope = self.recovery_cleanup_scope(issue).await?;
        if !recovery_cleanup_scope_matches(&authorization.scope, &scope) {
            bail!("recovery cleanup state changed; refresh the preview before confirming");
        }
        let current_resources = self
            .collect_recovery_cleanup_resources(issue, &scope)
            .await?;
        if current_resources != authorization.preview.resources {
            bail!("recovery cleanup state changed; refresh the preview before confirming");
        }

        let preview_by_work_unit = authorization
            .preview
            .resources
            .iter()
            .map(|resource| (resource.work_unit_id.as_str(), resource))
            .collect::<HashMap<_, _>>();
        let mut durable_by_workspace = BTreeMap::<String, Vec<DurableWorktreeResource>>::new();
        for cleanup in &scope.runs {
            let durable = durable_by_workspace
                .entry(cleanup.run.workspace_root.clone())
                .or_default();
            durable.extend(cleanup.work_units.iter().map(|unit| {
                DurableWorktreeResource {
                    task_run_id: cleanup.run.id.clone(),
                    path: unit.worktree_path.clone().into(),
                    branch: unit.branch.clone(),
                    expected_head: preview_by_work_unit
                        .get(unit.id.as_str())
                        .and_then(|resource| resource.branch_head.clone()),
                    presence: DurableWorktreePresence::MayBeUncreated,
                    disposition: DurableWorktreeDisposition::Cleanup,
                }
            }));
        }
        for (workspace_root, durable) in &durable_by_workspace {
            validate_task_worktree_resource_identities(workspace_root, durable)?;
        }
        for cleanup in &scope.runs {
            self.store
                .authorize_recovery_cleanup(&cleanup.run.id)
                .await?;
            self.release_owned_process_lease(&cleanup.run.id);
        }
        for (workspace_root, durable) in durable_by_workspace {
            cleanup_task_worktree_resources(workspace_root, &durable).await?;
        }
        Ok(())
    }

    async fn recovery_cleanup_scope(
        &self,
        issue: &StudioRecoveryIssue,
    ) -> Result<RecoveryCleanupScope> {
        let (project_updated_at, thread_updated_at, runs) = match issue.action {
            StudioRecoveryIssueAction::CleanupThread => {
                let thread_id = issue
                    .thread_id
                    .as_deref()
                    .context("recovery issue has no Thread")?;
                let thread = self
                    .store
                    .read_thread(thread_id)
                    .await?
                    .context("recovery cleanup Thread not found")?;
                let runs = if let Some(task_run_id) = issue.task_run_id.as_deref() {
                    vec![
                        self.store
                            .read_task_run(task_run_id)
                            .await?
                            .context("recovery cleanup task run not found")?,
                    ]
                } else {
                    Vec::new()
                };
                (None, Some(thread.updated_at), runs)
            }
            StudioRecoveryIssueAction::RemoveProject => {
                let project_id = issue
                    .project_id
                    .as_deref()
                    .context("project recovery cleanup has no project")?;
                let project = self
                    .store
                    .read_project(project_id)
                    .await?
                    .context("project recovery cleanup target not found")?;
                (
                    Some(project.updated_at),
                    None,
                    self.store.list_task_runs_for_project(project_id).await?,
                )
            }
            StudioRecoveryIssueAction::Retry => {
                bail!("recovery issue does not authorize destructive cleanup")
            }
        };
        let mut cleanup_runs = Vec::with_capacity(runs.len());
        for run in runs {
            let work_units = self.store.list_work_units(&run.id).await?;
            cleanup_runs.push(RecoveryCleanupRun { run, work_units });
        }
        Ok(RecoveryCleanupScope {
            project_updated_at,
            thread_updated_at,
            runs: cleanup_runs,
        })
    }

    async fn recovery_cleanup_preview_for_scope(
        &self,
        issue: &StudioRecoveryIssue,
        scope: &RecoveryCleanupScope,
    ) -> Result<StudioRecoveryCleanupPreview> {
        let resources = self
            .collect_recovery_cleanup_resources(issue, scope)
            .await?;
        let expected_revision = recovery_cleanup_revision(issue, scope, &resources);
        Ok(StudioRecoveryCleanupPreview {
            issue_id: issue.id.clone(),
            expected_revision,
            scope: issue.scope,
            project_id: issue.project_id.clone(),
            thread_id: issue.thread_id.clone(),
            message: issue.message.clone(),
            resources,
        })
    }

    async fn collect_recovery_cleanup_resources(
        &self,
        issue: &StudioRecoveryIssue,
        scope: &RecoveryCleanupScope,
    ) -> Result<Vec<StudioRecoveryCleanupResource>> {
        let resource_capacity = scope
            .runs
            .iter()
            .map(|cleanup| cleanup.work_units.len())
            .sum();
        let mut resources = Vec::with_capacity(resource_capacity);
        for cleanup in &scope.runs {
            let durable = cleanup
                .work_units
                .iter()
                .map(|unit| DurableWorktreeResource {
                    task_run_id: cleanup.run.id.clone(),
                    path: unit.worktree_path.clone().into(),
                    branch: unit.branch.clone(),
                    expected_head: None,
                    presence: DurableWorktreePresence::MayBeUncreated,
                    disposition: DurableWorktreeDisposition::Cleanup,
                })
                .collect::<Vec<_>>();
            validate_task_worktree_resource_identities(&cleanup.run.workspace_root, &durable)?;
            let inspections = match inspect_task_worktree_resources(
                &cleanup.run.workspace_root,
                &durable,
            )
            .await
            {
                Ok(inspections) => Some(inspections),
                Err(_) if issue.scope == StudioRecoveryIssueScope::Project => None,
                Err(error) => return Err(error),
            };
            for (index, unit) in cleanup.work_units.iter().enumerate() {
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
        }
        Ok(resources)
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
        phase: TaskRunStateKind,
        status_message: Option<String>,
    ) -> Result<TaskRunRecord> {
        if !matches!(
            phase,
            TaskRunStateKind::Completed | TaskRunStateKind::Failed | TaskRunStateKind::Cancelled
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
        if !run.kind().is_terminal() {
            self.block_run(&run, reason).await?;
        }
        Ok(())
    }

    pub(super) async fn block_run(&self, run: &TaskRunRecord, reason: String) -> Result<()> {
        let blocked = self
            .store
            .block_task_and_release_lease(&run.id, &reason)
            .await?;
        self.publish_blocked_terminal(&blocked)?;
        Ok(())
    }

    fn publish_blocked_terminal(&self, run: &TaskRunRecord) -> Result<()> {
        if run.kind() != TaskRunStateKind::Blocked {
            bail!("blocked task fact is not canonical");
        }
        self.release_owned_process_lease(&run.id);
        release_process_lease(
            &BranchKey::new(Path::new(&run.git_common_dir), &run.branch),
            &run.id,
        );
        self.publish_terminal_fact(&run.id);
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

fn recovery_cleanup_scope_matches(
    authorized: &RecoveryCleanupScope,
    current: &RecoveryCleanupScope,
) -> bool {
    if authorized.project_updated_at != current.project_updated_at
        || authorized.thread_updated_at != current.thread_updated_at
    {
        return false;
    }
    let authorized_runs = authorized
        .runs
        .iter()
        .map(|cleanup| {
            (
                cleanup.run.id.as_str(),
                cleanup.run.root_thread_id.as_str(),
                cleanup.run.workspace_root.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    let current_runs = current
        .runs
        .iter()
        .map(|cleanup| {
            (
                cleanup.run.id.as_str(),
                cleanup.run.root_thread_id.as_str(),
                cleanup.run.workspace_root.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    if authorized_runs != current_runs {
        return false;
    }
    let authorized_work_units = authorized
        .runs
        .iter()
        .flat_map(|cleanup| cleanup.work_units.iter())
        .map(|unit| {
            (
                unit.id.as_str(),
                unit.task_run_id.as_str(),
                unit.base_commit.as_str(),
                unit.worktree_path.as_str(),
                unit.branch.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    let current_work_units = current
        .runs
        .iter()
        .flat_map(|cleanup| cleanup.work_units.iter())
        .map(|unit| {
            (
                unit.id.as_str(),
                unit.task_run_id.as_str(),
                unit.base_commit.as_str(),
                unit.worktree_path.as_str(),
                unit.branch.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    authorized_work_units == current_work_units
}

fn recovery_cleanup_revision(
    issue: &StudioRecoveryIssue,
    scope: &RecoveryCleanupScope,
    resources: &[StudioRecoveryCleanupResource],
) -> String {
    let mut digest = Sha256::new();
    digest.update(issue.id.as_bytes());
    digest.update(scope.project_updated_at.unwrap_or_default().to_le_bytes());
    digest.update(scope.thread_updated_at.unwrap_or_default().to_le_bytes());
    let mut resource_index = 0;
    for cleanup in &scope.runs {
        digest.update(cleanup.run.id.as_bytes());
        digest.update(cleanup.run.updated_at.to_le_bytes());
        for unit in &cleanup.work_units {
            let resource = &resources[resource_index];
            resource_index += 1;
            digest.update(unit.id.as_bytes());
            digest.update(unit.status().as_str().as_bytes());
            digest.update(unit.worktree_disposition().as_str().as_bytes());
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
    }
    format!("{:x}", digest.finalize())
}

fn validate_snapshot(run: &TaskRunRecord, snapshot: &RepositorySnapshot) -> Result<()> {
    validate_snapshot_owner(run, snapshot)?;
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
