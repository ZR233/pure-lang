use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Context, Result, bail};
use tokio::sync::MutexGuard;

use super::git::{RepositorySnapshot, inspect_repository};
use super::{CreateTaskRun, TaskRunPhase, TaskRunRecord, TaskWorktreeOwnerSnapshot};
use crate::studio::ids::new_id;
use crate::studio::store::StudioStore;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BranchKey {
    common_dir: String,
    branch: String,
}

struct WorktreeRecoveryGroup {
    repositories: Vec<PathBuf>,
    owners: Vec<TaskWorktreeOwnerSnapshot>,
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
        Self {
            store,
            owned_process_leases: Mutex::new(HashMap::new()),
            allocation_lock: tokio::sync::Mutex::new(()),
            branch_mutation_lock: tokio::sync::Mutex::new(()),
            branch_mutation_owner: Arc::new(()),
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
        let snapshot = inspect_repository(repository, true).await?;
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

    pub(crate) async fn recover_active_tasks(&self) -> Result<Vec<TaskRunRecord>> {
        let mut prepared = Vec::new();
        let mut failed_agent_runs = HashSet::new();
        for run in self.store.list_active_task_runs().await? {
            let key = BranchKey::new(Path::new(&run.git_common_dir), &run.branch);
            if let Err(error) = acquire_process_lease(&key, &run.id) {
                self.block_run(&run, error.to_string()).await?;
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
                self.block_run(
                    &run,
                    format!("agent restart reconciliation failed: {error}"),
                )
                .await?;
                failed_agent_runs.insert(run.id.clone());
                continue;
            }
            prepared.push(run);
        }

        let owners = self.store.list_all_task_worktree_owners().await?;
        let (groups, run_groups) = resolve_worktree_recovery_groups(owners).await?;
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
                let affected = prepared
                    .iter()
                    .filter(|run| run_groups.get(&run.id) == Some(&key))
                    .cloned()
                    .collect::<Vec<_>>();
                if affected.is_empty() {
                    return Err(error).context("worktree restart reconciliation failed");
                }
                for run in affected {
                    self.block_run(
                        &run,
                        format!("worktree restart reconciliation failed: {error}"),
                    )
                    .await?;
                }
                continue;
            }
            successful_groups.insert(key);
        }

        let mut recovered = Vec::new();
        for run in prepared {
            if !run_groups
                .get(&run.id)
                .is_some_and(|group| successful_groups.contains(group))
            {
                continue;
            }
            if run.phase == TaskRunPhase::Merging {
                match self.recover_merging_run(&run).await {
                    Ok(super::merge::MergeRestartRecovery::Resume(recovered_run)) => {
                        recovered.push(*recovered_run);
                    }
                    Ok(super::merge::MergeRestartRecovery::Blocked) => {}
                    Err(error) => {
                        self.block_run(&run, format!("merge recovery failed: {error}"))
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
                    self.block_run(&run, format!("repository recovery failed: {error}"))
                        .await?;
                    continue;
                }
            };
            if let Err(reason) = validate_snapshot(&run, &snapshot) {
                self.block_run(&run, reason.to_string()).await?;
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
                    self.block_run(&run, format!("conflict recovery failed: {error}"))
                        .await?;
                    continue;
                }
            }
            recovered.push(run);
        }
        Ok(recovered)
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

async fn resolve_worktree_recovery_groups(
    owners: Vec<TaskWorktreeOwnerSnapshot>,
) -> Result<(
    HashMap<String, WorktreeRecoveryGroup>,
    HashMap<String, String>,
)> {
    let mut repositories = HashMap::new();
    for owner in &owners {
        let workspace = workspace_key(&owner.run.workspace_root);
        if repositories.contains_key(&workspace) {
            continue;
        }
        let snapshot = inspect_repository(&owner.run.workspace_root, false)
            .await
            .with_context(|| {
                format!(
                    "failed to resolve Git common directory for known task workspace {}",
                    owner.run.workspace_root
                )
            })?;
        repositories.insert(workspace, snapshot);
    }

    let mut groups = HashMap::<String, WorktreeRecoveryGroup>::new();
    let mut run_groups = HashMap::new();
    for owner in owners {
        let workspace = workspace_key(&owner.run.workspace_root);
        let snapshot = repositories
            .get(&workspace)
            .context("known task workspace inspection disappeared")?;
        let group_key = canonical_path_key(&snapshot.git_common_dir);
        let group = groups
            .entry(group_key.clone())
            .or_insert_with(|| WorktreeRecoveryGroup {
                repositories: Vec::new(),
                owners: Vec::new(),
            });
        let repository_key = canonical_path_key(&snapshot.workspace_root);
        if !group
            .repositories
            .iter()
            .any(|repository| canonical_path_key(repository) == repository_key)
        {
            group.repositories.push(snapshot.workspace_root.clone());
        }
        run_groups.insert(owner.run.id.clone(), group_key);
        group.owners.push(owner);
    }
    Ok((groups, run_groups))
}

fn workspace_key(workspace: &str) -> String {
    let workspace = workspace.replace('\\', "/");
    if cfg!(windows) {
        workspace.to_lowercase()
    } else {
        workspace
    }
}

fn canonical_path_key(path: &Path) -> String {
    workspace_key(&path.to_string_lossy())
}

impl Drop for TaskCoordinator {
    fn drop(&mut self) {
        self.suspend();
    }
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
